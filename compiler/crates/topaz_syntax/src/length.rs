//! R17 exact grid arithmetic. All callers round at the final value boundary.
use crate::ast::BinaryOp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LengthUnit {
    Pt,
    Mm,
    Cm,
    In,
    Em,
}
impl LengthUnit {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Pt => "pt",
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::In => "in",
            Self::Em => "em",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Pt => "Pt",
            Self::Mm => "Mm",
            Self::Cm => "Cm",
            Self::In => "In",
            Self::Em => "Em",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "Pt" => Self::Pt,
            "Mm" => Self::Mm,
            "Cm" => Self::Cm,
            "In" => Self::In,
            "Em" => Self::Em,
            _ => return None,
        })
    }
    pub fn from_suffix(s: &str) -> Option<Self> {
        Some(match s {
            "pt" => Self::Pt,
            "mm" => Self::Mm,
            "cm" => Self::Cm,
            "in" => Self::In,
            "em" => Self::Em,
            _ => return None,
        })
    }
    pub fn nominal(self) -> String {
        format!("Length<{}>", self.name())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthError {
    Syntax,
    Overflow,
    DivisionByZero,
    FontSize,
}
impl LengthError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Syntax => "malformed Length literal",
            Self::Overflow => "Length result is not representable",
            Self::DivisionByZero => "Length division by zero",
            Self::FontSize => "Length.resolve font size must be positive",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Length {
    pub unit: LengthUnit,
    pub ticks: i64,
}
fn signed(m: u128, negative: bool) -> Result<i64, LengthError> {
    let bound = if negative {
        1u128 << 63
    } else {
        i64::MAX as u128
    };
    if m > bound {
        return Err(LengthError::Overflow);
    }
    Ok(if negative {
        (-(m as i128)) as i64
    } else {
        m as i64
    })
}
fn rounded(n: u128, d: u128) -> u128 {
    let q = n / d;
    let r = n % d;
    q + u128::from(r > d - r || (r == d - r && q & 1 == 1))
}
pub fn length_fraction(n: i128, d: i128) -> Result<i64, LengthError> {
    if d == 0 {
        return Err(LengthError::DivisionByZero);
    }
    signed(
        rounded(n.unsigned_abs(), d.unsigned_abs()),
        (n < 0) != (d < 0),
    )
}
/// Validate separators before removing them. Each side of a decimal point is a digit run.
pub fn length_literal_parts(text: &str) -> Result<(&str, LengthUnit), LengthError> {
    if text.len() < 3 {
        return Err(LengthError::Syntax);
    }
    let split = text.len() - 2;
    if !text.is_char_boundary(split) {
        return Err(LengthError::Syntax);
    }
    let unit = LengthUnit::from_suffix(&text[split..]).ok_or(LengthError::Syntax)?;
    let number = &text[..split];
    crate::decimal_literal_parts(number).map_err(|_| LengthError::Syntax)?;
    Ok((number, unit))
}
/// Decimal long multiplication/division avoids a bounded intermediate magnitude.
/// The tail and small division remainder decide one final nearest-even rounding.
pub fn decode_length(text: &str, negative: bool) -> Result<Length, LengthError> {
    let (number, unit) = length_literal_parts(text)?;
    let fraction = number
        .split_once('.')
        .map_or(0, |(_, f)| f.bytes().filter(|b| *b != b'_').count());
    let (mul, div) = match unit {
        LengthUnit::Pt | LengthUnit::Em => (65536u64, 1u64),
        LengthUnit::In => (4718592, 1),
        LengthUnit::Mm => (23592960, 127),
        LengthUnit::Cm => (235929600, 127),
    };
    let mut digits: Vec<u8> = number
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|d| d - b'0')
        .collect();
    let mut carry = 0u64;
    for d in digits.iter_mut().rev() {
        let n = u64::from(*d) * mul + carry;
        *d = (n % 10) as u8;
        carry = n / 10;
    }
    let mut prefix = Vec::new();
    while carry > 0 {
        prefix.push((carry % 10) as u8);
        carry /= 10;
    }
    prefix.reverse();
    prefix.extend(digits);
    digits = prefix;
    let mut remainder = 0u64;
    for d in &mut digits {
        let n = remainder * 10 + u64::from(*d);
        *d = (n / div) as u8;
        remainder = n % div;
    }
    if digits.len() <= fraction {
        let mut padded = vec![0; fraction + 1 - digits.len()];
        padded.extend(digits);
        digits = padded;
    }
    let boundary = digits.len() - fraction;
    let mut magnitude = 0u128;
    for d in &digits[..boundary] {
        magnitude = magnitude
            .checked_mul(10)
            .and_then(|n| n.checked_add(u128::from(*d)))
            .ok_or(LengthError::Overflow)?;
    }
    let (greater, tie) = if fraction == 0 {
        (remainder > div - remainder, remainder == div - remainder)
    } else {
        let tail = &digits[boundary..];
        let rest = tail[1..].iter().any(|d| *d != 0) || remainder != 0;
        (tail[0] > 5 || (tail[0] == 5 && rest), tail[0] == 5 && !rest)
    };
    if greater || (tie && magnitude & 1 == 1) {
        magnitude = magnitude.checked_add(1).ok_or(LengthError::Overflow)?;
    }
    Ok(Length {
        unit,
        ticks: signed(magnitude, negative)?,
    })
}
impl Length {
    pub fn neg(self) -> Result<Self, LengthError> {
        Ok(Self {
            ticks: self.ticks.checked_neg().ok_or(LengthError::Overflow)?,
            ..self
        })
    }
    pub fn add(self, b: Self) -> Result<Self, LengthError> {
        Ok(Self {
            ticks: self
                .ticks
                .checked_add(b.ticks)
                .ok_or(LengthError::Overflow)?,
            ..self
        })
    }
    pub fn sub(self, b: Self) -> Result<Self, LengthError> {
        Ok(Self {
            ticks: self
                .ticks
                .checked_sub(b.ticks)
                .ok_or(LengthError::Overflow)?,
            ..self
        })
    }
    pub fn mul_int(self, b: i64) -> Result<Self, LengthError> {
        Ok(Self {
            ticks: length_fraction(i128::from(self.ticks) * i128::from(b), 1)?,
            ..self
        })
    }
    pub fn div_int(self, b: i64) -> Result<Self, LengthError> {
        Ok(Self {
            ticks: length_fraction(i128::from(self.ticks), i128::from(b))?,
            ..self
        })
    }
    pub fn mul_float(self, b: f64) -> Result<Self, LengthError> {
        if !b.is_finite() {
            return Err(LengthError::Overflow);
        }
        let bits = b.to_bits();
        let exp = ((bits >> 52) & 2047) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let mantissa = if exp == 0 {
            fraction
        } else {
            fraction | (1u64 << 52)
        };
        let shift = if exp == 0 { -1074 } else { exp - 1023 - 52 };
        let n = u128::from(self.ticks.unsigned_abs()) * u128::from(mantissa);
        let magnitude = if n == 0 {
            0
        } else if shift >= 0 {
            let shift = shift as u32;
            if shift >= 128 || n > u128::MAX >> shift {
                return Err(LengthError::Overflow);
            }
            n << shift
        } else {
            let shift = (-shift) as u32;
            if shift >= 128 {
                0
            } else {
                rounded(n, 1u128 << shift)
            }
        };
        Ok(Self {
            ticks: signed(magnitude, (self.ticks < 0) != (bits >> 63 != 0))?,
            ..self
        })
    }
    pub fn ratio(self, b: Self) -> Result<f64, LengthError> {
        if b.ticks == 0 {
            return Err(LengthError::DivisionByZero);
        }
        if self.ticks == 0 {
            return Ok(if b.ticks < 0 { -0.0 } else { 0.0 });
        }
        let n = u128::from(self.ticks.unsigned_abs());
        let d = u128::from(b.ticks.unsigned_abs());
        let mut e = (128 - n.leading_zeros()) as i32 - (128 - d.leading_zeros()) as i32;
        if if e >= 0 {
            n < (d << e)
        } else {
            (n << (-e)) < d
        } {
            e -= 1;
        }
        let shift = 52 - e;
        let mut mantissa = if shift >= 0 {
            rounded(n << shift, d)
        } else {
            rounded(n, d << (-shift))
        };
        if mantissa == 1u128 << 53 {
            mantissa >>= 1;
            e += 1;
        }
        let sign = u64::from((self.ticks < 0) != (b.ticks < 0)) << 63;
        Ok(f64::from_bits(
            sign | (((e + 1023) as u64) << 52) | ((mantissa as u64) & ((1u64 << 52) - 1)),
        ))
    }
    pub fn convert(self, unit: LengthUnit) -> Self {
        Self { unit, ..self }
    }
    pub fn milli_points(self) -> Result<i64, LengthError> {
        length_fraction(i128::from(self.ticks) * 1000, 65536)
    }
    pub fn from_milli_points(n: i64) -> Result<Self, LengthError> {
        Ok(Self {
            unit: LengthUnit::Pt,
            ticks: length_fraction(i128::from(n) * 65536, 1000)?,
        })
    }
    pub fn resolve(self, font: Self) -> Result<Self, LengthError> {
        if font.ticks <= 0 {
            return Err(LengthError::FontSize);
        }
        Ok(Self {
            unit: LengthUnit::Pt,
            ticks: length_fraction(i128::from(self.ticks) * i128::from(font.ticks), 65536)?,
        })
    }
}
/// Common const/runtime result. Unit admission is performed by the typed caller.
#[derive(Debug, Clone, Copy)]
pub enum LengthScalar {
    Length(Length),
    Int(i64),
    Float(f64),
    Bool(bool),
}
pub fn length_binary(
    op: BinaryOp,
    a: LengthScalar,
    b: LengthScalar,
) -> Option<Result<LengthScalar, LengthError>> {
    use BinaryOp::*;
    use LengthScalar as S;
    Some(match (a, b) {
        (S::Length(a), S::Length(b)) if a.unit == b.unit => match op {
            Add => a.add(b).map(S::Length),
            Sub => a.sub(b).map(S::Length),
            Div => a.ratio(b).map(S::Float),
            Eq => Ok(S::Bool(a.ticks == b.ticks)),
            Ne => Ok(S::Bool(a.ticks != b.ticks)),
            Lt => Ok(S::Bool(a.ticks < b.ticks)),
            Le => Ok(S::Bool(a.ticks <= b.ticks)),
            Gt => Ok(S::Bool(a.ticks > b.ticks)),
            Ge => Ok(S::Bool(a.ticks >= b.ticks)),
            _ => return None,
        },
        (S::Length(a), S::Int(b)) => match op {
            Mul => a.mul_int(b).map(S::Length),
            Div => a.div_int(b).map(S::Length),
            _ => return None,
        },
        (S::Int(b), S::Length(a)) if op == Mul => a.mul_int(b).map(S::Length),
        (S::Length(a), S::Float(b)) | (S::Float(b), S::Length(a)) if op == Mul => {
            a.mul_float(b).map(S::Length)
        }
        _ => return None,
    })
}

/// Direct unary-minus literal admission, including parentheses, uses the signed
/// final grid range. An already computed Length still uses checked negation.
pub fn signed_length_literal(
    expr: &crate::ast::Expr,
    source: &str,
) -> Option<Result<Length, LengthError>> {
    use crate::ast::{ExprKind as E, UnaryOp};
    fn unparen(mut e: &crate::ast::Expr) -> &crate::ast::Expr {
        while let E::Paren(inner) = &e.kind {
            e = inner;
        }
        e
    }
    let expr = unparen(expr);
    let (literal, negative) = match &expr.kind {
        E::Length(_) => (expr, false),
        E::Unary {
            op: UnaryOp::Minus,
            operand,
        } if matches!(unparen(operand).kind, E::Length(_)) => (unparen(operand), true),
        _ => return None,
    };
    Some(decode_length(
        &source[literal.span.lo as usize..literal.span.hi as usize],
        negative,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    // Expectations are integers derived from 65536, 72, and 360/127, not a
    // second invocation of the decoder. These are product grid boundaries.
    #[test]
    fn literal_grid_units_and_exact_decimal_ties() {
        for (text, negative, expected) in [
            ("1pt", false, 65536),
            ("1in", false, 4718592),
            ("1mm", false, 185771),
            ("1cm", false, 1857713),
            ("1em", false, 65536),
            ("0.00000762939453125pt", false, 0),
            ("0.00002288818359375pt", false, 2),
            ("0.00002288818359375pt", true, -2),
            ("140737488355327.9999847412109375pt", false, i64::MAX),
            ("140737488355328pt", true, i64::MIN),
            ("1_000.00_5pt", false, 65536328),
        ] {
            assert_eq!(
                decode_length(text, negative).unwrap().ticks,
                expected,
                "{text}"
            );
        }
        assert_eq!(
            decode_length("140737488355328pt", false),
            Err(LengthError::Overflow)
        );
        assert_eq!(
            decode_length("140737488355328.0000152587890625pt", true),
            Err(LengthError::Overflow)
        );
        for text in [
            "1__0pt", "1_.0mm", "1._0em", "0x1pt", "0b1em", "1e2pt", "10_pt",
        ] {
            assert_eq!(
                decode_length(text, false),
                Err(LengthError::Syntax),
                "{text}"
            );
        }
        let long = format!("1.{}pt", "0".repeat(1000));
        assert_eq!(decode_length(&long, false).unwrap().ticks, 65536);
    }
    #[test]
    fn rational_arithmetic_and_float_bits_round_once() {
        let pt = |ticks| Length {
            unit: LengthUnit::Pt,
            ticks,
        };
        assert_eq!(pt(3).div_int(2).unwrap().ticks, 2);
        assert_eq!(pt(5).div_int(2).unwrap().ticks, 2);
        assert_eq!(pt(-3).div_int(2).unwrap().ticks, -2);
        assert_eq!(pt(i64::MIN).div_int(-1), Err(LengthError::Overflow));
        assert_eq!(pt(1).div_int(0), Err(LengthError::DivisionByZero));
        assert_eq!(pt(3).mul_float(0.5).unwrap().ticks, 2);
        assert_eq!(pt(-5).mul_float(0.5).unwrap().ticks, -2);
        assert_eq!(pt(i64::MAX).mul_float(f64::from_bits(1)).unwrap().ticks, 0);
        assert_eq!(pt(i64::MIN).mul_float(1.0).unwrap().ticks, i64::MIN);
        assert_eq!(pt(i64::MAX).mul_float(1.0).unwrap().ticks, i64::MAX);
        for f in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(pt(0).mul_float(f), Err(LengthError::Overflow));
        }
        assert_eq!(pt(1).ratio(pt(3)).unwrap().to_bits(), 0x3fd5555555555555);
        assert_eq!(
            pt(i64::MAX).ratio(pt(i64::MAX - 1)).unwrap().to_bits(),
            0x3ff0000000000000
        );
        assert_eq!(pt(4096).milli_points().unwrap(), 62);
        assert_eq!(Length::from_milli_points(1).unwrap().ticks, 66);
        let em = Length {
            unit: LengthUnit::Em,
            ticks: 32768,
        };
        assert_eq!(em.resolve(pt(3)).unwrap(), pt(2));
        assert_eq!(em.resolve(pt(0)), Err(LengthError::FontSize));
        assert_eq!(em.resolve(pt(-1)), Err(LengthError::FontSize));
        // 2^63*65536 has a wide intermediate, and this final result fits.
        assert_eq!(
            Length {
                unit: LengthUnit::Em,
                ticks: i64::MIN
            }
            .resolve(pt(65536))
            .unwrap()
            .ticks,
            i64::MIN
        );
    }
}

/// Scalar shape used for selective pure receiver admission. The caller supplies
/// resolved constructor identity from its actual generated-module facts; names
/// in source text never establish constructor provenance here.
pub fn constant_length_unit(
    expr: &crate::ast::Expr,
    identifier: &impl Fn(topaz_diag::Span) -> Option<LengthUnit>,
    constructor: &impl Fn(&crate::ast::Expr) -> Option<LengthUnit>,
    text: &impl Fn(topaz_diag::Span) -> String,
) -> Option<LengthUnit> {
    use crate::ast::{BinaryOp as B, ExprKind as E};
    match &expr.kind {
        E::Length(unit) => Some(*unit),
        E::Ident => identifier(expr.span),
        E::Paren(value) | E::Unary { operand: value, .. } => {
            constant_length_unit(value, identifier, constructor, text)
        }
        E::Binary { op, lhs, rhs } => {
            let left = constant_length_unit(lhs, identifier, constructor, text);
            let right = constant_length_unit(rhs, identifier, constructor, text);
            match op {
                B::Add | B::Sub if left == right => left,
                B::Mul if left.is_none() || right.is_none() => left.or(right),
                B::Div if right.is_none() => left,
                _ => None,
            }
        }
        E::Call { callee, .. } => {
            if let Some(unit) = constructor(expr) {
                return Some(unit);
            }
            let E::Member { object, field } = &callee.kind else {
                return None;
            };
            let unit = constant_length_unit(object, identifier, constructor, text)?;
            match (unit, text(field.span).as_str()) {
                (LengthUnit::Em, "resolve") => Some(LengthUnit::Pt),
                (_, "toPt") if unit != LengthUnit::Em => Some(LengthUnit::Pt),
                (_, "toMm") if unit != LengthUnit::Em => Some(LengthUnit::Mm),
                (_, "toCm") if unit != LengthUnit::Em => Some(LengthUnit::Cm),
                (_, "toIn") if unit != LengthUnit::Em => Some(LengthUnit::In),
                _ => None,
            }
        }
        _ => None,
    }
}
/// Shared exact receiver operation for const and boxed/native runtime callers.
pub fn length_member(
    value: Length,
    method: &str,
    font: Option<Length>,
) -> Option<Result<LengthScalar, LengthError>> {
    use LengthScalar as S;
    use LengthUnit as U;
    Some(match (value.unit, method) {
        (U::Em, "resolve") => value.resolve(font?).map(S::Length),
        (U::Em, _) => return None,
        (_, "scaledPoints") => Ok(S::Int(value.ticks)),
        (U::Pt, "toMilliPoints") => value.milli_points().map(S::Int),
        (_, "toPt") => Ok(S::Length(value.convert(U::Pt))),
        (_, "toMm") => Ok(S::Length(value.convert(U::Mm))),
        (_, "toCm") => Ok(S::Length(value.convert(U::Cm))),
        (_, "toIn") => Ok(S::Length(value.convert(U::In))),
        _ => return None,
    })
}
