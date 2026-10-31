use super::*;

pub(super) const BIGINT_BASE: u64 = 1_000_000_000;
pub(super) const BIGINT_BASE_U32: u32 = 1_000_000_000;

impl BigIntData {
    pub(super) fn zero() -> Self {
        Self {
            sign: 0,
            limbs: Rc::from([] as [u32; 0]),
        }
    }

    pub(super) fn normalize(sign: i8, mut limbs: Vec<u32>) -> Self {
        while limbs.last().copied() == Some(0) {
            limbs.pop();
        }
        if limbs.is_empty() || sign == 0 {
            Self::zero()
        } else {
            Self {
                sign: if sign < 0 { -1 } else { 1 },
                limbs: Rc::from(limbs.into_boxed_slice()),
            }
        }
    }

    pub(super) fn from_i64(n: i64) -> Self {
        if n == 0 {
            return Self::zero();
        }
        let sign = if n < 0 { -1 } else { 1 };
        let mut mag = if n < 0 {
            (-(n as i128)) as u128
        } else {
            n as u128
        };
        let mut limbs = Vec::new();
        while mag > 0 {
            limbs.push((mag % BIGINT_BASE as u128) as u32);
            mag /= BIGINT_BASE as u128;
        }
        Self::normalize(sign, limbs)
    }

    pub(super) fn is_zero(&self) -> bool {
        self.sign == 0
    }

    pub(super) fn cmp_abs(&self, other: &Self) -> std::cmp::Ordering {
        cmp_limbs(&self.limbs, &other.limbs)
    }

    pub(super) fn neg(&self) -> Self {
        Self::normalize(-self.sign, self.limbs.to_vec())
    }

    pub(super) fn add(&self, other: &Self) -> Self {
        match (self.sign, other.sign) {
            (0, _) => other.clone(),
            (_, 0) => self.clone(),
            (a, b) if a == b => Self::normalize(a, add_limbs(&self.limbs, &other.limbs)),
            _ => match self.cmp_abs(other) {
                std::cmp::Ordering::Greater => {
                    Self::normalize(self.sign, sub_limbs(&self.limbs, &other.limbs))
                }
                std::cmp::Ordering::Less => {
                    Self::normalize(other.sign, sub_limbs(&other.limbs, &self.limbs))
                }
                std::cmp::Ordering::Equal => Self::zero(),
            },
        }
    }

    pub(super) fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    pub(super) fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        Self::normalize(self.sign * other.sign, mul_limbs(&self.limbs, &other.limbs))
    }

    pub(super) fn div_mod(&self, other: &Self) -> Option<(Self, Self)> {
        if other.is_zero() {
            return None;
        }
        if self.is_zero() {
            return Some((Self::zero(), Self::zero()));
        }
        let (q_abs, r_abs) = div_mod_abs(self, other);
        let q = Self::normalize(self.sign * other.sign, q_abs.limbs.to_vec());
        let r = Self::normalize(self.sign, r_abs.limbs.to_vec());
        Some((q, r))
    }

    pub(super) fn parse_radix(text: &str, radix: i64) -> Option<Self> {
        if !(2..=36).contains(&radix) || text.is_empty() {
            return None;
        }
        let bytes = text.as_bytes();
        let (sign, digits) = match bytes[0] {
            b'-' => (-1, &bytes[1..]),
            b'+' => (1, &bytes[1..]),
            _ => (1, bytes),
        };
        if digits.is_empty() {
            return None;
        }
        let radix = radix as u32;
        let mut out = Self::zero();
        for &b in digits {
            let digit = ascii_digit_value(b)?;
            if digit >= radix {
                return None;
            }
            out = out.mul_small(radix).add_small(digit);
        }
        Some(Self::normalize(sign, out.limbs.to_vec()))
    }

    pub(super) fn to_string_radix(&self, radix: u32) -> String {
        if !(2..=36).contains(&radix) {
            return String::new();
        }
        if self.is_zero() {
            return "0".to_string();
        }
        let mut n = Self::normalize(1, self.limbs.to_vec());
        let mut chars = Vec::new();
        while !n.is_zero() {
            let (q, rem) = n.div_rem_small(radix);
            chars.push(digit_char(rem));
            n = q;
        }
        if self.sign < 0 {
            chars.push('-');
        }
        chars.iter().rev().collect()
    }

    pub(super) fn to_i64(&self) -> Option<i64> {
        let limit = if self.sign < 0 {
            (i64::MAX as i128) + 1
        } else {
            i64::MAX as i128
        };
        let mut acc = 0i128;
        for &limb in self.limbs.iter().rev() {
            acc = acc.checked_mul(BIGINT_BASE as i128)?;
            acc = acc.checked_add(limb as i128)?;
            if acc > limit {
                return None;
            }
        }
        if self.sign < 0 {
            if acc == limit {
                Some(i64::MIN)
            } else {
                Some(-(acc as i64))
            }
        } else {
            Some(acc as i64)
        }
    }

    pub(super) fn mul_small(&self, by: u32) -> Self {
        if self.is_zero() || by == 0 {
            return Self::zero();
        }
        Self::normalize(self.sign, mul_small_limbs(&self.limbs, by))
    }

    pub(super) fn mul_pow10(&self, exp: u32) -> Self {
        let mut out = self.clone();
        for _ in 0..exp {
            out = out.mul_small(10);
        }
        out
    }

    pub(super) fn add_small(&self, add: u32) -> Self {
        if add == 0 {
            return self.clone();
        }
        let mut out = self.limbs.to_vec();
        let mut carry = add as u64;
        let mut i = 0usize;
        while carry > 0 {
            if i == out.len() {
                out.push(0);
            }
            let total = out[i] as u64 + carry;
            out[i] = (total % BIGINT_BASE) as u32;
            carry = total / BIGINT_BASE;
            i += 1;
        }
        Self::normalize(1, out)
    }

    pub(super) fn div_rem_small(&self, by: u32) -> (Self, u32) {
        debug_assert!((2..=36).contains(&by));
        let mut q = vec![0u32; self.limbs.len()];
        let mut rem = 0u64;
        for (i, &limb) in self.limbs.iter().enumerate().rev() {
            let cur = rem * BIGINT_BASE + limb as u64;
            q[i] = (cur / by as u64) as u32;
            rem = cur % by as u64;
        }
        (Self::normalize(1, q), rem as u32)
    }
}

impl PartialOrd for BigIntData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigIntData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.sign.cmp(&other.sign) {
            std::cmp::Ordering::Equal => match self.sign {
                -1 => other.cmp_abs(self),
                0 => std::cmp::Ordering::Equal,
                _ => self.cmp_abs(other),
            },
            ord => ord,
        }
    }
}

pub(super) fn ascii_digit_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'z' => Some((b - b'a' + 10) as u32),
        b'A'..=b'Z' => Some((b - b'A' + 10) as u32),
        _ => None,
    }
}

pub(super) fn digit_char(digit: u32) -> char {
    match digit {
        0..=9 => (b'0' + digit as u8) as char,
        10..=35 => (b'a' + (digit as u8 - 10)) as char,
        _ => unreachable!("radix digit out of range"),
    }
}

pub(super) fn cmp_limbs(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => {
            for (x, y) in a.iter().rev().zip(b.iter().rev()) {
                match x.cmp(y) {
                    std::cmp::Ordering::Equal => {}
                    ord => return ord,
                }
            }
            std::cmp::Ordering::Equal
        }
        ord => ord,
    }
}

pub(super) fn add_limbs(a: &[u32], b: &[u32]) -> Vec<u32> {
    let len = a.len().max(b.len());
    let mut out = Vec::with_capacity(len + 1);
    let mut carry = 0u64;
    for i in 0..len {
        let total =
            a.get(i).copied().unwrap_or(0) as u64 + b.get(i).copied().unwrap_or(0) as u64 + carry;
        out.push((total % BIGINT_BASE) as u32);
        carry = total / BIGINT_BASE;
    }
    if carry > 0 {
        out.push(carry as u32);
    }
    out
}

pub(super) fn sub_limbs(a: &[u32], b: &[u32]) -> Vec<u32> {
    debug_assert!(cmp_limbs(a, b) != std::cmp::Ordering::Less);
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for (i, &x) in a.iter().enumerate() {
        let y = b.get(i).copied().unwrap_or(0) as i64;
        let mut cur = x as i64 - y - borrow;
        if cur < 0 {
            cur += BIGINT_BASE as i64;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(cur as u32);
    }
    out
}

pub(super) fn mul_limbs(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = vec![0u64; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u128;
        for (j, &y) in b.iter().enumerate() {
            let idx = i + j;
            let total = out[idx] as u128 + (x as u128 * y as u128) + carry;
            out[idx] = (total % BIGINT_BASE as u128) as u64;
            carry = total / BIGINT_BASE as u128;
        }
        let mut idx = i + b.len();
        while carry > 0 {
            if idx == out.len() {
                out.push(0);
            }
            let total = out[idx] as u128 + carry;
            out[idx] = (total % BIGINT_BASE as u128) as u64;
            carry = total / BIGINT_BASE as u128;
            idx += 1;
        }
    }
    out.into_iter().map(|d| d as u32).collect()
}

pub(super) fn mul_small_limbs(a: &[u32], by: u32) -> Vec<u32> {
    if by == 0 || a.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(a.len() + 1);
    let mut carry = 0u128;
    for &x in a {
        let total = x as u128 * by as u128 + carry;
        out.push((total % BIGINT_BASE as u128) as u32);
        carry = total / BIGINT_BASE as u128;
    }
    if carry > 0 {
        out.push(carry as u32);
    }
    out
}

pub(super) fn div_mod_abs(a: &BigIntData, b: &BigIntData) -> (BigIntData, BigIntData) {
    let b_abs = BigIntData::normalize(1, b.limbs.to_vec());
    match a.cmp_abs(&b_abs) {
        std::cmp::Ordering::Less => {
            return (
                BigIntData::zero(),
                BigIntData::normalize(1, a.limbs.to_vec()),
            );
        }
        std::cmp::Ordering::Equal => return (BigIntData::from_i64(1), BigIntData::zero()),
        std::cmp::Ordering::Greater => {}
    }

    let mut quotient_be = Vec::with_capacity(a.limbs.len());
    let mut rem = BigIntData::zero();
    for &digit in a.limbs.iter().rev() {
        let mut shifted = Vec::with_capacity(rem.limbs.len() + 1);
        shifted.push(digit);
        shifted.extend(rem.limbs.iter().copied());
        rem = BigIntData::normalize(1, shifted);

        let mut lo = 0u32;
        let mut hi = BIGINT_BASE_U32 - 1;
        let mut best = 0u32;
        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            let prod = BigIntData::normalize(1, mul_small_limbs(&b_abs.limbs, mid));
            if prod.cmp_abs(&rem) != std::cmp::Ordering::Greater {
                best = mid;
                if mid == BIGINT_BASE_U32 - 1 {
                    break;
                }
                lo = mid + 1;
            } else if mid == 0 {
                break;
            } else {
                hi = mid - 1;
            }
        }

        if best != 0 {
            let prod = BigIntData::normalize(1, mul_small_limbs(&b_abs.limbs, best));
            rem = BigIntData::normalize(1, sub_limbs(&rem.limbs, &prod.limbs));
        }
        quotient_be.push(best);
    }
    quotient_be.reverse();
    (BigIntData::normalize(1, quotient_be), rem)
}
