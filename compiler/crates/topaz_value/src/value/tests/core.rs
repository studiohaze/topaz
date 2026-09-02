use super::*;

#[test]
fn exact_runtime_args_preserve_order_and_return_arity_faults() {
    let [first, second] =
        exact_args::<2>(vec![Value::Int(11), Value::Int(22)], SP).expect("two arguments");
    assert_eq!(render(&first), "11");
    assert_eq!(render(&second), "22");

    let error = exact_args::<2>(vec![Value::Unit], SP).expect_err("arity mismatch");
    assert_eq!(error.code, codes::GUARD_ARITY);
    assert_eq!(error.message, "expected 2 argument(s), found 1");
}

#[test]
fn extern_policy_replay_store_preserves_legacy_unconfigured_behavior() {
    let store = ExternReplayStore::parse_jsonl(EXTERN_REPLAY_JSONL).expect("replay parses");
    let result = store
        .call("host.math", "twice", &[Value::Int(21)])
        .expect("legacy replay without a policy map still works");
    assert_eq!(render(&result), "42");
    assert!(store.sandbox_policy("host.math").is_none());
}

#[test]
fn extern_policy_map_is_visible_to_replay_calls() {
    let store = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.math",
            ExternSandboxKind::Wasm,
            Some("artifacts/host-math.wasm".to_string()),
            Some(1000),
            Some(65536),
        )],
    )
    .expect("policy-bound replay parses");
    let policy = store
        .sandbox_policy("host.math")
        .expect("policy is stored by module");
    assert_eq!(policy.kind, ExternSandboxKind::Wasm);
    assert_eq!(
        policy.artifact_path.as_deref(),
        Some("artifacts/host-math.wasm")
    );
    assert_eq!(policy.fuel, Some(1000));
    assert_eq!(policy.memory_bytes, Some(65536));
    let result = store
        .call("host.math", "twice", &[Value::Int(21)])
        .expect("wasm policy with an artifact still runs through replay");
    assert_eq!(render(&result), "42");
}

#[test]
fn extern_policy_wasm_kind_is_replay_sandbox_not_live_execution() {
    let store = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.math",
            ExternSandboxKind::Wasm,
            Some("artifacts/not-loaded-by-v54-runtime.wasm".to_string()),
            Some(3),
            Some(128),
        )],
    )
    .expect("policy-bound replay parses");

    let result = store
        .call_replay_sandbox("host.math", "twice", &[Value::Int(21)])
        .expect("v5.4 wasm policy is replay-sandboxed, not live-loaded");
    assert_eq!(render(&result), "42");
}

#[test]
fn extern_policy_fuel_limit_is_inclusive_and_deterministic() {
    let exact_store = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.math",
            ExternSandboxKind::Replay,
            None,
            Some(3),
            None,
        )],
    )
    .expect("exact fuel-bound replay parses");
    let result = exact_store
        .call("host.math", "twice", &[Value::Int(21)])
        .expect("used == budget is accepted");
    assert_eq!(render(&result), "42");

    let tight_store = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.math",
            ExternSandboxKind::Replay,
            None,
            Some(2),
            None,
        )],
    )
    .expect("tight fuel-bound replay parses");
    let err = tight_store
        .call("host.math", "twice", &[Value::Int(21)])
        .unwrap_err();
    assert!(
        err.contains("extern replay fuel limit exceeded for `host.math.twice`"),
        "{err}"
    );
    assert!(err.contains("used 3, budget 2"), "{err}");
}

#[test]
fn extern_policy_memory_limit_is_inclusive_and_deterministic() {
    let args = [Value::Int(21)];
    let result = Value::Int(42);
    let exact_budget = (canonical_abi_args_encode(&args).expect("args encode").len()
        + canonical_abi_encode(&result).expect("result encode").len())
        as u64;

    let exact_store = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.math",
            ExternSandboxKind::Replay,
            None,
            None,
            Some(exact_budget),
        )],
    )
    .expect("exact memory-bound replay parses");
    let result = exact_store
        .call("host.math", "twice", &args)
        .expect("used == budget is accepted");
    assert_eq!(render(&result), "42");

    let tight_store = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.math",
            ExternSandboxKind::Replay,
            None,
            None,
            Some(exact_budget - 1),
        )],
    )
    .expect("tight memory-bound replay parses");
    let err = tight_store.call("host.math", "twice", &args).unwrap_err();
    assert!(
        err.contains("extern replay memory_bytes limit exceeded for `host.math.twice`"),
        "{err}"
    );
    assert!(
        err.contains(&format!("used {exact_budget}, budget {}", exact_budget - 1)),
        "{err}"
    );
}

#[test]
fn extern_policy_fuel_counter_counts_nested_nodes_and_depth_limits_cycles() {
    let record = Value::Record(Rc::new(BTreeMap::from([(
        "flag".to_string(),
        Value::Bool(true),
    )])));
    let nested = Value::Array(Rc::new(RefCell::new(vec![
        Value::Some(Rc::new(Value::Int(1))),
        record,
    ])));
    assert_eq!(
        extern_replay_fuel_used(&[nested], &Value::Ok(Rc::new(Value::Int(2))))
            .expect("nested fuel counts"),
        8
    );

    let cycle_items = Rc::new(RefCell::new(Vec::new()));
    let cycle = Value::Array(cycle_items.clone());
    cycle_items.borrow_mut().push(cycle.clone());
    let err = abi_value_nodes(&cycle, 0).unwrap_err();
    assert!(
        err.contains(
            "ABI_LIMIT: extern replay resource envelope exceeds the ABI value depth limit"
        ),
        "{err}"
    );
}

#[test]
fn extern_policy_map_rejects_missing_runtime_policy() {
    let store = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.image",
            ExternSandboxKind::Replay,
            None,
            None,
            None,
        )],
    )
    .expect("policy-bound replay parses");
    let err = store
        .call("host.math", "twice", &[Value::Int(21)])
        .unwrap_err();
    assert!(
        err.contains("extern sandbox policy for `host.math` is not available"),
        "{err}"
    );
}

#[test]
fn extern_policy_map_rejects_malformed_wasm_policy() {
    let err = ExternReplayStore::parse_jsonl_with_policies(
        EXTERN_REPLAY_JSONL,
        vec![ExternSandboxPolicy::new(
            "host.math",
            ExternSandboxKind::Wasm,
            None,
            None,
            None,
        )],
    )
    .unwrap_err();
    assert!(
        err.contains("extern sandbox policy for `host.math` kind `wasm` requires an artifact"),
        "{err}"
    );
}

#[test]
fn primitive_equality_follows_spec() {
    assert_eq!(values_equal(&Value::Int(1), &Value::Int(1)), Ok(true));
    assert_eq!(
        values_equal(&Value::Float(f64::NAN), &Value::Float(f64::NAN)),
        Ok(false) // IEEE-754
    );
    assert_eq!(values_equal(&Value::Null, &Value::Null), Ok(true));
    // Union-member semantics: different comparable kinds are
    // unequal, never an error.
    assert_eq!(values_equal(&Value::Int(1), &Value::Null), Ok(false));
    assert_eq!(values_equal(&Value::Int(1), &Value::Float(1.0)), Ok(false));
}

#[test]
fn records_compare_fieldwise_by_name() {
    let a = Value::record([("x".into(), Value::Int(1)), ("y".into(), Value::Int(2))]);
    let b = Value::record([("y".into(), Value::Int(2)), ("x".into(), Value::Int(1))]);
    assert_eq!(values_equal(&a, &b), Ok(true));
    let c = Value::record([("x".into(), Value::Int(1))]);
    assert_eq!(values_equal(&a, &c), Err(CmpError::RecordShape));
}

#[test]
fn arrays_compare_by_length_and_order() {
    let a = Value::array(vec![Value::Int(1), Value::Int(2)]);
    let b = Value::array(vec![Value::Int(1), Value::Int(2)]);
    let c = Value::array(vec![Value::Int(2), Value::Int(1)]);
    assert_eq!(values_equal(&a, &b), Ok(true));
    assert_eq!(values_equal(&a, &c), Ok(false));
}

#[test]
fn checked_key_surface_enforces_comparability() {
    let mut map = OrderedMap::new();
    let m = Value::Map(Rc::new(RefCell::new(OrderedMap::new())));
    assert_eq!(
        map.insert_value(&m, Value::Int(1)).err(),
        Some(CmpError::NotComparable("Map"))
    );
    map.insert_value(&Value::str("k"), Value::Int(7))
        .expect("comparable");
    assert_eq!(
        map.get_value(&Value::str("k"))
            .expect("comparable")
            .map(|v| render(&v)),
        Some("7".to_string())
    );
    let mut set = OrderedSet::new();
    assert!(set.add_value(&Value::Int(1)).expect("comparable"));
    assert!(set.contains_value(&Value::Int(1)).expect("comparable"));
    assert!(set.remove_value(&Value::Int(1)).expect("comparable"));
}

#[test]
fn non_comparables_are_guards_not_false() {
    let m = Value::Map(Rc::new(RefCell::new(OrderedMap::new())));
    assert_eq!(
        values_equal(&m, &Value::Int(1)),
        Err(CmpError::NotComparable("Map"))
    );
}

#[test]
fn template_and_namespace_equality_is_false_not_a_guard() {
    // Behavior-neutrality (CDR-006 E-1c): the interpreter compared
    // templates and namespaces as Ok(false), never a guard error
    // — the checker rejects §2 comparison of these statically, so
    // a runtime comparison is unreachable in checked programs. The
    // shared comparator must match that exactly, not tighten it.
    #[derive(Debug)]
    struct T;
    impl TpzTemplate for T {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn render_into(&self, _: &mut String) {}
    }
    let t1 = Value::Template(Rc::new(T));
    let t2 = Value::Template(Rc::new(T));
    assert_eq!(values_equal(&t1, &t2), Ok(false));
    let ns = Value::Namespace(Rc::from("m"));
    assert_eq!(
        values_equal(&ns, &Value::Namespace(Rc::from("m"))),
        Ok(false)
    );
    // Against a comparable kind: still Ok(false), not a guard.
    assert_eq!(values_equal(&ns, &Value::Int(1)), Ok(false));
    assert_eq!(values_equal(&t1, &Value::Int(1)), Ok(false));
}

#[test]
fn option_result_equality() {
    let some1 = Value::Some(Rc::new(Value::Int(1)));
    let some2 = Value::Some(Rc::new(Value::Int(1)));
    assert_eq!(values_equal(&some1, &some2), Ok(true));
    assert_eq!(values_equal(&some1, &Value::None), Ok(false));
    let ok = Value::Ok(Rc::new(Value::Int(1)));
    let err = Value::Err(Rc::new(Value::Int(1)));
    assert_eq!(values_equal(&ok, &err), Ok(false));
}

#[test]
fn map_keys_are_snapshots() {
    let arr = Value::array(vec![Value::Int(1)]);
    let key = canonical_key(&arr).expect("comparable");
    let mut map = OrderedMap::new();
    map.insert(key.clone(), Value::str("hit"));
    // Mutate the source aggregate after insertion.
    if let Value::Array(items) = &arr {
        items.borrow_mut().push(Value::Int(2));
    }
    // The stored key is unaffected; the original key still hits.
    assert_eq!(map.get(&key).map(|v| render(&v)), Some("hit".to_string()));
    // The mutated aggregate freezes to a different key.
    let new_key = canonical_key(&arr).expect("comparable");
    assert!(map.get(&new_key).is_none());
}

#[test]
fn map_insert_keeps_insertion_order_and_updates_in_place() {
    let mut map = OrderedMap::new();
    map.insert(Key::Str(Rc::from("a")), Value::Int(1));
    map.insert(Key::Str(Rc::from("b")), Value::Int(2));
    map.insert(Key::Str(Rc::from("a")), Value::Int(3));
    let keys: Vec<String> = map.keys().iter().map(render).collect();
    assert_eq!(keys, vec!["a", "b"]);
    assert_eq!(
        map.get(&Key::Str(Rc::from("a")))
            .map(|v| render(&v))
            .as_deref(),
        Some("3")
    );
}

#[test]
fn set_add_remove_contract() {
    let mut set = OrderedSet::new();
    assert!(set.add(Key::Int(1)));
    assert!(!set.add(Key::Int(1)));
    assert!(set.remove(&Key::Int(1)));
    assert!(!set.remove(&Key::Int(1)));
}

#[test]
fn non_comparable_keys_rejected() {
    let m = Value::Map(Rc::new(RefCell::new(OrderedMap::new())));
    assert_eq!(
        canonical_key(&m).err(),
        Some(CmpError::NotComparable("Map"))
    );
}

#[test]
fn cyclic_values_exhaust_fuel_not_the_stack() {
    let arr = Rc::new(RefCell::new(vec![Value::Int(1)]));
    let cyclic = Value::Array(arr.clone());
    arr.borrow_mut().push(cyclic.clone());
    assert_eq!(canonical_key(&cyclic).err(), Some(CmpError::Fuel));
    let other = Value::array(vec![Value::Int(1)]);
    assert_eq!(values_equal(&cyclic, &other), Ok(false)); // length differs
    // No reflexive fast path: cyclic self-comparison fuels out.
    assert_eq!(values_equal(&cyclic, &cyclic).err(), Some(CmpError::Fuel));
    // Truncation is deterministic and bounded: exactly one
    // ellipsis, output stops there.
    let rendered = render(&cyclic);
    assert_eq!(rendered.matches("...").count(), 1);
    assert!(rendered.ends_with("..."));
    assert!(rendered.len() < 8 * STRUCT_DEPTH);
}

#[test]
fn rendering_is_stable() {
    assert_eq!(render(&Value::Float(1.0)), "1.0");
    assert_eq!(render(&Value::Float(1.5)), "1.5");
    assert_eq!(render(&Value::Unit), "()");
    let r = Value::record([("b".into(), Value::Int(2)), ("a".into(), Value::Int(1))]);
    assert_eq!(render(&r), "{ a: 1, b: 2 }");
    let some = Value::Some(Rc::new(Value::str("x")));
    assert_eq!(render(&some), "Some(x)");
}

#[test]
fn float_render_goldens_are_pinned_literals() {
    for golden in FLOAT_RENDER_GOLDENS {
        let value = f64::from_bits(golden.bits);
        assert_eq!(
            render_float(value),
            golden.render,
            "{} ({:016x})",
            golden.name,
            golden.bits
        );
    }
}

#[test]
fn float_render_golden_bits_roundtrip_without_canonicalizing_nan_payloads() {
    for golden in FLOAT_RENDER_GOLDENS {
        let value = f64::from_bits(golden.bits);
        assert_eq!(
            value.to_bits(),
            golden.bits,
            "{} ({:016x})",
            golden.name,
            golden.bits
        );
    }
}
