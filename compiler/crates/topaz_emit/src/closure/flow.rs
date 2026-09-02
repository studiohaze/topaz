use crate::*;

impl FlowCtx {
    /// The Rust statements that drain `stacks[from..]` inner→outer (`run_defers` LIFO
    /// per stack), for an early exit that crosses those blocks. Empty when `from` is at
    /// the top (no block stacks to drain).
    pub(crate) fn drain_from(&self, from: usize) -> String {
        let mut out = String::new();
        for name in self.stacks[from..].iter().rev() {
            out.push_str(&format!("run_defers(&{name}, &cx).await; "));
        }
        out
    }

    /// Resolve a `break`/`continue` target loop frame index (an index
    /// into `loop_markers`/`loop_frames`). `None` label → the innermost loop;
    /// `Some(name)` → the nearest enclosing `loop 'name`. Returns `None` when no
    /// in-scope loop matches (an unlabeled control outside any loop, or an unknown
    /// label) — the emitter then refuses (it never emits a control statement that
    /// would not compile).
    pub(crate) fn loop_target(&self, label: Option<&str>) -> Option<usize> {
        match label {
            None => self.loop_markers.len().checked_sub(1),
            Some(name) => self.loop_frames.iter().rposition(
                |k| matches!(k, LoopFrameKind::Value { src_label: Some(l), .. } if l == name),
            ),
        }
    }
}

/// §14 emit a CLOSURE body (lambda / defer action / concurrent arm) with a FRESH flow:
/// the body is its OWN async block with its OWN `__defers`, so its early exits must drain
/// ITS block defers, not the enclosing scope's (whose defer stacks are not in its
/// captures — draining them would be wrong AND a non-`'static` reference). Saves/clears
/// the shared `FlowCtx`, runs `emit`, restores.
pub(crate) fn with_reset_flow<T, F>(aliases: &Aliases<'_, '_>, emit: F) -> Result<T, EmitError>
where
    F: FnOnce(&Aliases<'_, '_>) -> Result<T, EmitError>,
{
    let saved = {
        let mut f = aliases.flow.borrow_mut();
        // A closure body has its own loop context (loop control may not
        // cross a function/lambda boundary), so clear the loop frames too — a
        // `break`/`continue` inside the closure cannot see an enclosing loop.
        let s = (
            std::mem::take(&mut f.stacks),
            std::mem::take(&mut f.loop_markers),
            f.fn_base,
            std::mem::take(&mut f.loop_frames),
        );
        f.fn_base = 0;
        s
    };
    let result = emit(aliases);
    {
        let mut f = aliases.flow.borrow_mut();
        f.stacks = saved.0;
        f.loop_markers = saved.1;
        f.fn_base = saved.2;
        f.loop_frames = saved.3;
    }
    result
}
