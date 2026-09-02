use super::capability::type_has_var;
use super::util::{
    contains_projection, contains_rigid, contains_true_unknown, contains_var_index,
    skolems_to_vars, strip_projections, substitute, unify_with, unknown_for_vars,
};
use crate::ty::{Prim, Type};

fn box_record(arg: Type) -> Type {
    Type::NominalRecord {
        base: "Box".into(),
        args: vec![arg],
    }
}

#[test]
fn nominal_arg_predicates_recurse() {
    let with_var = box_record(Type::Var(7));
    assert!(type_has_var(&with_var));
    assert!(contains_var_index(&with_var, 7));
    assert!(!contains_var_index(&with_var, 8));

    let with_unknown = box_record(Type::Unknown);
    assert!(contains_true_unknown(&with_unknown));

    let with_skolem = box_record(Type::Skolem {
        name: "T".into(),
        id: 99,
        origin: "test:T".into(),
    });
    assert!(contains_rigid(&with_skolem));
    assert!(contains_projection(&with_skolem, &[99]));
}

#[test]
fn nominal_arg_rewriters_preserve_nominal_structure() {
    let with_var = box_record(Type::Var(0));
    assert_eq!(unknown_for_vars(&with_var).to_string(), "Box<?>");
    assert_eq!(
        substitute(&with_var, &[Some(Type::Prim(Prim::Int))]).to_string(),
        "Box<int>"
    );

    let with_skolem = box_record(Type::Skolem {
        name: "T".into(),
        id: 11,
        origin: "test:T".into(),
    });
    assert_eq!(
        skolems_to_vars(&with_skolem, &[(11, 0)]).to_string(),
        "Box<?0>"
    );
    assert_eq!(strip_projections(&with_skolem, &[11]).to_string(), "Box<?>");
}

#[test]
fn nominal_unify_solves_args() {
    let mut subst = vec![None];
    unify_with(
        &box_record(Type::Var(0)),
        &box_record(Type::Prim(Prim::Int)),
        &mut subst,
        false,
    );
    assert_eq!(subst, vec![Some(Type::Prim(Prim::Int))]);
}
