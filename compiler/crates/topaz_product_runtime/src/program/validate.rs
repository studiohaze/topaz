use super::model::*;
use crate::*;

pub(crate) fn operation_requires_host(operation: &Operation) -> bool {
    operation.kind == "using"
        || (operation.kind == "expression/concurrent"
            && operation
                .operand_labels
                .iter()
                .any(|label| label.starts_with("timeout:")))
        || [
            operation.call_target.as_str(),
            operation.reference_identity.as_str(),
        ]
        .into_iter()
        .filter_map(direct_builtin)
        .any(|kind| {
            matches!(
                kind,
                Builtin::Print
                    | Builtin::Open
                    | Builtin::TestAssert
                    | Builtin::TestAssertEq
                    | Builtin::TestAssertNe
                    | Builtin::TestAssertContains
                    | Builtin::TestAssertOk
                    | Builtin::TestAssertErr
                    | Builtin::TestAssertSome
                    | Builtin::TestAssertNone
                    | Builtin::TestAssertGolden
            )
        })
        || ((operation.call_target == "builtin::FS" || operation.call_target == "builtin::fs")
            && matches!(
                operation.call_method.as_str(),
                "readText" | "writeText" | "readBytes" | "writeBytes" | "list"
            ))
        || operation.call_target.starts_with("builtin::__lispex")
}

pub(crate) fn direct_builtin(target: &str) -> Option<Builtin> {
    if let Some(name) = target.strip_prefix("builtin::")
        && let Some(kind) = Builtin::free(name)
    {
        return Some(kind);
    }
    Some(match target {
        "builtin::__lispexRule" => Builtin::LispexRule,
        "builtin::__lispexValueFromCanonical" => Builtin::LispexValueFromCanonical,
        "builtin::__lispexCanonicalBytes" => Builtin::LispexCanonicalBytes,
        "builtin::__lispexDefaultLimits" => Builtin::LispexDefaultLimits,
        "builtin::__lispexInspectRule" => Builtin::LispexInspectRule,
        "builtin::__lispexEvaluate" => Builtin::LispexEvaluate,
        "builtin::__lispexEvaluateWithEvidence" => Builtin::LispexEvaluateWithEvidence,
        "builtin::__lispexConsumerArtifactFromBytes" => Builtin::LispexConsumerArtifactFromBytes,
        "builtin::__lispexConsumerArtifactBytes" => Builtin::LispexConsumerArtifactBytes,
        "builtin::__lispexPortableCoreBytes" => Builtin::LispexPortableCoreBytes,
        "builtin::__lispexInspectConsumerArtifact" => Builtin::LispexInspectConsumerArtifact,
        "builtin::__lispexVerifyConsumerArtifact" => Builtin::LispexVerifyConsumerArtifact,
        "builtin::__lispexFreshReplay" => Builtin::LispexFreshReplay,
        _ => return None,
    })
}

pub(crate) fn uses_special_direct_dispatch(target: &str) -> bool {
    matches!(
        target,
        "resolver::discoveryWord"
            | "resolver::discoveryByte"
            | "resolver::discoveryIdentifierByte"
            | "raw::byteAt"
            | "raw::isAsciiDigit"
            | "raw::isIdentifierStart"
            | "raw::isIdentifierContinue"
            | "raw::utf8Width"
    ) || target.starts_with("std.http::")
        || target.starts_with("topaz.lispex-rule-handle/v1:")
        || target.starts_with("builtin::")
}
pub(crate) fn validate_operation_shape(
    operation: &Operation,
    stage: &str,
    require_call_plan: bool,
) -> Result<(), String> {
    let minimum_operands = match operation.kind.as_str() {
        "constant"
        | "expression/member"
        | "expression/optional-member"
        | "expression/call"
        | "expression/unary"
        | "expression/match"
        | "expression/result-propagation"
        | "pattern/literal" => 1,
        "expression/index"
        | "expression/binary"
        | "expression/if"
        | "expression/pipeline"
        | "expression/range"
        | "let"
        | "assignment"
        | "using"
        | "while" => 2,
        "expression/for" => 3,
        _ => return Ok(()),
    };
    if operation.operands.len() < minimum_operands {
        return Err(format!(
            "{stage} operation `{}` ({}) expects at least {minimum_operands} operand(s), found {}",
            operation.id,
            operation.kind,
            operation.operands.len()
        ));
    }
    if operation.call_evaluations.is_empty() {
        if require_call_plan
            && (operation.kind == "expression/pipeline"
                || (operation.kind == "expression/call" && !operation.call_callee_kind.is_empty()))
        {
            return Err(format!(
                "{stage} operation `{}` has no call evaluation plan",
                operation.id
            ));
        }
        if require_call_plan
            && operation.kind == "expression/call"
            && (!operation.call_callee_kind.is_empty()
                || !operation.call_target.is_empty()
                || !operation.call_method.is_empty()
                || operation.call_optional
                || operation.call_shadow_first
                || !operation.call_stage_method.is_empty()
                || !operation.call_arguments.is_empty())
        {
            return Err(format!(
                "{stage} unplanned pipeline stage `{}` owns call metadata",
                operation.id
            ));
        }
    } else {
        let inserted_lead_count = operation
            .call_arguments
            .iter()
            .filter(|argument| matches!(argument.binding, CallArgumentBinding::InsertedLead))
            .count();
        let pipe_lead_evaluation_count = operation
            .call_evaluations
            .iter()
            .filter(|evaluation| matches!(evaluation, CallEvaluation::PipeLead))
            .count();
        match (operation.kind.as_str(), operation.call_callee_kind.as_str()) {
            ("expression/call", "value" | "member") => {
                if operation.call_arguments.len() + 1 != operation.operands.len() {
                    return Err(format!(
                        "{stage} call operation `{}` has {} argument plans for {} operands",
                        operation.id,
                        operation.call_arguments.len(),
                        operation.operands.len().saturating_sub(1)
                    ));
                }
                if inserted_lead_count != 0 || pipe_lead_evaluation_count != 0 {
                    return Err(format!(
                        "{stage} direct call operation `{}` contains a pipeline lead",
                        operation.id
                    ));
                }
                if (operation.call_callee_kind == "value" && !operation.call_method.is_empty())
                    || (operation.call_callee_kind == "member" && operation.call_method.is_empty())
                    || !operation.call_stage_method.is_empty()
                    || (operation.call_optional && operation.call_callee_kind != "member")
                {
                    return Err(format!(
                        "{stage} direct call operation `{}` has inconsistent callee fields",
                        operation.id
                    ));
                }
            }
            ("expression/pipeline", "pipe") => {
                if operation.operands.len() != 2
                    || !matches!(
                        (inserted_lead_count, pipe_lead_evaluation_count),
                        (1, 0) | (0, 1)
                    )
                {
                    return Err(format!(
                        "{stage} pipeline operation `{}` has an invalid lead or stage shape",
                        operation.id
                    ));
                }
                if !operation.call_method.is_empty() {
                    return Err(format!(
                        "{stage} pipeline operation `{}` has inconsistent callee fields",
                        operation.id
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "{stage} operation `{}` has call evaluations for invalid callee kind `{}`",
                    operation.id, operation.call_callee_kind
                ));
            }
        }
        let mut evaluated_arguments = BTreeSet::new();
        let mut callee_evaluations = 0;
        let mut receiver_evaluations = 0;
        for evaluation in &operation.call_evaluations {
            match evaluation {
                CallEvaluation::Callee => callee_evaluations += 1,
                CallEvaluation::Receiver => receiver_evaluations += 1,
                CallEvaluation::Argument(index)
                    if *index >= operation.call_arguments.len()
                        || !evaluated_arguments.insert(*index) =>
                {
                    return Err(format!(
                        "{stage} operation `{}` has an invalid argument evaluation index `{index}`",
                        operation.id
                    ));
                }
                _ => {}
            }
        }
        let expected_callee_evaluations = usize::from(operation.call_callee_kind == "value");
        let evaluation_prefix_matches = if operation.kind == "expression/pipeline" {
            let lead_index = operation
                .call_arguments
                .iter()
                .position(|argument| matches!(argument.binding, CallArgumentBinding::InsertedLead));
            (matches!(
                (operation.call_evaluations.first(), lead_index),
                (Some(CallEvaluation::Argument(actual)), Some(expected)) if *actual == expected
            ) || matches!(
                (operation.call_evaluations.first(), lead_index),
                (Some(CallEvaluation::PipeLead), None)
            )) && matches!(
                operation.call_evaluations.get(1),
                Some(CallEvaluation::Receiver)
            ) && (!operation.call_optional
                || matches!(
                    operation.call_evaluations.get(2),
                    Some(CallEvaluation::OptionalGuard)
                ))
        } else {
            let head_matches = if expected_callee_evaluations == 1 {
                matches!(
                    operation.call_evaluations.first(),
                    Some(CallEvaluation::Callee)
                )
            } else {
                matches!(
                    operation.call_evaluations.first(),
                    Some(CallEvaluation::Receiver)
                )
            };
            head_matches
                && (!operation.call_optional
                    || matches!(
                        operation.call_evaluations.get(1),
                        Some(CallEvaluation::OptionalGuard)
                    ))
        };
        if evaluated_arguments.len() != operation.call_arguments.len()
            || callee_evaluations != expected_callee_evaluations
            || receiver_evaluations != 1 - expected_callee_evaluations
            || !evaluation_prefix_matches
            || operation
                .call_evaluations
                .iter()
                .filter(|evaluation| matches!(evaluation, CallEvaluation::OptionalGuard))
                .count()
                != usize::from(operation.call_optional)
        {
            return Err(format!(
                "{stage} operation `{}` has an incomplete call evaluation plan",
                operation.id
            ));
        }
        if operation.call_shadow_first && operation.call_callee_kind != "member" {
            return Err(format!(
                "{stage} operation `{}` gives shadow-first precedence to a non-member call",
                operation.id
            ));
        }
    }
    Ok(())
}
