//! Pure two's-complement operations shared by constant facts and runtime values.
//! Literal decoding and signed-magnitude admission live with the literal owner.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerBitOperation {
    And,
    Or,
    Xor,
    Shift,
}

/// Eligibility only: no call, literal parsing, or evaluation occurs here.
pub fn constant_shape(
    expr: &crate::ast::Expr,
    text: &impl Fn(topaz_diag::Span) -> String,
    integer_identifier: &impl Fn(topaz_diag::Span) -> bool,
) -> bool {
    use crate::ast::{BinaryOp, CallArg, ExprKind, UnaryOp};
    match &expr.kind {
        ExprKind::Int => true,
        ExprKind::Ident => integer_identifier(expr.span),
        ExprKind::Paren(value)
        | ExprKind::Unary {
            op: UnaryOp::Plus | UnaryOp::Minus,
            operand: value,
        } => constant_shape(value, text, integer_identifier),
        ExprKind::Binary {
            op:
                BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Rem
                | BinaryOp::Pow,
            lhs,
            rhs,
        } => {
            constant_shape(lhs, text, integer_identifier)
                && constant_shape(rhs, text, integer_identifier)
        }
        ExprKind::Call { callee, args, .. } => {
            let ExprKind::Member { object, field } = &callee.kind else {
                return false;
            };
            let Some(operation) = IntegerBitOperation::from_member(&text(field.span)) else {
                return false;
            };
            let [arg] = args.as_slice() else {
                return false;
            };
            let expected = if operation == IntegerBitOperation::Shift {
                "count"
            } else {
                "other"
            };
            let value = match arg {
                CallArg::Positional(value) => value,
                CallArg::Named { name, value } if text(name.span) == expected => value,
                _ => return false,
            };
            constant_shape(object, text, integer_identifier)
                && constant_shape(value, text, integer_identifier)
        }
        _ => false,
    }
}

impl IntegerBitOperation {
    pub fn from_member(member: &str) -> Option<Self> {
        match member {
            "and" => Some(Self::And),
            "or" => Some(Self::Or),
            "xor" => Some(Self::Xor),
            "shift" => Some(Self::Shift),
            _ => None,
        }
    }

    pub fn apply(self, left: i64, right: i64) -> Result<i64, &'static str> {
        Ok(match self {
            Self::And => left & right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
            Self::Shift => {
                if !(-63..=63).contains(&right) {
                    return Err("integer shift count must be between -63 and 63");
                }
                if right >= 0 {
                    ((left as u64) << (right as u32)) as i64
                } else {
                    left >> ((-right) as u32)
                }
            }
        })
    }
}
