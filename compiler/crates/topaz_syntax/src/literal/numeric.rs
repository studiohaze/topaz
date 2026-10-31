//! Source numeric grammar and exact signed magnitude, shared by front ends.
use crate::ast::{Expr, ExprKind, UnaryOp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericError {
    Syntax,
    Overflow,
}

pub fn valid_digit_run(run: &str, radix: u32) -> bool {
    let bytes = run.as_bytes();
    let digit = |b: u8| (b as char).is_digit(radix);
    !bytes.is_empty()
        && bytes.iter().enumerate().all(|(i, &b)| {
            digit(b)
                || (b == b'_'
                    && i > 0
                    && i + 1 < bytes.len()
                    && digit(bytes[i - 1])
                    && digit(bytes[i + 1]))
        })
}

/// Returns decimal integer/fraction runs without changing their source spelling.
pub fn decimal_literal_parts(text: &str) -> Result<(&str, Option<&str>), NumericError> {
    let mut parts = text.split('.');
    let integer = parts.next().unwrap_or("");
    let fraction = parts.next();
    if !valid_digit_run(integer, 10)
        || fraction.is_some_and(|run| !valid_digit_run(run, 10))
        || parts.next().is_some()
    {
        return Err(NumericError::Syntax);
    }
    Ok((integer, fraction))
}

pub fn integer_literal_parts(text: &str) -> Result<(&str, u32), NumericError> {
    let (digits, radix) = if text.starts_with("0x") || text.starts_with("0X") {
        (&text[2..], 16)
    } else if text.starts_with("0b") || text.starts_with("0B") {
        (&text[2..], 2)
    } else {
        (text, 10)
    };
    if !valid_digit_run(digits, radix) {
        return Err(NumericError::Syntax);
    }
    Ok((digits, radix))
}

/// Exact decimal magnitude for syntax observation, including out-of-range input.
pub fn integer_literal_decimal(text: &str) -> Result<String, NumericError> {
    let (digits, radix) = integer_literal_parts(text)?;
    let mut decimal = vec![0u8];
    for b in digits.bytes().filter(|&b| b != b'_') {
        let mut carry = (b as char).to_digit(radix).expect("validated digit");
        for digit in &mut decimal {
            let n = u32::from(*digit) * radix + carry;
            *digit = (n % 10) as u8;
            carry = n / 10;
        }
        while carry != 0 {
            decimal.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    Ok(decimal
        .into_iter()
        .rev()
        .map(|d| char::from(b'0' + d))
        .collect())
}

/// Grammar is checked before accumulation, so an oversized malformed run is syntax.
pub fn decode_integer_literal(text: &str, negative: bool) -> Result<i64, NumericError> {
    let (digits, radix) = integer_literal_parts(text)?;
    let bound = if negative {
        1u64 << 63
    } else {
        i64::MAX as u64
    };
    let mut magnitude = 0u64;
    for b in digits.bytes().filter(|&b| b != b'_') {
        magnitude = magnitude
            .checked_mul(radix as u64)
            .and_then(|n| n.checked_add((b as char).to_digit(radix)? as u64))
            .filter(|&n| n <= bound)
            .ok_or(NumericError::Overflow)?;
    }
    Ok(if negative {
        (-(magnitude as i128)) as i64
    } else {
        magnitude as i64
    })
}

pub fn decode_float_literal(text: &str) -> Result<f64, NumericError> {
    let (_, fraction) = decimal_literal_parts(text)?;
    if fraction.is_none() {
        return Err(NumericError::Syntax);
    }
    text.replace('_', "")
        .parse()
        .map_err(|_| NumericError::Syntax)
}

/// Parentheses preserve literal identity; an evaluated arithmetic expression does not.
pub fn signed_integer_literal(expr: &Expr, source: &str) -> Option<Result<i64, NumericError>> {
    fn bare(expr: &Expr) -> Option<&Expr> {
        match &expr.kind {
            ExprKind::Int => Some(expr),
            ExprKind::Paren(inner) => bare(inner),
            _ => None,
        }
    }
    let (literal, negative) = match &expr.kind {
        ExprKind::Int => (expr, false),
        ExprKind::Paren(inner) => return signed_integer_literal(inner, source),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            operand,
        } => (bare(operand)?, true),
        ExprKind::Unary {
            op: UnaryOp::Plus,
            operand,
        } => (bare(operand)?, false),
        _ => return None,
    };
    Some(decode_integer_literal(
        &source[literal.span.lo as usize..literal.span.hi as usize],
        negative,
    ))
}

pub fn signed_integer_literal_for_version(
    expr: &Expr,
    source: &str,
    version: crate::LangVersion,
) -> Option<Result<i64, NumericError>> {
    signed_integer_literal(expr, source).map(|value| {
        value.and_then(|value| {
            if version < crate::LangVersion::V5_21 && value == i64::MIN {
                Err(NumericError::Overflow)
            } else {
                Ok(value)
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_literals_validate_runs_before_exact_signed_bounds() {
        for source in [
            "9223372036854775808",
            "0x8000_0000_0000_0000",
            "0b1000000000000000000000000000000000000000000000000000000000000000",
        ] {
            assert_eq!(decode_integer_literal(source, true), Ok(i64::MIN));
            assert_eq!(
                decode_integer_literal(source, false),
                Err(NumericError::Overflow)
            );
        }
        assert_eq!(
            decode_integer_literal("0x7fff_ffff_ffff_ffff", false),
            Ok(i64::MAX)
        );
        assert_eq!(
            decode_integer_literal("0xffff_ffff_ffff_ffff", true),
            Err(NumericError::Overflow)
        );
        for source in ["1__0", "0x_ff", "0b2", "0x", "10_"] {
            assert_eq!(
                decode_integer_literal(source, false),
                Err(NumericError::Syntax)
            );
        }
        assert_eq!(
            decode_float_literal("12_345.67_89").unwrap().to_bits(),
            12345.6789f64.to_bits()
        );
        assert_eq!(
            crate::parse_duration_milliseconds("18_446_744_073_709_551_615ms"),
            Some(u64::MAX)
        );
        assert_eq!(
            crate::parse_duration_milliseconds("18446744073709551616ms"),
            None
        );
        assert_eq!(crate::parse_duration_milliseconds("0x10ms"), None);
        assert_eq!(
            crate::length::decode_length("1_0mm", false).unwrap().ticks,
            1857713
        );
        assert_eq!(
            crate::length::decode_length("140_737_488_355_328pt", true)
                .unwrap()
                .ticks,
            i64::MIN
        );
    }
}
