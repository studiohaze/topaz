//! §15 `concurrent` execution: the round-robin multi-arm join and the
//! cooperative checkpoint the emitter awaits at loop back-edges.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use topaz_value::{CallFuture, RtCx, RtError, Value};

/// A cooperative yield (§15): polls `Pending` once, then `Ready`. The emitter awaits
/// a fresh `checkpoint()` at each `while` back-edge, so a long-running (even infinite)
/// arm SUSPENDS and the round-robin scheduler can advance its siblings. Under the
/// single-future `block_on` driver a `Pending` is just a re-poll, so the checkpoint is
/// transparent there — a `while` loop completes exactly as before.
pub fn checkpoint() -> Checkpoint {
    Checkpoint(false)
}

#[must_use = "a checkpoint does nothing unless awaited"]
pub struct Checkpoint(bool);

impl Future for Checkpoint {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            // The original round-robin executor re-polls every arm itself, but
            // generated HTTP handlers are ordinary local futures on Tokio.
            // Schedule the completing poll so progress never depends on an
            // unrelated socket wakeup.
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// §4/§15 per-arm recursion-depth ISOLATION. Each `concurrent` arm runs with its OWN
/// call-depth counter, so arms suspended mid-call at a `while` back-edge do not
/// accumulate into one shared count (and an abandoned arm leaks none). On every poll it
/// swaps the arm's saved depth into the shared `RtCx`, drives the inner future, then
/// reads the updated depth back out and restores the ambient — mirroring the
/// interpreter's per-`ArmRun` `call_depth` swap. `saved` seeds from the AMBIENT depth
/// (the concurrent's own), so an arm's calls count from there exactly as the
/// interpreter's raw-eval arm does.
struct DepthScoped {
    /// `Option` so an ABANDONED pending arm can be dropped INSIDE its own depth scope
    /// (see `Drop`); `None` once the inner future has completed.
    inner: Option<CallFuture>,
    cx: RtCx,
    saved: usize,
}

impl Future for DepthScoped {
    type Output = Result<Value, RtError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // All fields are `Unpin` (`Option<Pin<Box<…>>>`, `Rc`-backed `RtCx`, `usize`).
        let this = self.get_mut();
        let inner = this
            .inner
            .as_mut()
            .expect("DepthScoped polled after completion");
        let ambient = this.cx.call_depth();
        this.cx.set_call_depth(this.saved);
        let result = inner.as_mut().poll(cx);
        this.saved = this.cx.call_depth();
        this.cx.set_call_depth(ambient);
        if result.is_ready() {
            // Completed cleanly INSIDE the scope above (its `CallDepthGuard`s already
            // dropped there) — release the future so `Drop` has nothing to clean.
            this.inner = None;
        }
        result
    }
}

impl Drop for DepthScoped {
    fn drop(&mut self) {
        // §4 an ABANDONED pending arm (a timed-out sibling): drop the inner future with
        // the arm's OWN (saved) depth current, so its live `CallDepthGuard`s decrement
        // the arm's ISOLATED counter — not the ambient (enclosing) depth, which they
        // would otherwise undercount, letting a later call wrongly slip under the cap.
        if let Some(inner) = self.inner.take() {
            let ambient = self.cx.call_depth();
            self.cx.set_call_depth(self.saved);
            drop(inner);
            self.cx.set_call_depth(ambient);
        }
    }
}

/// Wrap a `concurrent` arm future so it carries its own isolated call-depth scope
/// (see `DepthScoped`); `saved` seeds from the current (ambient) depth.
pub fn depth_scoped(inner: CallFuture, cx: RtCx) -> CallFuture {
    let saved = cx.call_depth();
    Box::pin(DepthScoped {
        inner: Some(inner),
        cx,
        saved,
    })
}

/// §15 `concurrent { a: e1, b: e2 }` (no timeout) evaluates to a record mapping each arm
/// NAME to the value its expression produces. Each arm arrives as a `CallFuture` — a
/// no-argument thunk of the arm expression that already captured its enclosing scope.
///
/// DETERMINISTIC ROUND-ROBIN: one poll-step per arm per round, in textual order,
/// repeating until all arms complete. An arm that SUSPENDS (at a `checkpoint()`) yields
/// its turn, so a later arm can surface its fault or result even while an earlier arm is
/// still running: a `while`-spinning earlier arm no longer blocks a later faulting one
/// (the in-order predecessor hung). A fault propagates the first arm, in poll order, to
/// fault — with no deadline every fault is observable (§15). The record is `BTreeMap`-keyed
/// by arm name, exactly as the interpreter's `step_concurrent`.
///
/// SUSPENSION depends on the emitter inserting `checkpoint().await` at each `while`
/// back-edge, so this fixes `while`-based non-termination. A NON-`while` non-terminating
/// arm does not yet yield: `for` is bounded and direct `function` recursion is refused,
/// but a recursive CLOSURE call through a captured mutable cell would still starve the
/// scheduler (and overflow the stack) — the per-call checkpoint that covers it is a later
/// step.
///
/// SPEC §15 is INFORMATIVE about the schedule ("Implementations may choose different
/// internal strategies"), so the exact cross-arm interleaving and the racing-fault winner
/// are implementation-defined; the differential fixtures stay where the result, the
/// arm-local effects, and a single fault are order-insensitive (effects across arms and
/// abandonment-truncation are checked once the harness allows dual expectations). The
/// `timeout`/`else` deadline form is [`concurrent_join_timeout`].
pub async fn concurrent_join(arms: Vec<(String, CallFuture)>) -> Result<Value, RtError> {
    ConcurrentJoin {
        pending: arms,
        done: BTreeMap::new(),
    }
    .await
}

struct ConcurrentJoin {
    pending: Vec<(String, CallFuture)>,
    done: BTreeMap<String, Value>,
}

impl Future for ConcurrentJoin {
    type Output = Result<Value, RtError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        let mut i = 0;
        while i < me.pending.len() {
            match me.pending[i].1.as_mut().poll(cx) {
                Poll::Ready(Ok(value)) => {
                    let (name, _) = me.pending.remove(i);
                    me.done.insert(name, value);
                    // The arm at `i` shifted down — re-poll it next, do not advance.
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => i += 1,
            }
        }
        if me.pending.is_empty() {
            Poll::Ready(Ok(Value::record(std::mem::take(&mut me.done))))
        } else {
            Poll::Pending
        }
    }
}

/// §15 `concurrent(timeout: d) { arms } else { fallback }`: the round-robin join with a
/// DEADLINE. The deadline is sampled ONCE at entry — `now_millis() + ms`, the emitter
/// having parsed the duration literal `d` to `ms`. Each round polls the pending arms in
/// textual order exactly as [`concurrent_join`], and — mirroring the interpreter, which
/// samples expiry after EVERY arm's quantum — checks the deadline AFTER EACH arm: the moment
/// `now_millis() >= deadline` with any arm still pending, those arms are ABANDONED (§15
/// structured concurrency) and the `else` future drives to completion as the value. A fast
/// arm still beats the deadline (it is removed before the check), but a `0ms` deadline
/// reaches the else right after the FIRST arm even if a later arm could itself complete; only
/// when EVERY arm finishes before the deadline is sampled expired does the record become the
/// value (the `else` is dead). A fault sampled BEFORE the deadline propagates; one at/after it
/// is abandoned work the deadline owns (→ else) — mirroring the interpreter's `if Err &&
/// !expired` then `if expired && any-not-done`.
///
/// CLOCK: the differential harness runs both engines on a FROZEN test clock (`now_millis()`
/// ≡ 0), so the verified fixtures are the two outcomes that clock pins deterministically: a
/// `timeout: 0ms` is always already expired (`0 >= 0` → else past any still-pending arm), and
/// any non-zero timeout is never reached (→ the plain record, or a pre-deadline fault). A
/// timing-DEPENDENT deadline — where the engines' differing progress granularity (the
/// interpreter's 50-frame quantum per arm vs this executor's poll-to-suspension) would sample
/// an ADVANCING clock a different number of times — needs the virtual-clock model, a later
/// step. The lowering is shared; only the frozen-clock boundary forms are differentially
/// verified.
pub async fn concurrent_join_timeout(
    cx: RtCx,
    ms: u64,
    arms: Vec<(String, CallFuture)>,
    else_fut: CallFuture,
) -> Result<Value, RtError> {
    let deadline = cx.host().now_millis().saturating_add(ms);
    ConcurrentTimeout {
        cx,
        deadline,
        pending: arms,
        done: BTreeMap::new(),
        else_fut,
        driving_else: false,
    }
    .await
}

struct ConcurrentTimeout {
    cx: RtCx,
    deadline: u64,
    pending: Vec<(String, CallFuture)>,
    done: BTreeMap<String, Value>,
    else_fut: CallFuture,
    driving_else: bool,
}

impl ConcurrentTimeout {
    /// Abandon every arm and partial result before the else branch starts. This
    /// matches the interpreter dropping its `ConcurrentState` at the timeout
    /// transition and releases suspended arm captures before else effects run.
    fn begin_else(&mut self, cx: &mut Context<'_>) -> Poll<Result<Value, RtError>> {
        self.pending.clear();
        self.done.clear();
        self.driving_else = true;
        self.else_fut.as_mut().poll(cx)
    }
}

impl Future for ConcurrentTimeout {
    type Output = Result<Value, RtError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        // Once the deadline has fired the arms are abandoned (§15) and only the else
        // future remains to drive.
        if me.driving_else {
            return me.else_fut.as_mut().poll(cx);
        }
        // One round, textual order. The interpreter runs an arm's quantum then samples
        // expiry AFTER EACH arm — so check the deadline per arm here too, not once per
        // round. A fast arm completing still beats the deadline (it is removed before the
        // check), but a `0ms` deadline reaches the else right after the FIRST arm even when
        // a later arm could itself complete.
        let mut i = 0;
        while i < me.pending.len() {
            match me.pending[i].1.as_mut().poll(cx) {
                Poll::Ready(Ok(value)) => {
                    let (name, _) = me.pending.remove(i);
                    me.done.insert(name, value);
                }
                Poll::Ready(Err(error)) => {
                    // A fault before the deadline propagates; one at/after it is abandoned
                    // work the deadline owns — the else path produces the value instead.
                    if me.cx.host().now_millis() < me.deadline {
                        return Poll::Ready(Err(error));
                    }
                    return me.begin_else(cx);
                }
                Poll::Pending => i += 1,
            }
            // (b) per-arm deadline check (the interpreter's `if expired && any-not-done`):
            // expired with any arm still pending → abandon them and run the else.
            if !me.pending.is_empty() && me.cx.host().now_millis() >= me.deadline {
                return me.begin_else(cx);
            }
        }
        if me.pending.is_empty() {
            // Every arm completed before the deadline was sampled expired: the record,
            // exactly as the no-timeout join. The else is dead.
            Poll::Ready(Ok(Value::record(std::mem::take(&mut me.done))))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::future::pending;
    use std::rc::Rc;
    use std::task::Waker;
    use topaz_value::{FileId, Host, ResourceId, Span, codes, fault};

    /// A minimal host whose clock is frozen at 0, so a `0ms` deadline is always already
    /// expired (`0 >= 0`) — enough to drive the timeout's else path in a unit test.
    struct FrozenClock;
    impl Host for FrozenClock {
        fn print(&self, _line: &str) {}
        fn open(&self, _path: &str) -> Result<ResourceId, String> {
            Err("no host resources".to_string())
        }
        fn read(&self, _handle: ResourceId) -> Result<String, String> {
            Err("no host resources".to_string())
        }
        fn write(&self, _handle: ResourceId, _s: &str) -> Result<(), String> {
            Err("no host resources".to_string())
        }
        fn close(&self, _handle: ResourceId) {}
        fn now_millis(&self) -> u64 {
            0
        }
        fn defer_error(&self, _rendered: &str) {}
        fn lispex_application(
            &self,
            _request: topaz_value::LispexApplicationRequest,
        ) -> topaz_value::LispexApplicationResponse {
            topaz_value::LispexApplicationResponse::OperationalFault {
                code: "target-unavailable".into(),
                detail: None,
            }
        }
    }

    #[test]
    fn round_robin_surfaces_a_fault_past_a_suspended_arm() {
        // Arm `a` SUSPENDS forever (always `Pending` — a spinning arm); arm `b` FAULTS.
        // The round-robin must surface `b`'s fault in ONE poll (it polls `a` → Pending,
        // then `b` → the fault), NOT block on `a` (the in-order predecessor hung here).
        // One poll, so the test cannot hang regardless of a future regression.
        let spinner: CallFuture = Box::pin(pending());
        let faulter: CallFuture =
            Box::pin(async { Err(fault(codes::GUARD_TYPE, "boom", Span::new(FileId(0), 0, 0))) });
        let mut fut = Box::pin(concurrent_join(vec![
            ("a".to_string(), spinner),
            ("b".to_string(), faulter),
        ]));
        let mut cx = Context::from_waker(Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(Err(e)) => assert_eq!(e.message, "boom"),
            other => panic!("expected b's fault in one round-robin poll, got {other:?}"),
        }
    }

    #[test]
    fn checkpoint_yields_once_then_completes() {
        // The cooperative yield: one `Pending`, then `Ready` — the suspension point a
        // `while` back-edge awaits.
        #[derive(Default)]
        struct WakeProbe(std::sync::atomic::AtomicUsize);
        impl std::task::Wake for WakeProbe {
            fn wake(self: std::sync::Arc<Self>) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let probe = std::sync::Arc::new(WakeProbe::default());
        let waker = Waker::from(probe.clone());
        let mut cp = Box::pin(checkpoint());
        let mut cx = Context::from_waker(&waker);
        assert!(cp.as_mut().poll(&mut cx).is_pending());
        assert_eq!(probe.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(cp.as_mut().poll(&mut cx).is_ready());
    }

    #[test]
    fn timeout_fires_the_else_past_a_suspended_arm() {
        // A `0ms` deadline (frozen clock → already expired) with an arm that SUSPENDS
        // forever: the deadline fires, the pending arm is abandoned, and the else value
        // becomes the result. One poll, so the test cannot hang regardless of a regression.
        struct DropProbe(Rc<Cell<bool>>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let abandoned = Rc::new(Cell::new(false));
        let arm_probe = abandoned.clone();
        let spinner: CallFuture = Box::pin(async move {
            let _probe = DropProbe(arm_probe);
            pending::<()>().await;
            Ok(Value::Unit)
        });
        let else_probe = abandoned.clone();
        let else_fut: CallFuture = Box::pin(async move { Ok(Value::Bool(else_probe.get())) });
        let cx = RtCx::new(Rc::new(FrozenClock));
        let mut fut = Box::pin(concurrent_join_timeout(
            cx,
            0,
            vec![("slow".to_string(), spinner)],
            else_fut,
        ));
        let mut tcx = Context::from_waker(Waker::noop());
        match fut.as_mut().poll(&mut tcx) {
            Poll::Ready(Ok(Value::Bool(true))) => {}
            other => panic!("expected the else to observe the abandoned arm drop, got {other:?}"),
        }
    }
}
