use super::*;

impl DecimalData {
    pub(super) fn normalize(mut coeff: BigIntData, mut scale: u32) -> Self {
        while scale > 0 && !coeff.is_zero() {
            let abs = BigIntData::normalize(1, coeff.limbs.to_vec());
            let (q, rem) = abs.div_rem_small(10);
            if rem != 0 {
                break;
            }
            coeff = BigIntData::normalize(coeff.sign, q.limbs.to_vec());
            scale -= 1;
        }
        if coeff.is_zero() {
            scale = 0;
        }
        Self { coeff, scale }
    }

    pub(super) fn from_i64(n: i64) -> Self {
        Self::normalize(BigIntData::from_i64(n), 0)
    }

    pub(super) fn parse(text: &str) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let bytes = text.as_bytes();
        let (sign, rest) = match bytes[0] {
            b'-' => (-1, &bytes[1..]),
            b'+' => (1, &bytes[1..]),
            _ => (1, bytes),
        };
        if rest.is_empty() {
            return None;
        }
        let mut digits = Vec::with_capacity(rest.len());
        let mut seen_dot = false;
        let mut frac = 0usize;
        for &b in rest {
            match b {
                b'.' if !seen_dot => seen_dot = true,
                b'.' => return None,
                b'0'..=b'9' => {
                    digits.push(b);
                    if seen_dot {
                        frac += 1;
                    }
                }
                _ => return None,
            }
        }
        if digits.is_empty() || (seen_dot && frac == 0) {
            return None;
        }
        let scale: u32 = frac.try_into().ok()?;
        let mut coeff = BigIntData::zero();
        for digit in digits {
            coeff = coeff.mul_small(10).add_small((digit - b'0') as u32);
        }
        Some(Self::normalize(
            BigIntData::normalize(sign, coeff.limbs.to_vec()),
            scale,
        ))
    }

    pub(super) fn neg(&self) -> Self {
        Self::normalize(self.coeff.neg(), self.scale)
    }

    pub(super) fn to_string_canonical(&self) -> String {
        if self.coeff.is_zero() {
            return "0".to_string();
        }
        let mut digits = BigIntData::normalize(1, self.coeff.limbs.to_vec()).to_string_radix(10);
        if self.scale == 0 {
            if self.coeff.sign < 0 {
                digits.insert(0, '-');
            }
            return digits;
        }
        let scale = self.scale as usize;
        let mut out = String::new();
        if self.coeff.sign < 0 {
            out.push('-');
        }
        if digits.len() <= scale {
            out.push_str("0.");
            for _ in 0..(scale - digits.len()) {
                out.push('0');
            }
            out.push_str(&digits);
        } else {
            let split = digits.len() - scale;
            out.push_str(&digits[..split]);
            out.push('.');
            out.push_str(&digits[split..]);
        }
        out
    }

    pub(super) fn to_i64(&self) -> Option<i64> {
        if self.scale == 0 {
            self.coeff.to_i64()
        } else {
            None
        }
    }

    pub(super) fn pow10(exp: u32) -> BigIntData {
        BigIntData::from_i64(1).mul_pow10(exp)
    }

    pub(super) fn abs_coeff(&self) -> BigIntData {
        BigIntData::normalize(1, self.coeff.limbs.to_vec())
    }

    pub(super) fn should_round_increment(
        sign: i8,
        quotient_abs: &BigIntData,
        remainder_abs: &BigIntData,
        divisor_abs: &BigIntData,
        mode: RoundingMode,
    ) -> bool {
        if remainder_abs.is_zero() {
            return false;
        }
        match mode {
            RoundingMode::Down => sign < 0,
            RoundingMode::Up => sign > 0,
            RoundingMode::TowardZero => false,
            RoundingMode::AwayFromZero => true,
            RoundingMode::HalfUp | RoundingMode::HalfEven => {
                let doubled = remainder_abs.mul_small(2);
                match doubled.cmp_abs(divisor_abs) {
                    std::cmp::Ordering::Greater => true,
                    std::cmp::Ordering::Less => false,
                    std::cmp::Ordering::Equal => match mode {
                        RoundingMode::HalfUp => true,
                        RoundingMode::HalfEven => quotient_abs.div_rem_small(2).1 != 0,
                        _ => unreachable!(),
                    },
                }
            }
        }
    }

    pub(super) fn round_to_scale(&self, target_scale: u32, mode: RoundingMode) -> Self {
        if self.scale <= target_scale || self.coeff.is_zero() {
            return Self::normalize(self.coeff.clone(), self.scale);
        }
        let drop = self.scale - target_scale;
        let divisor = Self::pow10(drop);
        let (mut quotient, remainder) = self
            .abs_coeff()
            .div_mod(&divisor)
            .expect("non-zero power-of-ten divisor");
        if Self::should_round_increment(self.coeff.sign, &quotient, &remainder, &divisor, mode) {
            quotient = quotient.add_small(1);
        }
        Self::normalize(
            BigIntData::normalize(self.coeff.sign, quotient.limbs.to_vec()),
            target_scale,
        )
    }

    pub(super) fn div_rounded(
        &self,
        other: &Self,
        target_scale: u32,
        mode: RoundingMode,
    ) -> Option<Self> {
        if other.coeff.is_zero() {
            return None;
        }
        let sign = if self.coeff.sign == other.coeff.sign {
            1
        } else {
            -1
        };
        let numerator = self
            .abs_coeff()
            .mul_pow10(other.scale.checked_add(target_scale)?);
        let denominator = other.abs_coeff().mul_pow10(self.scale);
        let (mut quotient, remainder) = numerator.div_mod(&denominator)?;
        if Self::should_round_increment(sign, &quotient, &remainder, &denominator, mode) {
            quotient = quotient.add_small(1);
        }
        Some(Self::normalize(
            BigIntData::normalize(sign, quotient.limbs.to_vec()),
            target_scale,
        ))
    }

    pub(super) fn align_coeffs(&self, other: &Self) -> (BigIntData, BigIntData, u32) {
        let scale = self.scale.max(other.scale);
        let lhs = self.coeff.mul_pow10(scale - self.scale);
        let rhs = other.coeff.mul_pow10(scale - other.scale);
        (lhs, rhs, scale)
    }

    pub(super) fn add(&self, other: &Self) -> Self {
        let (lhs, rhs, scale) = self.align_coeffs(other);
        Self::normalize(lhs.add(&rhs), scale)
    }

    pub(super) fn sub(&self, other: &Self) -> Self {
        let (lhs, rhs, scale) = self.align_coeffs(other);
        Self::normalize(lhs.sub(&rhs), scale)
    }

    pub(super) fn mul(&self, other: &Self) -> Option<Self> {
        Some(Self::normalize(
            self.coeff.mul(&other.coeff),
            self.scale.checked_add(other.scale)?,
        ))
    }
}

impl PartialOrd for DecimalData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DecimalData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let (lhs, rhs, _) = self.align_coeffs(other);
        lhs.cmp(&rhs)
    }
}
