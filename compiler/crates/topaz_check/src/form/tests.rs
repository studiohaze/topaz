use super::nominals::has_invalid_nominal_instance_arg;
use super::*;
use crate::ty::{Prim, Type};

#[test]
fn nominal_instance_display_and_substitution_recurse_into_args() {
    let boxed = Type::NominalRecord {
        base: "Box".into(),
        args: vec![Type::Var(0)],
    };

    assert_eq!(boxed.to_string(), "Box<?0>");
    assert_eq!(nominal_instance_id("Box", &[Type::Var(0)]), "Box<?0>");

    let concrete = substitute(&boxed, &[Type::Prim(Prim::Int)]);
    assert_eq!(
        concrete,
        Type::NominalRecord {
            base: "Box".into(),
            args: vec![Type::Prim(Prim::Int)]
        }
    );
    assert_eq!(concrete.to_string(), "Box<int>");
}

#[test]
fn invalid_nominal_instance_arg_detection_recurse_into_args() {
    let boxed_var = Type::NominalRecord {
        base: "Box".into(),
        args: vec![Type::Var(0)],
    };
    let boxed_skolem = Type::NominalRecord {
        base: "Box".into(),
        args: vec![Type::Skolem {
            name: "T".into(),
            id: 1,
            origin: "test:T".into(),
        }],
    };
    let boxed_sentinel = Type::NominalRecord {
        base: "Box".into(),
        args: vec![Type::Var(u32::MAX)],
    };
    let boxed_unknown = Type::NominalRecord {
        base: "Box".into(),
        args: vec![Type::Unknown],
    };
    let boxed_concrete = Type::NominalRecord {
        base: "Box".into(),
        args: vec![Type::Prim(Prim::Int)],
    };

    assert!(!has_invalid_nominal_instance_arg(&boxed_var));
    assert!(!has_invalid_nominal_instance_arg(&boxed_skolem));
    assert!(has_invalid_nominal_instance_arg(&boxed_sentinel));
    assert!(has_invalid_nominal_instance_arg(&boxed_unknown));
    assert!(!has_invalid_nominal_instance_arg(&boxed_concrete));
}
