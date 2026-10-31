//! The cooperative single-future driver (CDR-006 §4). It runs one program
//! future to completion on the calling thread; a `Pending` is a cooperative
//! yield, so the driver simply re-polls. The multi-arm executor owns scheduling,
//! while this remains the single-arm fast path.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineExceeded;

/// Drive `future` to completion on the calling thread. Nothing parks
/// and no waker registers work: the single-future model treats
/// `Pending` as "re-poll me", which is exactly a checkpoint yield, so
/// the loop re-polls until the future is `Ready`.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => continue,
        }
    }
}

/// Drive one cooperative future until completion or a monotonic deadline.
///
/// This is intentionally not advertised as arbitrary preemption: the deadline
/// is observed between polls, so generated service code must retain checkpoints
/// on potentially unbounded paths. Dropping the future on expiry is the actual
/// cancellation operation; no detached evaluation survives the return.
pub fn block_on_until<F: Future>(
    deadline: Instant,
    future: F,
) -> Result<F::Output, DeadlineExceeded> {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if Instant::now() >= deadline {
            return Err(DeadlineExceeded);
        }
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return Ok(output),
            Poll::Pending => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_runs_a_ready_future() {
        assert_eq!(block_on(async { 7 }), 7);
    }

    #[test]
    fn block_on_drives_a_pending_then_ready_future() {
        // The executor half of the compile-shape proof (CDR-006 §3): a
        // future that yields once (`Pending`) before completing must be
        // re-polled to completion. This proves the suspension path the
        // emitted call ABI relies on without the multi-arm executor.
        let mut yielded = false;
        let fut = std::future::poll_fn(move |cx| {
            if yielded {
                Poll::Ready(42)
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        });
        assert_eq!(block_on(fut), 42);
    }

    #[test]
    fn block_on_until_drops_pending_work_and_capacity_is_reusable() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::time::Duration;

        struct DropProbe(Rc<Cell<bool>>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let dropped = Rc::new(Cell::new(false));
        let probe = DropProbe(dropped.clone());
        let pending = std::future::poll_fn(move |_| {
            let _ = &probe;
            Poll::<()>::Pending
        });
        let deadline = Instant::now() + Duration::from_millis(2);
        assert_eq!(block_on_until(deadline, pending), Err(DeadlineExceeded));
        assert!(
            dropped.get(),
            "expired future must be dropped before return"
        );
        assert_eq!(
            block_on_until(Instant::now() + Duration::from_secs(1), async { 7 }),
            Ok(7),
            "the same executor thread remains reusable"
        );
    }
}
