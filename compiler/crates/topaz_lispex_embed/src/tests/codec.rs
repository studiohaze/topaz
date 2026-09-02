use super::*;
use crate::value_codec::MAX_VALUE_DEPTH;
use crate::*;

#[test]
fn canonical_lispex_value_is_lossless_closed_and_role_aware() {
    let mut rational = canonical_field(4, b"-2");
    rational.extend(1_u64.to_be_bytes());
    rational.extend(b"3");
    let mut real = vec![5];
    real.extend(1.5_f64.to_bits().to_be_bytes());
    let mut character = vec![6];
    character.extend(u32::from('한').to_be_bytes());
    let improper = {
        let mut value = vec![10];
        value.extend(1_u64.to_be_bytes());
        value.extend(canonical_field(3, b"7"));
        value.extend(canonical_field(8, b"tail"));
        value
    };
    let record = canonical_record(&[
        ("a", canonical_field(7, "λ".as_bytes())),
        ("b", canonical_sequence(9, &[vec![0], vec![2]])),
    ]);
    let positives = [
        vec![0],
        vec![1],
        vec![2],
        canonical_field(3, b"0"),
        canonical_field(3, b"-17"),
        rational,
        real,
        character,
        canonical_field(7, "사결".as_bytes()),
        canonical_field(8, "결과".as_bytes()),
        canonical_sequence(9, &[vec![0], vec![2]]),
        improper,
        canonical_sequence(11, &[vec![1], vec![2]]),
        canonical_field(12, &[0, 17, 255]),
        record.clone(),
    ];
    for bytes in positives {
        let value = LispexValue::from_canonical(bytes.clone()).expect("canonical value");
        assert_eq!(value.canonical_bytes(), bytes);
        assert_eq!(value.into_canonical_bytes(), bytes);
    }
    assert_eq!(
        LispexValue::from_guest_result(record)
            .expect_err("host record is input-only")
            .code(),
        "value-host-record-result"
    );

    let mut noncanonical_rational = canonical_field(4, b"2");
    noncanonical_rational.extend(1_u64.to_be_bytes());
    noncanonical_rational.extend(b"4");
    let mut infinite = vec![5];
    infinite.extend(f64::INFINITY.to_bits().to_be_bytes());
    let mut surrogate = vec![6];
    surrogate.extend(0xd800_u32.to_be_bytes());
    let unsorted = canonical_record(&[("b", vec![0]), ("a", vec![0])]);
    let duplicate = canonical_record(&[("a", vec![0]), ("a", vec![1])]);
    let negatives = [
        (vec![255], "value-tag"),
        (vec![0, 0], "value-trailing"),
        (canonical_field(3, b"01"), "value-integer"),
        (noncanonical_rational, "value-rational"),
        (infinite, "value-real"),
        (surrogate, "value-character"),
        (canonical_field(8, &[255]), "value-utf8"),
        (unsorted, "value-record-order"),
        (duplicate, "value-record-order"),
        (vec![8, 0, 0, 0], "value-truncated"),
    ];
    for (bytes, code) in negatives {
        assert_eq!(
            LispexValue::from_canonical(bytes)
                .expect_err("noncanonical value")
                .code(),
            code
        );
    }

    let mut too_deep = vec![0];
    for _ in 0..MAX_VALUE_DEPTH {
        too_deep = canonical_sequence(9, &[too_deep]);
    }
    assert_eq!(
        LispexValue::from_canonical(too_deep)
            .expect_err("depth limit")
            .code(),
        "value-depth"
    );

    let oversized = vec![0; MAX_CANONICAL_VALUE_BYTES + 1];
    assert_eq!(
        LispexValue::from_canonical(oversized)
            .expect_err("byte limit")
            .code(),
        "value-byte-limit"
    );
}
