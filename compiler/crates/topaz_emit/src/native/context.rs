use super::*;

/// A native refusal: a structured `TPZ6002` the caller turns into a boxed
/// fallback. The `what` names the construct for diagnostics/tests; the SPAN is
/// attached by [`crate::EmitError::at`] as it unwinds (first-wins).
pub(super) fn decline(what: &'static str) -> EmitError {
    EmitError::native_declined(what)
}

impl Ctx<'_> {
    /// Cross-check a local against the typed HIR: the checker MUST have recorded
    /// this name at this span with exactly this `MonoTy`, else the native fact is
    /// unanchored and the program refuses (soundness rule). A native SCALAR local
    /// confirms its scalar `MonoTy`; a boxed `Array<scalar>` boundary local
    /// confirms `MonoTy::Boxed` (the checker agrees it is non-scalar/boxed — so
    /// the native ELEMENT type rests on the syntactic `Array<E>` annotation, which
    /// a clean check has verified).
    pub(super) fn confirm_local(
        &self,
        name: &str,
        span: Span,
        mono: MonoTy,
    ) -> Result<(), EmitError> {
        match self.hir_locals.get(name, span) {
            Some(recorded) if recorded == mono => Ok(()),
            // The HIR recorded a different repr, or did not record this binding at
            // all — refuse, never lower a native binding the checker did not bless.
            _ => Err(decline("a local the typed HIR did not confirm")),
        }
    }

    pub(super) fn confirm_byte_record_param(
        &self,
        name: &str,
        span: Span,
    ) -> Result<(), EmitError> {
        let Some(function_span) = self.current_function else {
            return Err(decline("a byte record parameter outside a native function"));
        };
        if self
            .byte_record_params
            .iter()
            .any(|(function, recorded_name, recorded_span, _)| {
                *function == function_span && recorded_name == name && *recorded_span == span
            })
        {
            Ok(())
        } else {
            Err(decline("a record parameter the typed HIR did not confirm"))
        }
    }

    pub(super) fn byte_projection(
        &self,
        local_name: &str,
        local_span: Span,
        value: &Expr,
        scope: &[NativeLocal],
    ) -> Option<&ByteProjectionProof> {
        let function_span = self.current_function?;
        let ExprKind::Member { object, field } = &value.kind else {
            return None;
        };
        if !matches!(object.kind, ExprKind::Ident) {
            return None;
        }
        let receiver = text(self.src, object.span);
        let field_name = text(self.src, field.span);
        let local = scope
            .iter()
            .rev()
            .find(|local| local.name == receiver && local.is_byte_record())?;
        let _ = local;
        self.byte_projections.iter().find(|fact| {
            fact.function_span == function_span
                && fact.receiver_name == receiver
                && fact.field == field_name
                && fact.expression_span == value.span
                && fact.local_name == local_name
                && fact.local_span == local_span
                && self
                    .byte_record_params
                    .iter()
                    .any(|(function, name, span, _)| {
                        *function == function_span
                            && name == receiver
                            && *span == fact.receiver_span
                    })
        })
    }

    pub(super) fn is_math_namespace(&self, name: &str) -> bool {
        name == "Math" || self.math_namespaces.iter().any(|alias| alias == name)
    }
}
