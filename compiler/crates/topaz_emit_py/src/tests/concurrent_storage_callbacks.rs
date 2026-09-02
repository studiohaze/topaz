use super::*;

#[test]
fn routes_unproven_storage_backed_function_value_callbacks_through_runtime_driver() {
    let mutable_callbacks_after_dynamic_assignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/001_mutable_callbacks_after_dynamic_assignment.tpz"
    ));
    assert!(
        mutable_callbacks_after_dynamic_assignment.contains("yield from tpz_array_map__co(")
            && !mutable_callbacks_after_dynamic_assignment
                .contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "mutable callback arrays touched through dynamic indices should use the runtime driver without stale static metadata: {mutable_callbacks_after_dynamic_assignment}"
    );
    assert_generated_python_gates(&mutable_callbacks_after_dynamic_assignment).unwrap_or_else(
        |e| panic!("dynamic-assigned mutable callback array Python gate failed: {e}"),
    );

    let mutable_callbacks_after_rhs_mutator = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/002_mutable_callbacks_after_rhs_mutator.tpz"
    ));
    assert!(
        mutable_callbacks_after_rhs_mutator.contains("yield from tpz_array_map__co(")
            && !mutable_callbacks_after_rhs_mutator.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "assignment RHS array mutators should invalidate static callback metadata while keeping the runtime driver: {mutable_callbacks_after_rhs_mutator}"
    );
    assert_generated_python_gates(&mutable_callbacks_after_rhs_mutator).unwrap_or_else(|e| {
        panic!("assignment-RHS mutator callback array Python gate failed: {e}")
    });

    let direct_co_callback_recovery_count =
        |generated: &str| generated.matches("__co(host, __tpz_cb_").count();
    let assert_optional_yield_from_hoisted = |generated: &str, label: &str| {
        let yield_pos = generated
            .find(" = yield from tpz_option_ok_or_else__co(")
            .unwrap_or_else(|| {
                panic!("missing hoisted cooperative okOrElse in {label}: {generated}")
            });
        let wrap_pos = generated
            .find(" = tpz_wrap_optional(")
            .unwrap_or_else(|| panic!("missing optional wrapper in {label}: {generated}"));
        assert!(
            yield_pos < wrap_pos,
            "optional receiver {label} should hoist yield-from before wrapping: {generated}"
        );
        assert!(
            !generated.contains("tpz_wrap_optional((yield from "),
            "optional receiver {label} must not embed yield-from inside tpz_wrap_optional: {generated}"
        );
    };

    let dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/003_dynamic_index.tpz"
    ));
    assert!(
        dynamic_index.contains("yield from tpz_array_map__co(")
            && !dynamic_index.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "dynamic callback array indices should use the runtime driver without static callback recovery: {dynamic_index}"
    );
    assert_generated_python_gates(&dynamic_index)
        .unwrap_or_else(|e| panic!("dynamic callback array index Python gate failed: {e}"));

    let same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/004_same_target_dynamic_index.tpz"
    ));
    assert!(
        same_target_dynamic_index.contains("yield from tpz_array_map__co(")
            && same_target_dynamic_index.contains("tpz_index(")
            && !same_target_dynamic_index.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "same-target dynamic callback array indices should preserve the runtime read and driver without static callback recovery: {same_target_dynamic_index}"
    );
    assert_generated_python_gates(&same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic callback array index Python gate failed: {e}")
    });

    let array_filter_same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/005_array_filter_same_target_dynamic_index.tpz"
    ));
    assert!(
        array_filter_same_target_dynamic_index.contains("yield from tpz_array_filter__co(")
            && array_filter_same_target_dynamic_index.contains("tpz_index(")
            && direct_co_callback_recovery_count(&array_filter_same_target_dynamic_index) == 0,
        "same-target dynamic Array.filter callback indices should preserve the runtime read and driver without static callback recovery: {array_filter_same_target_dynamic_index}"
    );
    assert_generated_python_gates(&array_filter_same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic Array.filter callback index Python gate failed: {e}")
    });

    let array_reduce_same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/006_array_reduce_same_target_dynamic_index.tpz"
    ));
    assert!(
        array_reduce_same_target_dynamic_index.contains("yield from tpz_array_reduce__co(")
            && array_reduce_same_target_dynamic_index.contains("tpz_index(")
            && direct_co_callback_recovery_count(&array_reduce_same_target_dynamic_index) == 0,
        "same-target dynamic Array.reduce callback indices should preserve the runtime read and driver without static callback recovery: {array_reduce_same_target_dynamic_index}"
    );
    assert_generated_python_gates(&array_reduce_same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic Array.reduce callback index Python gate failed: {e}")
    });

    let array_sorted_by_same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/007_array_sorted_by_same_target_dynamic_index.tpz"
    ));
    assert!(
        array_sorted_by_same_target_dynamic_index.contains("yield from tpz_array_sorted_by__co(")
            && array_sorted_by_same_target_dynamic_index.contains("tpz_index(")
            && direct_co_callback_recovery_count(&array_sorted_by_same_target_dynamic_index) == 0,
        "same-target dynamic Array.sortedBy callback indices should preserve the runtime read and driver without static callback recovery: {array_sorted_by_same_target_dynamic_index}"
    );
    assert_generated_python_gates(&array_sorted_by_same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic Array.sortedBy callback index Python gate failed: {e}")
    });

    let array_sort_by_same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/008_array_sort_by_same_target_dynamic_index.tpz"
    ));
    assert!(
        array_sort_by_same_target_dynamic_index.contains("yield from tpz_array_sort_by__co(")
            && array_sort_by_same_target_dynamic_index.contains("tpz_index(")
            && direct_co_callback_recovery_count(&array_sort_by_same_target_dynamic_index) == 0,
        "same-target dynamic Array.sortBy callback indices should preserve the runtime read and driver without static callback recovery: {array_sort_by_same_target_dynamic_index}"
    );
    assert_generated_python_gates(&array_sort_by_same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic Array.sortBy callback index Python gate failed: {e}")
    });

    let array_retain_same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/009_array_retain_same_target_dynamic_index.tpz"
    ));
    assert!(
        array_retain_same_target_dynamic_index.contains("yield from tpz_array_retain__co(")
            && array_retain_same_target_dynamic_index.contains("tpz_index(")
            && direct_co_callback_recovery_count(&array_retain_same_target_dynamic_index) == 0,
        "same-target dynamic Array.retain callback indices should preserve the runtime read and driver without static callback recovery: {array_retain_same_target_dynamic_index}"
    );
    assert_generated_python_gates(&array_retain_same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic Array.retain callback index Python gate failed: {e}")
    });

    let assert_same_arm_hof_dynamic_write_gate = |generated: &str, helper: &str, label: &str| {
        assert!(
            generated.contains(helper)
                && generated.contains("tpz_index_slot(")
                && generated.contains("tpz_index_slot_set(")
                && generated.contains("tpz_index(")
                && direct_co_callback_recovery_count(generated) == 0,
            "same-arm {label} dynamic-index writes should preserve the runtime write/read and driver without static callback recovery: {generated}"
        );
        assert_generated_python_gates(generated).unwrap_or_else(|e| {
            panic!("same-arm {label} dynamic-index write Python gate failed: {e}")
        });
    };
    let assert_absent_key_skip_wrapper_gate = |generated: &str, label: &str| {
        assert!(
            generated.contains("yield from tpz_map_update__co(")
                && generated.contains("tpz_index_slot(")
                && generated.contains("tpz_index_slot_set(")
                && generated.contains("tpz_index(")
                && direct_co_callback_recovery_count(generated) == 0
                && !generated.contains("_t_7370696e__co(host, __tpz_cb_0)"),
            "absent-key Map.update {label} callback skip should preserve runtime write/read, co-lowered update, and no direct callback recovery: {generated}"
        );
        assert_generated_python_gates(generated).unwrap_or_else(|e| {
            panic!("absent-key Map.update {label} callback skip Python gate failed: {e}")
        });
    };
    let assert_dynamic_spread_array_gate =
        |generated: &str, helper: &str, static_recovery_mangles: &[&str], label: &str| {
            let static_recoveries_absent = static_recovery_mangles
                .iter()
                .all(|mangle| !generated.contains(&format!("_t_{mangle}__co(host, __tpz_cb_0)")));
            assert!(
                generated.contains(helper)
                    && generated.contains("tpz_index(")
                    && direct_co_callback_recovery_count(generated) == 0
                    && static_recoveries_absent,
                "dynamic-index spread-built {label} should preserve runtime index reads and use the runtime driver without static callback recovery: {generated}"
            );
            assert_generated_python_gates(generated).unwrap_or_else(|e| {
                panic!("dynamic-index spread-built {label} Python gate failed: {e}")
            });
        };
    let assert_dynamic_spread_array_map_gate = |generated: &str, label: &str| {
        assert_dynamic_spread_array_gate(
            generated,
            "yield from tpz_array_map__co(",
            &["7a65726f", "696e63", "64626c", "7370696e"],
            &format!("Array.map {label}"),
        );
    };
    let assert_mutable_spread_array_hof_recovery =
        |generated: &str, helper: &str, expected_recovery_mangle: &str, label: &str| {
            assert!(
                generated.contains(helper)
                    && generated.contains(&format!(
                        "_t_{expected_recovery_mangle}__co(host, __tpz_cb_0"
                    )),
                "tracked mutable spread-source {label} should recover the selected cooperative callback directly: {generated}"
            );
            assert_generated_python_gates(generated).unwrap_or_else(|e| {
                panic!("tracked mutable spread-source {label} Python gate failed: {e}")
            });
        };
    let assert_cross_arm_dynamic_hof_gate =
        |generated: &str, helper: &str, static_recovery_mangles: &[&str], label: &str| {
            let static_recoveries_absent = static_recovery_mangles
                .iter()
                .all(|mangle| !generated.contains(&format!("_t_{mangle}__co(host, __tpz_cb_0)")));
            assert!(
                generated.contains(helper)
                    && generated.contains("tpz_index_slot(")
                    && generated.contains("tpz_index_slot_set(")
                    && generated.contains("tpz_index(")
                    && direct_co_callback_recovery_count(generated) == 0
                    && static_recoveries_absent,
                "cross-arm dynamic-index {label} should preserve runtime writes/reads and the cooperative driver without static callback recovery: {generated}"
            );
            assert_generated_python_gates(generated).unwrap_or_else(|e| {
                panic!("cross-arm dynamic-index {label} Python gate failed: {e}")
            });
        };
    let assert_exactly_one_helper = |generated: &str, helper: &str, label: &str| {
        let count = generated.matches(helper).count();
        assert_eq!(
            count, 1,
            "{label} should emit exactly one {helper} helper call, found {count}: {generated}"
        );
    };

    let array_map_cross_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/010_array_map_cross_arm_dynamic_index_write.tpz"
    ));
    assert_cross_arm_dynamic_hof_gate(
        &array_map_cross_arm_dynamic_index_write,
        "yield from tpz_array_map__co(",
        &["7a65726f", "696e63", "64626c"],
        "Array.map callback carrier",
    );
    assert_generated_python_ok_int(
        &array_map_cross_arm_dynamic_index_write,
        0,
        "cross-arm dynamic-index Array.map callback carrier",
    );

    let map_values_cross_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/011_map_values_cross_arm_dynamic_index_write.tpz"
    ));
    assert_cross_arm_dynamic_hof_gate(
        &map_values_cross_arm_dynamic_index_write,
        "yield from tpz_map_map_values__co(",
        &["7a65726f", "696e63", "64626c"],
        "Map.mapValues callback carrier",
    );
    assert_generated_python_ok_int(
        &map_values_cross_arm_dynamic_index_write,
        0,
        "cross-arm dynamic-index Map.mapValues callback carrier",
    );

    let array_map_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/012_array_map_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_map_same_arm_dynamic_index_write,
        "yield from tpz_array_map__co(",
        "Array.map",
    );

    let array_map_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/013_array_map_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_map_match_same_arm_dynamic_index_write,
        "yield from tpz_array_map__co(",
        "Array.map match",
    );

    let array_map_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/014_array_map_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_map_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_map__co(",
        "Array.map loop",
    );

    let array_map_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/015_array_map_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_map_while_same_arm_dynamic_index_write,
        "yield from tpz_array_map__co(",
        "Array.map while",
    );

    let array_filter_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/016_array_filter_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_filter_same_arm_dynamic_index_write,
        "yield from tpz_array_filter__co(",
        "Array.filter",
    );

    let array_filter_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/017_array_filter_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_filter_match_same_arm_dynamic_index_write,
        "yield from tpz_array_filter__co(",
        "Array.filter match",
    );

    let array_filter_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/018_array_filter_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_filter_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_filter__co(",
        "Array.filter loop",
    );

    let array_filter_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/019_array_filter_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_filter_while_same_arm_dynamic_index_write,
        "yield from tpz_array_filter__co(",
        "Array.filter while",
    );

    let array_filter_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/020_array_filter_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_filter_named_match_same_arm_dynamic_index_write,
        "yield from tpz_array_filter__co(",
        "Array.filter named match",
    );

    let array_filter_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/021_array_filter_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_filter_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_filter__co(",
        "Array.filter named loop",
    );

    let array_filter_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/022_array_filter_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_filter_named_while_same_arm_dynamic_index_write,
        "yield from tpz_array_filter__co(",
        "Array.filter named while",
    );

    let array_reduce_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/023_array_reduce_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_reduce_same_arm_dynamic_index_write,
        "yield from tpz_array_reduce__co(",
        "Array.reduce",
    );

    let array_reduce_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/024_array_reduce_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_reduce_match_same_arm_dynamic_index_write,
        "yield from tpz_array_reduce__co(",
        "Array.reduce match",
    );

    let array_reduce_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/025_array_reduce_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_reduce_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_reduce__co(",
        "Array.reduce loop",
    );

    let array_reduce_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/026_array_reduce_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_reduce_while_same_arm_dynamic_index_write,
        "yield from tpz_array_reduce__co(",
        "Array.reduce while",
    );

    let array_reduce_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/027_array_reduce_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_reduce_named_match_same_arm_dynamic_index_write,
        "yield from tpz_array_reduce__co(",
        "Array.reduce named match",
    );

    let array_reduce_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/028_array_reduce_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_reduce_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_reduce__co(",
        "Array.reduce named loop",
    );

    let array_reduce_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/029_array_reduce_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_reduce_named_while_same_arm_dynamic_index_write,
        "yield from tpz_array_reduce__co(",
        "Array.reduce named while",
    );

    let array_sorted_by_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/030_array_sorted_by_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sorted_by_same_arm_dynamic_index_write,
        "yield from tpz_array_sorted_by__co(",
        "Array.sortedBy",
    );

    let array_sorted_by_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/031_array_sorted_by_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sorted_by_match_same_arm_dynamic_index_write,
        "yield from tpz_array_sorted_by__co(",
        "Array.sortedBy match",
    );

    let array_sorted_by_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/032_array_sorted_by_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sorted_by_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_sorted_by__co(",
        "Array.sortedBy loop",
    );

    let array_sorted_by_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/033_array_sorted_by_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sorted_by_while_same_arm_dynamic_index_write,
        "yield from tpz_array_sorted_by__co(",
        "Array.sortedBy while",
    );

    let array_sorted_by_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/034_array_sorted_by_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sorted_by_named_match_same_arm_dynamic_index_write,
        "yield from tpz_array_sorted_by__co(",
        "Array.sortedBy named match",
    );

    let array_sorted_by_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/035_array_sorted_by_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sorted_by_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_sorted_by__co(",
        "Array.sortedBy named loop",
    );

    let array_sorted_by_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/036_array_sorted_by_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sorted_by_named_while_same_arm_dynamic_index_write,
        "yield from tpz_array_sorted_by__co(",
        "Array.sortedBy named while",
    );

    let array_sort_by_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/037_array_sort_by_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sort_by_same_arm_dynamic_index_write,
        "yield from tpz_array_sort_by__co(",
        "Array.sortBy",
    );

    let array_sort_by_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/038_array_sort_by_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sort_by_match_same_arm_dynamic_index_write,
        "yield from tpz_array_sort_by__co(",
        "Array.sortBy match",
    );

    let array_sort_by_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/039_array_sort_by_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sort_by_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_sort_by__co(",
        "Array.sortBy loop",
    );

    let array_sort_by_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/040_array_sort_by_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sort_by_while_same_arm_dynamic_index_write,
        "yield from tpz_array_sort_by__co(",
        "Array.sortBy while",
    );

    let array_sort_by_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/041_array_sort_by_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sort_by_named_match_same_arm_dynamic_index_write,
        "yield from tpz_array_sort_by__co(",
        "Array.sortBy named match",
    );

    let array_sort_by_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/042_array_sort_by_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sort_by_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_sort_by__co(",
        "Array.sortBy named loop",
    );

    let array_sort_by_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/043_array_sort_by_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_sort_by_named_while_same_arm_dynamic_index_write,
        "yield from tpz_array_sort_by__co(",
        "Array.sortBy named while",
    );

    let array_retain_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/044_array_retain_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_retain_same_arm_dynamic_index_write,
        "yield from tpz_array_retain__co(",
        "Array.retain",
    );

    let array_retain_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/045_array_retain_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_retain_match_same_arm_dynamic_index_write,
        "yield from tpz_array_retain__co(",
        "Array.retain match",
    );

    let array_retain_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/046_array_retain_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_retain_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_retain__co(",
        "Array.retain loop",
    );

    let array_retain_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/047_array_retain_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_retain_while_same_arm_dynamic_index_write,
        "yield from tpz_array_retain__co(",
        "Array.retain while",
    );

    let array_retain_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/048_array_retain_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_retain_named_match_same_arm_dynamic_index_write,
        "yield from tpz_array_retain__co(",
        "Array.retain named match",
    );

    let array_retain_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/049_array_retain_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_retain_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_array_retain__co(",
        "Array.retain named loop",
    );

    let array_retain_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/050_array_retain_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &array_retain_named_while_same_arm_dynamic_index_write,
        "yield from tpz_array_retain__co(",
        "Array.retain named while",
    );

    let option_map_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/051_option_map_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_map_same_arm_dynamic_index_write,
        "yield from tpz_option_map__co(",
        "Option.map",
    );

    let option_flat_map_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/052_option_flat_map_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_same_arm_dynamic_index_write,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap",
    );

    let option_ok_or_else_none_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/053_option_ok_or_else_none_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse None",
    );

    let option_ok_or_else_named_none_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/054_option_ok_or_else_named_none_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_named_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named None",
    );
    assert_exactly_one_helper(
        &option_ok_or_else_named_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named None",
    );

    let option_ok_or_else_record_field_receiver_same_arm_dynamic_index_write = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/055_option_ok_or_else_record_field_receiver_same_arm_dynamic_index_write.tpz"
        ),
    );
    assert!(
        option_ok_or_else_record_field_receiver_same_arm_dynamic_index_write
            .contains("tpz_member("),
        "record-field Option receiver should be read through tpz_member: {option_ok_or_else_record_field_receiver_same_arm_dynamic_index_write}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_record_field_receiver_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse record-field receiver",
    );

    let option_ok_or_else_record_field_receiver_some_same_arm_dynamic_index_write_non_call =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/056_option_ok_or_else_record_field_receiver_some_same_arm_dynamic_index_writ.tpz"
        ));
    assert!(
        option_ok_or_else_record_field_receiver_some_same_arm_dynamic_index_write_non_call
            .contains("tpz_member("),
        "record-field Option receiver Some non-call should be read through tpz_member: {option_ok_or_else_record_field_receiver_some_same_arm_dynamic_index_write_non_call}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_record_field_receiver_some_same_arm_dynamic_index_write_non_call,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse record-field receiver Some non-call",
    );
    assert_generated_python_ok_int(
        &option_ok_or_else_record_field_receiver_some_same_arm_dynamic_index_write_non_call,
        9,
        "Option.okOrElse record-field receiver Some non-call parity",
    );

    let option_ok_or_else_named_record_field_receiver_none_same_arm_dynamic_index_write =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/057_option_ok_or_else_named_record_field_receiver_none_same_arm_dynamic_inde.tpz"
        ));
    assert!(
        option_ok_or_else_named_record_field_receiver_none_same_arm_dynamic_index_write
            .contains("tpz_member("),
        "named record-field Option receiver None should be read through tpz_member: {option_ok_or_else_named_record_field_receiver_none_same_arm_dynamic_index_write}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_named_record_field_receiver_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named record-field receiver None",
    );
    assert_generated_python_ok_int(
        &option_ok_or_else_named_record_field_receiver_none_same_arm_dynamic_index_write,
        6,
        "Option.okOrElse named record-field receiver None parity",
    );

    let option_ok_or_else_named_record_field_receiver_some_same_arm_dynamic_index_write_non_call =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/058_option_ok_or_else_named_record_field_receiver_some_same_arm_dynamic_inde.tpz"
        ));
    assert!(
        option_ok_or_else_named_record_field_receiver_some_same_arm_dynamic_index_write_non_call
            .contains("tpz_member("),
        "named record-field Option receiver Some non-call should be read through tpz_member: {option_ok_or_else_named_record_field_receiver_some_same_arm_dynamic_index_write_non_call}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_named_record_field_receiver_some_same_arm_dynamic_index_write_non_call,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named record-field receiver Some non-call",
    );
    assert_generated_python_ok_int(
        &option_ok_or_else_named_record_field_receiver_some_same_arm_dynamic_index_write_non_call,
        9,
        "Option.okOrElse named record-field receiver Some non-call parity",
    );

    let option_ok_or_else_named_record_field_receiver_eval_order = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/059_option_ok_or_else_named_record_field_receiver_eval_order.tpz"
    ));
    assert!(
        option_ok_or_else_named_record_field_receiver_eval_order.contains("tpz_member("),
        "named record-field Option receiver eval-order should be read through tpz_member: {option_ok_or_else_named_record_field_receiver_eval_order}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_named_record_field_receiver_eval_order,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named record-field receiver evaluation order",
    );
    assert!(
        option_ok_or_else_named_record_field_receiver_eval_order
            .contains("_t_6d616b65486f6c646572__co"),
        "named record-field receiver eval-order gate should route the function-produced holder through its cooperative body: {option_ok_or_else_named_record_field_receiver_eval_order}"
    );
    assert!(
        option_ok_or_else_named_record_field_receiver_eval_order.contains("_t_7469636b__co(2"),
        "named record-field receiver eval-order gate should keep the named callback index observable: {option_ok_or_else_named_record_field_receiver_eval_order}"
    );
    assert_generated_python_ok_int(
        &option_ok_or_else_named_record_field_receiver_eval_order,
        1206,
        "Option.okOrElse named record-field receiver evaluation order parity",
    );

    let option_map_array_index_receiver_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/060_option_map_array_index_receiver_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_map_array_index_receiver_same_arm_dynamic_index_write,
        "yield from tpz_option_map__co(",
        "Option.map array-index receiver",
    );

    let option_ok_or_array_index_receiver = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/061_option_ok_or_array_index_receiver.tpz"
    ));
    assert!(
        option_ok_or_array_index_receiver.contains("tpz_option_ok_or(")
            && option_ok_or_array_index_receiver.contains("tpz_index(")
            && !option_ok_or_array_index_receiver.contains("tpz_option_ok_or__co("),
        "Option.okOr array-index receivers should use tpz_index plus the ordinary Option helper: {option_ok_or_array_index_receiver}"
    );
    assert_generated_python_gates(&option_ok_or_array_index_receiver)
        .unwrap_or_else(|e| panic!("Option.okOr array-index receiver Python gate failed: {e}"));

    let optional_option_ok_or_array_index_receiver = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/062_optional_option_ok_or_array_index_receiver.tpz"
    ));
    assert!(
        optional_option_ok_or_array_index_receiver.contains("tpz_option_ok_or(")
            && optional_option_ok_or_array_index_receiver.contains("tpz_wrap_optional(")
            && !optional_option_ok_or_array_index_receiver.contains(" = yield from "),
        "non-cooperative optional Option.okOr array-index receivers should wrap string payloads without cooperative yield-from: {optional_option_ok_or_array_index_receiver}"
    );
    assert_generated_python_gates(&optional_option_ok_or_array_index_receiver).unwrap_or_else(
        |e| panic!("optional Option.okOr array-index receiver Python gate failed: {e}"),
    );

    let option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/063_option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse array-index receiver None",
    );

    let option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write_non_call =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/064_option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write.tpz"
        ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write_non_call,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse array-index receiver Some non-call",
    );

    let option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index_write =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/065_option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index.tpz"
        ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named array-index receiver None",
    );

    let option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index_write_non_call =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/066_option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index.tpz"
        ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index_write_non_call,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named array-index receiver Some non-call",
    );

    let option_ok_or_else_named_array_index_receiver_eval_order = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/067_option_ok_or_else_named_array_index_receiver_eval_order.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_named_array_index_receiver_eval_order,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse named array-index receiver evaluation order",
    );
    let receiver_tick = option_ok_or_else_named_array_index_receiver_eval_order
        .find("_t_7469636b__co(1")
        .unwrap_or_else(|| {
            panic!(
                "missing receiver-index tick in named array-index receiver gate: {option_ok_or_else_named_array_index_receiver_eval_order}"
            )
        });
    let callback_tick = option_ok_or_else_named_array_index_receiver_eval_order
        .find("_t_7469636b__co(2")
        .unwrap_or_else(|| {
            panic!(
                "missing named callback-index tick in named array-index receiver gate: {option_ok_or_else_named_array_index_receiver_eval_order}"
            )
        });
    assert!(
        receiver_tick < callback_tick,
        "named array-index receiver calls should evaluate receiver index before named callback index: {option_ok_or_else_named_array_index_receiver_eval_order}"
    );
    assert_generated_python_ok_int(
        &option_ok_or_else_named_array_index_receiver_eval_order,
        1206,
        "Option.okOrElse named array-index receiver evaluation order parity",
    );

    let optional_option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/068_optional_option_ok_or_else_array_index_receiver_none_same_arm_dynamic_in.tpz"
        ));
    assert!(
        optional_option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write
            .contains("tpz_wrap_optional("),
        "optional array-index receiver should wrap the inner Option.okOrElse result: {optional_option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &optional_option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "optional Option.okOrElse array-index receiver None",
    );
    assert_optional_yield_from_hoisted(
        &optional_option_ok_or_else_array_index_receiver_none_same_arm_dynamic_index_write,
        "Option.okOrElse array-index receiver None",
    );

    let optional_option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write_non_call =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/069_optional_option_ok_or_else_array_index_receiver_some_same_arm_dynamic_in.tpz"
        ));
    assert!(
        optional_option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write_non_call
            .contains("tpz_wrap_optional("),
        "optional array-index receiver should wrap the inner Some-path result: {optional_option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write_non_call}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &optional_option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write_non_call,
        "yield from tpz_option_ok_or_else__co(",
        "optional Option.okOrElse array-index receiver Some non-call",
    );
    assert_optional_yield_from_hoisted(
        &optional_option_ok_or_else_array_index_receiver_some_same_arm_dynamic_index_write_non_call,
        "Option.okOrElse array-index receiver Some non-call",
    );

    let optional_option_ok_or_else_array_index_receiver_outer_none_skip_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/070_optional_option_ok_or_else_array_index_receiver_outer_none_skip_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &optional_option_ok_or_else_array_index_receiver_outer_none_skip_order,
        "yield from tpz_option_ok_or_else__co(",
        "optional Option.okOrElse array-index receiver outer None order",
    );
    assert_optional_yield_from_hoisted(
        &optional_option_ok_or_else_array_index_receiver_outer_none_skip_order,
        "Option.okOrElse array-index receiver outer None order",
    );
    assert_generated_python_ok_int(
        &optional_option_ok_or_else_array_index_receiver_outer_none_skip_order,
        107,
        "optional Option.okOrElse array-index receiver outer None skip/order parity",
    );

    let optional_option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index_write =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/071_optional_option_ok_or_else_named_array_index_receiver_none_same_arm_dyna.tpz"
        ));
    assert!(
        optional_option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index_write
            .contains("tpz_wrap_optional("),
        "named optional array-index receiver should wrap the inner Option.okOrElse result: {optional_option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index_write}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &optional_option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "named optional Option.okOrElse array-index receiver None",
    );
    assert_optional_yield_from_hoisted(
        &optional_option_ok_or_else_named_array_index_receiver_none_same_arm_dynamic_index_write,
        "named Option.okOrElse array-index receiver None",
    );

    let optional_option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index_write_non_call =
        emit_source(include_str!(
            "fixtures/concurrent_storage_callbacks/072_optional_option_ok_or_else_named_array_index_receiver_some_same_arm_dyna.tpz"
        ));
    assert!(
        optional_option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index_write_non_call
            .contains("tpz_wrap_optional("),
        "named optional array-index receiver should wrap the inner Some-path result: {optional_option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index_write_non_call}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &optional_option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index_write_non_call,
        "yield from tpz_option_ok_or_else__co(",
        "named optional Option.okOrElse array-index receiver Some non-call",
    );
    assert_optional_yield_from_hoisted(
        &optional_option_ok_or_else_named_array_index_receiver_some_same_arm_dynamic_index_write_non_call,
        "named Option.okOrElse array-index receiver Some non-call",
    );

    let optional_option_ok_or_else_named_array_index_receiver_outer_none_skip_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/073_optional_option_ok_or_else_named_array_index_receiver_outer_none_skip_or.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &optional_option_ok_or_else_named_array_index_receiver_outer_none_skip_order,
        "yield from tpz_option_ok_or_else__co(",
        "named optional Option.okOrElse array-index receiver outer None order",
    );
    assert_optional_yield_from_hoisted(
        &optional_option_ok_or_else_named_array_index_receiver_outer_none_skip_order,
        "named Option.okOrElse array-index receiver outer None order",
    );
    assert_generated_python_ok_int(
        &optional_option_ok_or_else_named_array_index_receiver_outer_none_skip_order,
        107,
        "named optional Option.okOrElse array-index receiver outer None skip/order parity",
    );

    let option_innerless_array_index_receiver_decline = emit_error_for_source(include_str!(
        "fixtures/concurrent_storage_callbacks/074_case_74.tpz"
    ));
    assert_eq!(
        option_innerless_array_index_receiver_decline.code(),
        "TPZ6PY0001"
    );
    assert!(
        matches!(
            option_innerless_array_index_receiver_decline.kind,
            PyEmitErrorKind::Unsupported(_)
        ),
        "Array<Option<int>> optional array-index receivers should decline instead of silently wiring an inner int receiver: {option_innerless_array_index_receiver_decline:?}"
    );

    let option_innerless_named_array_index_receiver_decline = emit_error_for_source(include_str!(
        "fixtures/concurrent_storage_callbacks/075_case_75.tpz"
    ));
    assert_eq!(
        option_innerless_named_array_index_receiver_decline.code(),
        "TPZ6PY0001"
    );
    assert!(
        matches!(
            option_innerless_named_array_index_receiver_decline.kind,
            PyEmitErrorKind::Unsupported(_)
        ),
        "Array<Option<int>> named optional array-index receivers should decline instead of silently wiring an inner int receiver: {option_innerless_named_array_index_receiver_decline:?}"
    );

    let option_ok_or_else_named_plain = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/076_option_ok_or_else_named_plain.tpz"
    ));
    assert!(
        option_ok_or_else_named_plain.contains("tpz_option_ok_or_else(")
            && !option_ok_or_else_named_plain.contains("tpz_option_ok_or_else__co("),
        "plain named Option.okOrElse calls should stay on the non-cooperative helper: {option_ok_or_else_named_plain}"
    );
    assert_generated_python_gates(&option_ok_or_else_named_plain)
        .unwrap_or_else(|e| panic!("plain named Option.okOrElse Python gate failed: {e}"));

    let option_ok_or_else_some_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/077_option_ok_or_else_some_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_some_same_arm_dynamic_index_write,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse Some",
    );

    let option_ok_or_else_some_same_arm_dynamic_index_write_non_call = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/078_option_ok_or_else_some_same_arm_dynamic_index_write_non_call.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_ok_or_else_some_same_arm_dynamic_index_write_non_call,
        "yield from tpz_option_ok_or_else__co(",
        "Option.okOrElse Some non-call",
    );

    let result_map_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/079_result_map_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_map_same_arm_dynamic_index_write,
        "yield from tpz_result_map__co(",
        "Result.map",
    );

    let result_flat_map_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/080_result_flat_map_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap",
    );

    let option_flat_map_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/081_option_flat_map_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_match_same_arm_dynamic_index_write,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap match",
    );
    assert_generated_python_ok_int(
        &option_flat_map_match_same_arm_dynamic_index_write,
        6,
        "Option.flatMap match same-arm dynamic-index write parity",
    );

    let option_flat_map_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/082_option_flat_map_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_loop_same_arm_dynamic_index_write,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap loop",
    );
    assert_generated_python_ok_int(
        &option_flat_map_loop_same_arm_dynamic_index_write,
        6,
        "Option.flatMap loop same-arm dynamic-index write parity",
    );

    let option_flat_map_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/083_option_flat_map_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_while_same_arm_dynamic_index_write,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap while",
    );
    assert_generated_python_ok_int(
        &option_flat_map_while_same_arm_dynamic_index_write,
        6,
        "Option.flatMap while same-arm dynamic-index write parity",
    );

    let result_flat_map_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/084_result_flat_map_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_match_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap match",
    );
    assert_generated_python_ok_int(
        &result_flat_map_match_same_arm_dynamic_index_write,
        6,
        "Result.flatMap match same-arm dynamic-index write parity",
    );

    let result_flat_map_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/085_result_flat_map_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_loop_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap loop",
    );
    assert_generated_python_ok_int(
        &result_flat_map_loop_same_arm_dynamic_index_write,
        6,
        "Result.flatMap loop same-arm dynamic-index write parity",
    );

    let result_flat_map_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/086_result_flat_map_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_while_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap while",
    );
    assert_generated_python_ok_int(
        &result_flat_map_while_same_arm_dynamic_index_write,
        6,
        "Result.flatMap while same-arm dynamic-index write parity",
    );

    for case in [
        (
            "Result.map match",
            include_str!("fixtures/concurrent_storage_callbacks/087_result_map_match.tpz"),
        ),
        (
            "Result.map loop",
            include_str!("fixtures/concurrent_storage_callbacks/088_result_map_loop.tpz"),
        ),
        (
            "Result.map while",
            include_str!("fixtures/concurrent_storage_callbacks/089_result_map_while.tpz"),
        ),
    ] {
        let emitted = emit_source(case.1);
        assert_same_arm_hof_dynamic_write_gate(&emitted, "yield from tpz_result_map__co(", case.0);
        assert_generated_python_ok_int(&emitted, 6, case.0);
    }

    let result_map_array_index_receiver_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/090_result_map_array_index_receiver_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_map_array_index_receiver_same_arm_dynamic_index_write,
        "yield from tpz_result_map__co(",
        "Result.map array-index receiver",
    );
    assert!(
        result_map_array_index_receiver_same_arm_dynamic_index_write
            .matches("tpz_index(")
            .count()
            >= 2,
        "Result.map array-index receiver should preserve receiver and callback runtime indexes: {result_map_array_index_receiver_same_arm_dynamic_index_write}"
    );
    assert_generated_python_ok_int(
        &result_map_array_index_receiver_same_arm_dynamic_index_write,
        6,
        "Result.map array-index receiver Ok parity",
    );

    let result_map_array_index_receiver_err_eval_order = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/091_result_map_array_index_receiver_err_eval_order.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_map_array_index_receiver_err_eval_order,
        "yield from tpz_result_map__co(",
        "Result.map array-index receiver Err",
    );
    let result_map_receiver_tick = result_map_array_index_receiver_err_eval_order
        .find("_t_7469636b__co(1")
        .unwrap_or_else(|| {
            panic!(
                "missing receiver-index tick in Result.map array-index receiver gate: {result_map_array_index_receiver_err_eval_order}"
            )
        });
    let result_map_callback_tick = result_map_array_index_receiver_err_eval_order
        .find("_t_7469636b__co(2")
        .unwrap_or_else(|| {
            panic!(
                "missing callback-index tick in Result.map array-index receiver gate: {result_map_array_index_receiver_err_eval_order}"
            )
        });
    assert!(
        result_map_receiver_tick < result_map_callback_tick,
        "Result.map array-index receiver should evaluate receiver index before callback index: {result_map_array_index_receiver_err_eval_order}"
    );
    assert_generated_python_ok_int(
        &result_map_array_index_receiver_err_eval_order,
        1207,
        "Result.map array-index receiver Err payload/eval-order parity",
    );

    let result_flat_map_array_index_receiver_same_arm_dynamic_index_write = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/092_result_flat_map_array_index_receiver_same_arm_dynamic_index_write.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_array_index_receiver_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap array-index receiver",
    );
    assert!(
        result_flat_map_array_index_receiver_same_arm_dynamic_index_write
            .matches("tpz_index(")
            .count()
            >= 2,
        "Result.flatMap array-index receiver should preserve receiver and callback runtime indexes: {result_flat_map_array_index_receiver_same_arm_dynamic_index_write}"
    );
    assert_generated_python_ok_int(
        &result_flat_map_array_index_receiver_same_arm_dynamic_index_write,
        6,
        "Result.flatMap array-index receiver Ok parity",
    );

    let result_flat_map_array_index_receiver_err_eval_order = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/093_result_flat_map_array_index_receiver_err_eval_order.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_array_index_receiver_err_eval_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap array-index receiver Err",
    );
    let result_flat_map_receiver_tick = result_flat_map_array_index_receiver_err_eval_order
        .find("_t_7469636b__co(1")
        .unwrap_or_else(|| {
            panic!(
                "missing receiver-index tick in Result.flatMap array-index receiver gate: {result_flat_map_array_index_receiver_err_eval_order}"
            )
        });
    let result_flat_map_callback_tick = result_flat_map_array_index_receiver_err_eval_order
        .find("_t_7469636b__co(2")
        .unwrap_or_else(|| {
            panic!(
                "missing callback-index tick in Result.flatMap array-index receiver gate: {result_flat_map_array_index_receiver_err_eval_order}"
            )
        });
    assert!(
        result_flat_map_receiver_tick < result_flat_map_callback_tick,
        "Result.flatMap array-index receiver should evaluate receiver index before callback index: {result_flat_map_array_index_receiver_err_eval_order}"
    );
    assert_generated_python_ok_int(
        &result_flat_map_array_index_receiver_err_eval_order,
        1207,
        "Result.flatMap array-index receiver Err payload/eval-order parity",
    );

    let result_map_record_field_receiver_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/094_result_map_record_field_receiver_same_arm_dynamic_index_write.tpz"
    ));
    assert!(
        result_map_record_field_receiver_same_arm_dynamic_index_write.contains("tpz_member("),
        "Result.map record-field receiver should be read through tpz_member: {result_map_record_field_receiver_same_arm_dynamic_index_write}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_map_record_field_receiver_same_arm_dynamic_index_write,
        "yield from tpz_result_map__co(",
        "Result.map record-field receiver",
    );
    assert_generated_python_ok_int(
        &result_map_record_field_receiver_same_arm_dynamic_index_write,
        6,
        "Result.map record-field receiver Ok parity",
    );

    let result_map_record_field_receiver_err_eval_order = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/095_result_map_record_field_receiver_err_eval_order.tpz"
    ));
    assert!(
        result_map_record_field_receiver_err_eval_order.contains("tpz_member("),
        "Result.map record-field receiver Err should be read through tpz_member: {result_map_record_field_receiver_err_eval_order}"
    );
    assert!(
        result_map_record_field_receiver_err_eval_order.contains("_t_6d616b65486f6c646572__co"),
        "Result.map record-field receiver Err should preserve the function-produced holder call: {result_map_record_field_receiver_err_eval_order}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_map_record_field_receiver_err_eval_order,
        "yield from tpz_result_map__co(",
        "Result.map record-field receiver Err",
    );
    assert_generated_python_ok_int(
        &result_map_record_field_receiver_err_eval_order,
        1207,
        "Result.map record-field receiver Err payload/eval-order parity",
    );

    let result_flat_map_record_field_receiver_same_arm_dynamic_index_write = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/096_result_flat_map_record_field_receiver_same_arm_dynamic_index_write.tpz"
        ),
    );
    assert!(
        result_flat_map_record_field_receiver_same_arm_dynamic_index_write.contains("tpz_member("),
        "Result.flatMap record-field receiver should be read through tpz_member: {result_flat_map_record_field_receiver_same_arm_dynamic_index_write}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_record_field_receiver_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap record-field receiver",
    );
    assert_generated_python_ok_int(
        &result_flat_map_record_field_receiver_same_arm_dynamic_index_write,
        6,
        "Result.flatMap record-field receiver Ok parity",
    );

    let result_flat_map_record_field_receiver_err_eval_order = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/097_result_flat_map_record_field_receiver_err_eval_order.tpz"
    ));
    assert!(
        result_flat_map_record_field_receiver_err_eval_order.contains("tpz_member("),
        "Result.flatMap record-field receiver Err should be read through tpz_member: {result_flat_map_record_field_receiver_err_eval_order}"
    );
    assert!(
        result_flat_map_record_field_receiver_err_eval_order
            .contains("_t_6d616b65486f6c646572__co"),
        "Result.flatMap record-field receiver Err should preserve the function-produced holder call: {result_flat_map_record_field_receiver_err_eval_order}"
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_record_field_receiver_err_eval_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap record-field receiver Err",
    );
    assert_generated_python_ok_int(
        &result_flat_map_record_field_receiver_err_eval_order,
        1207,
        "Result.flatMap record-field receiver Err payload/eval-order parity",
    );

    for case in [
        (
            "named Result.map array-index receiver Ok parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/098_named_result_map_array_index_receiver_ok_parity.tpz"
            ),
            "yield from tpz_result_map__co(",
            6,
            true,
            false,
            false,
            false,
        ),
        (
            "named Result.map array-index receiver Err payload/eval-order parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/099_named_result_map_array_index_receiver_err_payload_eval_order_parity.tpz"
            ),
            "yield from tpz_result_map__co(",
            1207,
            false,
            false,
            false,
            true,
        ),
        (
            "named Result.flatMap array-index receiver Ok parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/100_named_result_flatmap_array_index_receiver_ok_parity.tpz"
            ),
            "yield from tpz_result_flat_map__co(",
            6,
            true,
            false,
            false,
            false,
        ),
        (
            "named Result.flatMap array-index receiver Err payload/eval-order parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/101_named_result_flatmap_array_index_receiver_err_payload_eval_order_parity.tpz"
            ),
            "yield from tpz_result_flat_map__co(",
            1207,
            false,
            false,
            false,
            true,
        ),
        (
            "named Result.map record-field receiver Ok parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/102_named_result_map_record_field_receiver_ok_parity.tpz"
            ),
            "yield from tpz_result_map__co(",
            6,
            false,
            true,
            false,
            false,
        ),
        (
            "named Result.map record-field receiver Err payload/eval-order parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/103_named_result_map_record_field_receiver_err_payload_eval_order_parity.tpz"
            ),
            "yield from tpz_result_map__co(",
            1207,
            false,
            true,
            true,
            true,
        ),
        (
            "named Result.flatMap record-field receiver Ok parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/104_named_result_flatmap_record_field_receiver_ok_parity.tpz"
            ),
            "yield from tpz_result_flat_map__co(",
            6,
            false,
            true,
            false,
            false,
        ),
        (
            "named Result.flatMap record-field receiver Err payload/eval-order parity",
            include_str!(
                "fixtures/concurrent_storage_callbacks/105_named_result_flatmap_record_field_receiver_err_payload_eval_order_parity.tpz"
            ),
            "yield from tpz_result_flat_map__co(",
            1207,
            false,
            true,
            true,
            true,
        ),
    ] {
        let emitted = emit_source(case.1);
        assert_same_arm_hof_dynamic_write_gate(&emitted, case.2, case.0);
        if case.4 {
            assert!(
                emitted.matches("tpz_index(").count() >= 2,
                "{label} should preserve receiver and callback runtime indexes: {emitted}",
                label = case.0
            );
        }
        if case.5 {
            assert!(
                emitted.contains("tpz_member("),
                "{label} should read record-field receiver through tpz_member: {emitted}",
                label = case.0
            );
        }
        if case.6 {
            assert!(
                emitted.contains("_t_6d616b65486f6c646572__co"),
                "{label} should preserve the function-produced holder call: {emitted}",
                label = case.0
            );
        }
        if case.7 {
            let receiver_tick = emitted.find("_t_7469636b__co(1").unwrap_or_else(|| {
                panic!(
                    "missing receiver-index tick in {label}: {emitted}",
                    label = case.0
                )
            });
            let callback_tick = emitted.find("_t_7469636b__co(2").unwrap_or_else(|| {
                panic!(
                    "missing callback-index tick in {label}: {emitted}",
                    label = case.0
                )
            });
            assert!(
                receiver_tick < callback_tick,
                "{label} should evaluate receiver index before callback index: {emitted}",
                label = case.0
            );
        }
        assert_generated_python_ok_int(&emitted, case.3, case.0);
    }

    let option_flat_map_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/106_option_flat_map_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_named_match_same_arm_dynamic_index_write,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap named match",
    );
    assert_generated_python_ok_int(
        &option_flat_map_named_match_same_arm_dynamic_index_write,
        6,
        "Option.flatMap named match same-arm dynamic-index write parity",
    );

    let option_flat_map_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/107_option_flat_map_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap named loop",
    );
    assert_generated_python_ok_int(
        &option_flat_map_named_loop_same_arm_dynamic_index_write,
        6,
        "Option.flatMap named loop same-arm dynamic-index write parity",
    );

    let option_flat_map_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/108_option_flat_map_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_named_while_same_arm_dynamic_index_write,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap named while",
    );
    assert_generated_python_ok_int(
        &option_flat_map_named_while_same_arm_dynamic_index_write,
        6,
        "Option.flatMap named while same-arm dynamic-index write parity",
    );

    let result_flat_map_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/109_result_flat_map_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_named_match_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap named match",
    );
    assert_generated_python_ok_int(
        &result_flat_map_named_match_same_arm_dynamic_index_write,
        6,
        "Result.flatMap named match same-arm dynamic-index write parity",
    );

    let result_flat_map_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/110_result_flat_map_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap named loop",
    );
    assert_generated_python_ok_int(
        &result_flat_map_named_loop_same_arm_dynamic_index_write,
        6,
        "Result.flatMap named loop same-arm dynamic-index write parity",
    );

    let result_flat_map_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/111_result_flat_map_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_named_while_same_arm_dynamic_index_write,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap named while",
    );
    assert_generated_python_ok_int(
        &result_flat_map_named_while_same_arm_dynamic_index_write,
        6,
        "Result.flatMap named while same-arm dynamic-index write parity",
    );

    for case in [
        (
            "Result.map named match",
            include_str!("fixtures/concurrent_storage_callbacks/112_result_map_named_match.tpz"),
        ),
        (
            "Result.map named loop",
            include_str!("fixtures/concurrent_storage_callbacks/113_result_map_named_loop.tpz"),
        ),
        (
            "Result.map named while",
            include_str!("fixtures/concurrent_storage_callbacks/114_result_map_named_while.tpz"),
        ),
    ] {
        let emitted = emit_source(case.1);
        assert_same_arm_hof_dynamic_write_gate(&emitted, "yield from tpz_result_map__co(", case.0);
        assert_generated_python_ok_int(&emitted, 6, case.0);
    }

    let option_flat_map_none_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/115_option_flat_map_none_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_none_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap None",
    );
    assert_generated_python_ok_int(
        &option_flat_map_none_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Option.flatMap None callback-index eval but non-call order parity",
    );

    let option_flat_map_named_none_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/116_option_flat_map_named_none_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_named_none_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap named None",
    );
    assert_generated_python_ok_int(
        &option_flat_map_named_none_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Option.flatMap None callback-index eval but non-call order parity",
    );

    let result_flat_map_err_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/117_result_flat_map_err_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_err_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap Err",
    );
    assert_generated_python_ok_int(
        &result_flat_map_err_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Result.flatMap Err callback-index eval but non-call order parity",
    );

    let result_flat_map_named_err_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/118_result_flat_map_named_err_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_named_err_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap named Err",
    );
    assert_generated_python_ok_int(
        &result_flat_map_named_err_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Result.flatMap Err callback-index eval but non-call order parity",
    );

    let option_flat_map_none_match_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/119_option_flat_map_none_match_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_none_match_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap None match",
    );
    assert_generated_python_ok_int(
        &option_flat_map_none_match_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Option.flatMap None match callback-index eval but non-call order parity",
    );

    let option_flat_map_none_loop_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/120_option_flat_map_none_loop_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_none_loop_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap None loop",
    );
    assert_generated_python_ok_int(
        &option_flat_map_none_loop_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Option.flatMap None loop callback-index eval but non-call order parity",
    );

    let option_flat_map_none_while_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/121_option_flat_map_none_while_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_none_while_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap None while",
    );
    assert_generated_python_ok_int(
        &option_flat_map_none_while_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Option.flatMap None while callback-index eval but non-call order parity",
    );

    let option_flat_map_named_none_match_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/122_option_flat_map_named_none_match_same_arm_dynamic_index_write_non_call_o.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_named_none_match_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap named None match",
    );
    assert_generated_python_ok_int(
        &option_flat_map_named_none_match_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Option.flatMap None match callback-index eval but non-call order parity",
    );

    let option_flat_map_named_none_loop_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/123_option_flat_map_named_none_loop_same_arm_dynamic_index_write_non_call_or.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_named_none_loop_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap named None loop",
    );
    assert_generated_python_ok_int(
        &option_flat_map_named_none_loop_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Option.flatMap None loop callback-index eval but non-call order parity",
    );

    let option_flat_map_named_none_while_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/124_option_flat_map_named_none_while_same_arm_dynamic_index_write_non_call_o.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &option_flat_map_named_none_while_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_option_flat_map__co(",
        "Option.flatMap named None while",
    );
    assert_generated_python_ok_int(
        &option_flat_map_named_none_while_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Option.flatMap None while callback-index eval but non-call order parity",
    );

    let result_flat_map_err_match_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/125_result_flat_map_err_match_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_err_match_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap Err match",
    );
    assert_generated_python_ok_int(
        &result_flat_map_err_match_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Result.flatMap Err match callback-index eval but non-call order parity",
    );

    let result_flat_map_err_loop_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/126_result_flat_map_err_loop_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_err_loop_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap Err loop",
    );
    assert_generated_python_ok_int(
        &result_flat_map_err_loop_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Result.flatMap Err loop callback-index eval but non-call order parity",
    );

    let result_flat_map_err_while_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/127_result_flat_map_err_while_same_arm_dynamic_index_write_non_call_order.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_err_while_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap Err while",
    );
    assert_generated_python_ok_int(
        &result_flat_map_err_while_same_arm_dynamic_index_write_non_call_order,
        1207,
        "Result.flatMap Err while callback-index eval but non-call order parity",
    );

    let result_flat_map_named_err_match_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/128_result_flat_map_named_err_match_same_arm_dynamic_index_write_non_call_or.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_named_err_match_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap named Err match",
    );
    assert_generated_python_ok_int(
        &result_flat_map_named_err_match_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Result.flatMap Err match callback-index eval but non-call order parity",
    );

    let result_flat_map_named_err_loop_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/129_result_flat_map_named_err_loop_same_arm_dynamic_index_write_non_call_ord.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_named_err_loop_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap named Err loop",
    );
    assert_generated_python_ok_int(
        &result_flat_map_named_err_loop_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Result.flatMap Err loop callback-index eval but non-call order parity",
    );

    let result_flat_map_named_err_while_same_arm_dynamic_index_write_non_call_order = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/130_result_flat_map_named_err_while_same_arm_dynamic_index_write_non_call_or.tpz"
        ),
    );
    assert_same_arm_hof_dynamic_write_gate(
        &result_flat_map_named_err_while_same_arm_dynamic_index_write_non_call_order,
        "yield from tpz_result_flat_map__co(",
        "Result.flatMap named Err while",
    );
    assert_generated_python_ok_int(
        &result_flat_map_named_err_while_same_arm_dynamic_index_write_non_call_order,
        1207,
        "named Result.flatMap Err while callback-index eval but non-call order parity",
    );

    let dynamic_out_of_range = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/131_dynamic_out_of_range.tpz"
    ));
    assert!(
        dynamic_out_of_range.contains("yield from tpz_array_map__co(")
            && dynamic_out_of_range.contains("tpz_index(")
            && !dynamic_out_of_range.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "dynamic out-of-range callback array indices should preserve the runtime read and driver without static callback recovery: {dynamic_out_of_range}"
    );
    assert_generated_python_gates(&dynamic_out_of_range).unwrap_or_else(|e| {
        panic!("dynamic out-of-range callback array index Python gate failed: {e}")
    });

    let dynamic_negative_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/132_dynamic_negative_index.tpz"
    ));
    assert!(
        dynamic_negative_index.contains("yield from tpz_array_map__co(")
            && dynamic_negative_index.contains("tpz_index(")
            && !dynamic_negative_index.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "dynamic negative callback array indices should preserve the runtime read and driver without static callback recovery: {dynamic_negative_index}"
    );
    assert_generated_python_gates(&dynamic_negative_index).unwrap_or_else(|e| {
        panic!("dynamic negative callback array index Python gate failed: {e}")
    });

    let map_values_same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/133_map_values_same_target_dynamic_index.tpz"
    ));
    assert!(
        map_values_same_target_dynamic_index.contains("yield from tpz_map_map_values__co(")
            && map_values_same_target_dynamic_index.contains("tpz_index(")
            && !map_values_same_target_dynamic_index.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "same-target dynamic Map.mapValues callback indices should preserve the runtime read and driver without static callback recovery: {map_values_same_target_dynamic_index}"
    );
    assert_generated_python_gates(&map_values_same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic Map.mapValues callback index Python gate failed: {e}")
    });

    let map_values_dynamic_out_of_range = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/134_map_values_dynamic_out_of_range.tpz"
    ));
    assert!(
        map_values_dynamic_out_of_range.contains("yield from tpz_map_map_values__co(")
            && map_values_dynamic_out_of_range.contains("tpz_index(")
            && direct_co_callback_recovery_count(&map_values_dynamic_out_of_range) == 0,
        "dynamic out-of-range Map.mapValues callback indices should preserve the runtime read and driver without static callback recovery: {map_values_dynamic_out_of_range}"
    );
    assert_generated_python_gates(&map_values_dynamic_out_of_range).unwrap_or_else(|e| {
        panic!("dynamic out-of-range Map.mapValues callback index Python gate failed: {e}")
    });

    let map_values_static_callback_in_concurrent = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/135_map_values_static_callback_in_concurrent.tpz"
    ));
    assert!(
        map_values_static_callback_in_concurrent.contains("yield from tpz_map_map_values__co(")
            && direct_co_callback_recovery_count(&map_values_static_callback_in_concurrent) > 0,
        "static Map.mapValues callbacks in concurrent arms should expose the direct callback-recovery token used by dynamic negative gates: {map_values_static_callback_in_concurrent}"
    );
    assert_generated_python_gates(&map_values_static_callback_in_concurrent)
        .unwrap_or_else(|e| panic!("static Map.mapValues callback Python gate failed: {e}"));

    let map_values_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/136_map_values_same_arm_dynamic_index_write.tpz"
    ));
    assert!(
        map_values_same_arm_dynamic_index_write.contains("yield from tpz_map_map_values__co(")
            && map_values_same_arm_dynamic_index_write.contains("tpz_index_slot(")
            && map_values_same_arm_dynamic_index_write.contains("tpz_index_slot_set(")
            && map_values_same_arm_dynamic_index_write.contains("tpz_index(")
            && direct_co_callback_recovery_count(&map_values_same_arm_dynamic_index_write) == 0,
        "same-arm Map.mapValues dynamic-index writes should preserve the runtime write/read and driver without static callback recovery: {map_values_same_arm_dynamic_index_write}"
    );
    assert_generated_python_gates(&map_values_same_arm_dynamic_index_write).unwrap_or_else(|e| {
        panic!("same-arm dynamic-index Map.mapValues write Python gate failed: {e}")
    });

    let map_values_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/137_map_values_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_values_match_same_arm_dynamic_index_write,
        "yield from tpz_map_map_values__co(",
        "Map.mapValues match",
    );
    assert_generated_python_ok_int(
        &map_values_match_same_arm_dynamic_index_write,
        12,
        "Map.mapValues match dynamic-index write/read parity",
    );

    let map_values_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/138_map_values_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_values_loop_same_arm_dynamic_index_write,
        "yield from tpz_map_map_values__co(",
        "Map.mapValues loop",
    );
    assert_generated_python_ok_int(
        &map_values_loop_same_arm_dynamic_index_write,
        12,
        "Map.mapValues loop dynamic-index write/read parity",
    );

    let map_values_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/139_map_values_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_values_while_same_arm_dynamic_index_write,
        "yield from tpz_map_map_values__co(",
        "Map.mapValues while",
    );
    assert_generated_python_ok_int(
        &map_values_while_same_arm_dynamic_index_write,
        12,
        "Map.mapValues while dynamic-index write/read parity",
    );

    let map_values_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/140_map_values_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_values_named_match_same_arm_dynamic_index_write,
        "yield from tpz_map_map_values__co(",
        "Map.mapValues named match",
    );
    assert_generated_python_ok_int(
        &map_values_named_match_same_arm_dynamic_index_write,
        12,
        "named Map.mapValues match dynamic-index write/read parity",
    );

    let map_values_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/141_map_values_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_values_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_map_map_values__co(",
        "Map.mapValues named loop",
    );
    assert_generated_python_ok_int(
        &map_values_named_loop_same_arm_dynamic_index_write,
        12,
        "named Map.mapValues loop dynamic-index write/read parity",
    );

    let map_values_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/142_map_values_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_values_named_while_same_arm_dynamic_index_write,
        "yield from tpz_map_map_values__co(",
        "Map.mapValues named while",
    );
    assert_generated_python_ok_int(
        &map_values_named_while_same_arm_dynamic_index_write,
        12,
        "named Map.mapValues while dynamic-index write/read parity",
    );

    let map_filter_static_callback_in_concurrent = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/143_map_filter_static_callback_in_concurrent.tpz"
    ));
    assert!(
        map_filter_static_callback_in_concurrent.contains("yield from tpz_map_filter__co(")
            && direct_co_callback_recovery_count(&map_filter_static_callback_in_concurrent) > 0,
        "static Map.filter callbacks in concurrent arms should expose the direct callback-recovery token used by dynamic negative gates: {map_filter_static_callback_in_concurrent}"
    );
    assert_generated_python_gates(&map_filter_static_callback_in_concurrent)
        .unwrap_or_else(|e| panic!("static Map.filter callback Python gate failed: {e}"));

    let map_filter_same_target_local_immutable_index_read = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/144_map_filter_same_target_local_immutable_index_read.tpz"
    ));
    assert!(
        map_filter_same_target_local_immutable_index_read
            .contains("yield from tpz_map_filter__co(")
            && map_filter_same_target_local_immutable_index_read.contains("tpz_index(")
            && direct_co_callback_recovery_count(
                &map_filter_same_target_local_immutable_index_read
            ) == 0,
        "same-target local immutable Map.filter index reads should stay unfolded and use the runtime driver without static callback recovery: {map_filter_same_target_local_immutable_index_read}"
    );
    assert_generated_python_gates(&map_filter_same_target_local_immutable_index_read)
        .unwrap_or_else(|e| {
            panic!("same-target local immutable Map.filter index-read Python gate failed: {e}")
        });

    let map_filter_dynamic_out_of_range = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/145_map_filter_dynamic_out_of_range.tpz"
    ));
    assert!(
        map_filter_dynamic_out_of_range.contains("yield from tpz_map_filter__co(")
            && map_filter_dynamic_out_of_range.contains("tpz_index(")
            && direct_co_callback_recovery_count(&map_filter_dynamic_out_of_range) == 0,
        "dynamic out-of-range Map.filter callback indices should preserve the runtime read and driver without static callback recovery: {map_filter_dynamic_out_of_range}"
    );
    assert_generated_python_gates(&map_filter_dynamic_out_of_range).unwrap_or_else(|e| {
        panic!("dynamic out-of-range Map.filter callback index Python gate failed: {e}")
    });

    let map_filter_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/146_map_filter_same_arm_dynamic_index_write.tpz"
    ));
    assert!(
        map_filter_same_arm_dynamic_index_write.contains("yield from tpz_map_filter__co(")
            && map_filter_same_arm_dynamic_index_write.contains("tpz_index_slot(")
            && map_filter_same_arm_dynamic_index_write.contains("tpz_index_slot_set(")
            && map_filter_same_arm_dynamic_index_write.contains("tpz_index(")
            && direct_co_callback_recovery_count(&map_filter_same_arm_dynamic_index_write) == 0,
        "same-arm Map.filter dynamic-index writes should preserve the runtime write/read and driver without static callback recovery: {map_filter_same_arm_dynamic_index_write}"
    );
    assert_generated_python_gates(&map_filter_same_arm_dynamic_index_write).unwrap_or_else(|e| {
        panic!("same-arm dynamic-index Map.filter write Python gate failed: {e}")
    });

    let map_filter_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/147_map_filter_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_filter_match_same_arm_dynamic_index_write,
        "yield from tpz_map_filter__co(",
        "Map.filter match",
    );
    assert_generated_python_ok_int(
        &map_filter_match_same_arm_dynamic_index_write,
        5,
        "Map.filter match dynamic-index write/read parity",
    );

    let map_filter_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/148_map_filter_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_filter_loop_same_arm_dynamic_index_write,
        "yield from tpz_map_filter__co(",
        "Map.filter loop",
    );
    assert_generated_python_ok_int(
        &map_filter_loop_same_arm_dynamic_index_write,
        5,
        "Map.filter loop dynamic-index write/read parity",
    );

    let map_filter_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/149_map_filter_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_filter_while_same_arm_dynamic_index_write,
        "yield from tpz_map_filter__co(",
        "Map.filter while",
    );
    assert_generated_python_ok_int(
        &map_filter_while_same_arm_dynamic_index_write,
        5,
        "Map.filter while dynamic-index write/read parity",
    );

    let map_filter_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/150_map_filter_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_filter_named_match_same_arm_dynamic_index_write,
        "yield from tpz_map_filter__co(",
        "Map.filter named match",
    );
    assert_generated_python_ok_int(
        &map_filter_named_match_same_arm_dynamic_index_write,
        5,
        "named Map.filter match dynamic-index write/read parity",
    );

    let map_filter_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/151_map_filter_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_filter_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_map_filter__co(",
        "Map.filter named loop",
    );
    assert_generated_python_ok_int(
        &map_filter_named_loop_same_arm_dynamic_index_write,
        5,
        "named Map.filter loop dynamic-index write/read parity",
    );

    let map_filter_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/152_map_filter_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_filter_named_while_same_arm_dynamic_index_write,
        "yield from tpz_map_filter__co(",
        "Map.filter named while",
    );
    assert_generated_python_ok_int(
        &map_filter_named_while_same_arm_dynamic_index_write,
        5,
        "named Map.filter while dynamic-index write/read parity",
    );

    let map_update_static_callback_in_concurrent = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/153_map_update_static_callback_in_concurrent.tpz"
    ));
    assert!(
        map_update_static_callback_in_concurrent.contains("yield from tpz_map_update__co(")
            && direct_co_callback_recovery_count(&map_update_static_callback_in_concurrent) > 0,
        "static Map.update callbacks in concurrent arms should expose the direct callback-recovery token used by dynamic negative gates: {map_update_static_callback_in_concurrent}"
    );
    assert_generated_python_gates(&map_update_static_callback_in_concurrent)
        .unwrap_or_else(|e| panic!("static Map.update callback Python gate failed: {e}"));

    let map_update_same_target_dynamic_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/154_map_update_same_target_dynamic_index.tpz"
    ));
    assert!(
        map_update_same_target_dynamic_index.contains("yield from tpz_map_update__co(")
            && map_update_same_target_dynamic_index.contains("tpz_index(")
            && !map_update_same_target_dynamic_index.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "same-target dynamic Map.update callback indices should preserve the runtime read and present-key driver without static callback recovery: {map_update_same_target_dynamic_index}"
    );
    assert_generated_python_gates(&map_update_same_target_dynamic_index).unwrap_or_else(|e| {
        panic!("same-target dynamic Map.update callback index Python gate failed: {e}")
    });

    let map_update_dynamic_out_of_range = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/155_map_update_dynamic_out_of_range.tpz"
    ));
    assert!(
        map_update_dynamic_out_of_range.contains("yield from tpz_map_update__co(")
            && map_update_dynamic_out_of_range.contains("tpz_index(")
            && direct_co_callback_recovery_count(&map_update_dynamic_out_of_range) == 0,
        "dynamic out-of-range Map.update callback indices should preserve the runtime read and present-key driver without static callback recovery: {map_update_dynamic_out_of_range}"
    );
    assert_generated_python_gates(&map_update_dynamic_out_of_range).unwrap_or_else(|e| {
        panic!("dynamic out-of-range Map.update callback index Python gate failed: {e}")
    });

    let map_update_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/156_map_update_same_arm_dynamic_index_write.tpz"
    ));
    assert!(
        map_update_same_arm_dynamic_index_write.contains("yield from tpz_map_update__co(")
            && map_update_same_arm_dynamic_index_write.contains("tpz_index_slot(")
            && map_update_same_arm_dynamic_index_write.contains("tpz_index_slot_set(")
            && map_update_same_arm_dynamic_index_write.contains("tpz_index(")
            && direct_co_callback_recovery_count(&map_update_same_arm_dynamic_index_write) == 0,
        "same-arm Map.update dynamic-index writes should preserve the runtime write/read and driver without static callback recovery: {map_update_same_arm_dynamic_index_write}"
    );
    assert_generated_python_gates(&map_update_same_arm_dynamic_index_write).unwrap_or_else(|e| {
        panic!("same-arm dynamic-index Map.update write Python gate failed: {e}")
    });

    let map_update_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/157_map_update_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_update_match_same_arm_dynamic_index_write,
        "yield from tpz_map_update__co(",
        "Map.update match",
    );
    assert_generated_python_ok_int(
        &map_update_match_same_arm_dynamic_index_write,
        8,
        "Map.update match dynamic-index write/read parity",
    );

    let map_update_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/158_map_update_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_update_loop_same_arm_dynamic_index_write,
        "yield from tpz_map_update__co(",
        "Map.update loop",
    );
    assert_generated_python_ok_int(
        &map_update_loop_same_arm_dynamic_index_write,
        8,
        "Map.update loop dynamic-index write/read parity",
    );

    let map_update_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/159_map_update_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_update_while_same_arm_dynamic_index_write,
        "yield from tpz_map_update__co(",
        "Map.update while",
    );
    assert_generated_python_ok_int(
        &map_update_while_same_arm_dynamic_index_write,
        8,
        "Map.update while dynamic-index write/read parity",
    );

    let map_update_named_match_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/160_map_update_named_match_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_update_named_match_same_arm_dynamic_index_write,
        "yield from tpz_map_update__co(",
        "Map.update named match",
    );
    assert_generated_python_ok_int(
        &map_update_named_match_same_arm_dynamic_index_write,
        8,
        "named Map.update match dynamic-index write/read parity",
    );

    let map_update_named_loop_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/161_map_update_named_loop_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_update_named_loop_same_arm_dynamic_index_write,
        "yield from tpz_map_update__co(",
        "Map.update named loop",
    );
    assert_generated_python_ok_int(
        &map_update_named_loop_same_arm_dynamic_index_write,
        8,
        "named Map.update loop dynamic-index write/read parity",
    );

    let map_update_named_while_same_arm_dynamic_index_write = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/162_map_update_named_while_same_arm_dynamic_index_write.tpz"
    ));
    assert_same_arm_hof_dynamic_write_gate(
        &map_update_named_while_same_arm_dynamic_index_write,
        "yield from tpz_map_update__co(",
        "Map.update named while",
    );
    assert_generated_python_ok_int(
        &map_update_named_while_same_arm_dynamic_index_write,
        8,
        "named Map.update while dynamic-index write/read parity",
    );

    let map_update_absent_key_match_same_arm_dynamic_index_callback_skip = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/163_map_update_absent_key_match_same_arm_dynamic_index_callback_skip.tpz"
        ),
    );
    assert_absent_key_skip_wrapper_gate(
        &map_update_absent_key_match_same_arm_dynamic_index_callback_skip,
        "match",
    );
    assert_generated_python_ok_int(
        &map_update_absent_key_match_same_arm_dynamic_index_callback_skip,
        8,
        "absent-key Map.update match callback skip parity",
    );

    let map_update_absent_key_loop_same_arm_dynamic_index_callback_skip = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/164_map_update_absent_key_loop_same_arm_dynamic_index_callback_skip.tpz"
        ),
    );
    assert_absent_key_skip_wrapper_gate(
        &map_update_absent_key_loop_same_arm_dynamic_index_callback_skip,
        "loop",
    );
    assert_generated_python_ok_int(
        &map_update_absent_key_loop_same_arm_dynamic_index_callback_skip,
        8,
        "absent-key Map.update loop callback skip parity",
    );

    let map_update_absent_key_while_same_arm_dynamic_index_callback_skip = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/165_map_update_absent_key_while_same_arm_dynamic_index_callback_skip.tpz"
        ),
    );
    assert_absent_key_skip_wrapper_gate(
        &map_update_absent_key_while_same_arm_dynamic_index_callback_skip,
        "while",
    );
    assert_generated_python_ok_int(
        &map_update_absent_key_while_same_arm_dynamic_index_callback_skip,
        8,
        "absent-key Map.update while callback skip parity",
    );

    let map_update_absent_key_named_match_same_arm_dynamic_index_callback_skip = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/166_map_update_absent_key_named_match_same_arm_dynamic_index_callback_skip.tpz"
        ),
    );
    assert_absent_key_skip_wrapper_gate(
        &map_update_absent_key_named_match_same_arm_dynamic_index_callback_skip,
        "named match",
    );
    assert_generated_python_ok_int(
        &map_update_absent_key_named_match_same_arm_dynamic_index_callback_skip,
        8,
        "absent-key named Map.update match callback skip parity",
    );

    let map_update_absent_key_named_loop_same_arm_dynamic_index_callback_skip = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/167_map_update_absent_key_named_loop_same_arm_dynamic_index_callback_skip.tpz"
        ),
    );
    assert_absent_key_skip_wrapper_gate(
        &map_update_absent_key_named_loop_same_arm_dynamic_index_callback_skip,
        "named loop",
    );
    assert_generated_python_ok_int(
        &map_update_absent_key_named_loop_same_arm_dynamic_index_callback_skip,
        8,
        "absent-key named Map.update loop callback skip parity",
    );

    let map_update_absent_key_named_while_same_arm_dynamic_index_callback_skip = emit_source(
        include_str!(
            "fixtures/concurrent_storage_callbacks/168_map_update_absent_key_named_while_same_arm_dynamic_index_callback_skip.tpz"
        ),
    );
    assert_absent_key_skip_wrapper_gate(
        &map_update_absent_key_named_while_same_arm_dynamic_index_callback_skip,
        "named while",
    );
    assert_generated_python_ok_int(
        &map_update_absent_key_named_while_same_arm_dynamic_index_callback_skip,
        8,
        "absent-key named Map.update while callback skip parity",
    );

    let map_update_absent_key_dynamic_index_callback_skip = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/169_map_update_absent_key_dynamic_index_callback_skip.tpz"
    ));
    assert!(
        map_update_absent_key_dynamic_index_callback_skip
            .contains("yield from tpz_map_update__co(")
            && map_update_absent_key_dynamic_index_callback_skip.contains("tpz_index(")
            && !map_update_absent_key_dynamic_index_callback_skip
                .contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "absent-key dynamic Map.update callbacks should stay on co-lowered runtime reads without static recovery of the faulting callback: {map_update_absent_key_dynamic_index_callback_skip}"
    );
    assert_generated_python_gates(&map_update_absent_key_dynamic_index_callback_skip)
        .unwrap_or_else(|e| {
            panic!("absent-key dynamic Map.update callback skip Python gate failed: {e}")
        });

    let mutable_spread_array_source = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/170_mutable_spread_array_source.tpz"
    ));
    assert!(
        mutable_spread_array_source.contains("yield from tpz_array_map__co(")
            && mutable_spread_array_source.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "tracked local mutable spread sources should retain exact cooperative callback metadata: {mutable_spread_array_source}"
    );
    assert_generated_python_gates(&mutable_spread_array_source)
        .unwrap_or_else(|e| panic!("mutable spread-source callback array Python gate failed: {e}"));

    let empty_spread_array_source = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/171_empty_spread_array_source.tpz"
    ));
    assert!(
        empty_spread_array_source.contains("yield from tpz_array_map__co(")
            && empty_spread_array_source.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !empty_spread_array_source.contains("tpz_index(_t_63616c6c6261636b73, 0,"),
        "callback arrays with immutable empty spread sources should preserve proven cooperative metadata: {empty_spread_array_source}"
    );
    assert_generated_python_gates(&empty_spread_array_source)
        .unwrap_or_else(|e| panic!("empty spread-source callback array Python gate failed: {e}"));

    let mutable_empty_spread_array_source = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/172_mutable_empty_spread_array_source.tpz"
    ));
    assert!(
        mutable_empty_spread_array_source.contains("yield from tpz_array_map__co(")
            && mutable_empty_spread_array_source.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !mutable_empty_spread_array_source.contains("tpz_index(_t_63616c6c6261636b73, 0,"),
        "callback arrays with tracked mutable empty spread sources should preserve proven cooperative metadata: {mutable_empty_spread_array_source}"
    );
    assert_generated_python_gates(&mutable_empty_spread_array_source).unwrap_or_else(|e| {
        panic!("mutable empty spread-source callback array Python gate failed: {e}")
    });

    let mutable_empty_spread_filter_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/173_mutable_empty_spread_filter_value.tpz"
    ));
    assert_mutable_spread_array_hof_recovery(
        &mutable_empty_spread_filter_value,
        "yield from tpz_array_filter__co(",
        "6b656570",
        "Array.filter mutable-empty static-index value carrier",
    );
    assert_generated_python_ok_int(
        &mutable_empty_spread_filter_value,
        7,
        "mutable empty spread-source Array.filter value carrier",
    );

    let mutable_empty_spread_reduce_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/174_mutable_empty_spread_reduce_value.tpz"
    ));
    assert_mutable_spread_array_hof_recovery(
        &mutable_empty_spread_reduce_value,
        "yield from tpz_array_reduce__co(",
        "6d756c",
        "Array.reduce mutable-empty static-index value carrier",
    );
    assert_generated_python_ok_int(
        &mutable_empty_spread_reduce_value,
        60,
        "mutable empty spread-source Array.reduce value carrier",
    );

    let mutable_empty_spread_sorted_by_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/175_mutable_empty_spread_sorted_by_value.tpz"
    ));
    assert_mutable_spread_array_hof_recovery(
        &mutable_empty_spread_sorted_by_value,
        "yield from tpz_array_sorted_by__co(",
        "6d69644b6579",
        "Array.sortedBy mutable-empty static-index value carrier",
    );
    assert_generated_python_ok_int(
        &mutable_empty_spread_sorted_by_value,
        231,
        "mutable empty spread-source Array.sortedBy value carrier",
    );

    let mutable_empty_spread_sort_by_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/176_mutable_empty_spread_sort_by_value.tpz"
    ));
    assert_mutable_spread_array_hof_recovery(
        &mutable_empty_spread_sort_by_value,
        "yield from tpz_array_sort_by__co(",
        "6e65674b6579",
        "Array.sortBy mutable-empty static-index value carrier",
    );
    assert_generated_python_ok_int(
        &mutable_empty_spread_sort_by_value,
        321,
        "mutable empty spread-source Array.sortBy value carrier",
    );

    let mutable_empty_spread_retain_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/177_mutable_empty_spread_retain_value.tpz"
    ));
    assert_mutable_spread_array_hof_recovery(
        &mutable_empty_spread_retain_value,
        "yield from tpz_array_retain__co(",
        "6b656570",
        "Array.retain mutable-empty static-index value carrier",
    );
    assert_generated_python_ok_int(
        &mutable_empty_spread_retain_value,
        7,
        "mutable empty spread-source Array.retain value carrier",
    );

    let post_spread_source_binding_rebind_filter = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/178_post_spread_source_binding_rebind_filter.tpz"
    ));
    assert_mutable_spread_array_hof_recovery(
        &post_spread_source_binding_rebind_filter,
        "yield from tpz_array_filter__co(",
        "6b656570",
        "Array.filter post-spread source-binding rebind value carrier",
    );
    assert_generated_python_ok_int(
        &post_spread_source_binding_rebind_filter,
        7,
        "post-spread source-binding rebind must not change spread-built callback carrier",
    );

    let post_spread_source_binding_rebind_reduce = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/179_post_spread_source_binding_rebind_reduce.tpz"
    ));
    assert_mutable_spread_array_hof_recovery(
        &post_spread_source_binding_rebind_reduce,
        "yield from tpz_array_reduce__co(",
        "737562",
        "Array.reduce post-spread source-binding rebind value carrier",
    );
    assert_generated_python_ok_int(
        &post_spread_source_binding_rebind_reduce,
        4,
        "post-spread source-binding rebind must preserve spread-origin callback slots",
    );

    let dynamic_index_after_spread_array = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/180_dynamic_index_after_spread_array.tpz"
    ));
    assert_dynamic_spread_array_map_gate(
        &dynamic_index_after_spread_array,
        "spread-origin same-target callback carrier",
    );

    let dynamic_spread_origin_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/181_dynamic_spread_origin_value.tpz"
    ));
    assert_dynamic_spread_array_map_gate(
        &dynamic_spread_origin_value,
        "spread-origin value carrier",
    );
    assert_generated_python_ok_int(
        &dynamic_spread_origin_value,
        234,
        "dynamic-index spread-built Array.map spread-origin value carrier",
    );

    let dynamic_spread_appended_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/182_dynamic_spread_appended_value.tpz"
    ));
    assert_dynamic_spread_array_map_gate(
        &dynamic_spread_appended_value,
        "appended-slot value carrier",
    );
    assert_generated_python_ok_int(
        &dynamic_spread_appended_value,
        246,
        "dynamic-index spread-built Array.map appended-slot value carrier",
    );

    let out_of_range_spread_array = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/183_out_of_range_spread_array.tpz"
    ));
    assert_dynamic_spread_array_map_gate(
        &out_of_range_spread_array,
        "dynamic out-of-range carrier",
    );

    let dynamic_spread_filter_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/184_dynamic_spread_filter_value.tpz"
    ));
    assert_dynamic_spread_array_gate(
        &dynamic_spread_filter_value,
        "yield from tpz_array_filter__co(",
        &["616c6c", "6b656570", "6f6e6c7954776f"],
        "Array.filter value carrier",
    );
    assert_generated_python_ok_int(
        &dynamic_spread_filter_value,
        7,
        "dynamic-index spread-built Array.filter value carrier",
    );

    let dynamic_spread_reduce_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/185_dynamic_spread_reduce_value.tpz"
    ));
    assert_dynamic_spread_array_gate(
        &dynamic_spread_reduce_value,
        "yield from tpz_array_reduce__co(",
        &["616464", "737562", "6d756c"],
        "Array.reduce value carrier",
    );
    assert_generated_python_ok_int(
        &dynamic_spread_reduce_value,
        60,
        "dynamic-index spread-built Array.reduce value carrier",
    );

    let dynamic_spread_sorted_by_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/186_dynamic_spread_sorted_by_value.tpz"
    ));
    assert_dynamic_spread_array_gate(
        &dynamic_spread_sorted_by_value,
        "yield from tpz_array_sorted_by__co(",
        &["69644b6579", "6d69644b6579", "6e65674b6579"],
        "Array.sortedBy value carrier",
    );
    assert_generated_python_ok_int(
        &dynamic_spread_sorted_by_value,
        231,
        "dynamic-index spread-built Array.sortedBy value carrier",
    );

    let dynamic_spread_sort_by_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/187_dynamic_spread_sort_by_value.tpz"
    ));
    assert_dynamic_spread_array_gate(
        &dynamic_spread_sort_by_value,
        "yield from tpz_array_sort_by__co(",
        &["69644b6579", "6d69644b6579", "6e65674b6579"],
        "Array.sortBy value carrier",
    );
    assert_generated_python_ok_int(
        &dynamic_spread_sort_by_value,
        321,
        "dynamic-index spread-built Array.sortBy value carrier",
    );

    let dynamic_spread_retain_value = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/188_dynamic_spread_retain_value.tpz"
    ));
    assert_dynamic_spread_array_gate(
        &dynamic_spread_retain_value,
        "yield from tpz_array_retain__co(",
        &["616c6c", "6b656570", "6f6e6c7954776f"],
        "Array.retain value carrier",
    );
    assert_generated_python_ok_int(
        &dynamic_spread_retain_value,
        7,
        "dynamic-index spread-built Array.retain value carrier",
    );

    let mixed_array = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/189_mixed_array.tpz"
    ));
    assert!(
        mixed_array.contains("yield from tpz_array_map__co(")
            && !mixed_array.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "mixed callback array slots without cooperative metadata should use the runtime driver without static callback recovery: {mixed_array}"
    );
    assert_generated_python_gates(&mixed_array)
        .unwrap_or_else(|e| panic!("mixed callback array Python gate failed: {e}"));

    let out_of_range = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/190_out_of_range.tpz"
    ));
    assert!(
        out_of_range.contains("yield from tpz_array_map__co(")
            && !out_of_range.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "out-of-range static callback array indices should use the runtime driver without static callback recovery: {out_of_range}"
    );
    assert_generated_python_gates(&out_of_range)
        .unwrap_or_else(|e| panic!("out-of-range callback array index Python gate failed: {e}"));

    let nested_index = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/191_nested_index.tpz"
    ));
    assert!(
        nested_index.contains("yield from tpz_array_map__co(")
            && !nested_index.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "nested callback array index chains should use the runtime driver without static callback recovery: {nested_index}"
    );
    assert_generated_python_gates(&nested_index)
        .unwrap_or_else(|e| panic!("nested callback array index Python gate failed: {e}"));

    let non_callback_field = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/192_non_callback_field.tpz"
    ));
    assert!(
        non_callback_field.contains("yield from tpz_array_map__co(")
            && !non_callback_field.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "non-callback record fields should use the runtime driver without sibling field metadata: {non_callback_field}"
    );
    assert_generated_python_gates(&non_callback_field)
        .unwrap_or_else(|e| panic!("non-callback record field Python gate failed: {e}"));

    let mutable_record_after_dynamic_field_assignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/193_mutable_record_after_dynamic_field_assignment.tpz"
    ));
    assert!(
        mutable_record_after_dynamic_field_assignment.contains("yield from tpz_array_map__co(")
            && !mutable_record_after_dynamic_field_assignment
                .contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "mutable callback records touched through dynamic fields should use the runtime driver without stale static metadata: {mutable_record_after_dynamic_field_assignment}"
    );
    assert_generated_python_gates(&mutable_record_after_dynamic_field_assignment).unwrap_or_else(
        |e| panic!("dynamic-assigned mutable callback record Python gate failed: {e}"),
    );

    let mutable_inner_record_after_dynamic_field_assignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/194_mutable_inner_record_after_dynamic_field_assignment.tpz"
    ));
    assert!(
        mutable_inner_record_after_dynamic_field_assignment
            .contains("yield from tpz_array_map__co(")
            && !mutable_inner_record_after_dynamic_field_assignment
                .contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "mutable inner callback records touched through dynamic fields before nesting should use the runtime driver without stale static metadata: {mutable_inner_record_after_dynamic_field_assignment}"
    );
    assert_generated_python_gates(&mutable_inner_record_after_dynamic_field_assignment)
        .unwrap_or_else(|e| {
            panic!("dynamic-assigned mutable inner callback record Python gate failed: {e}")
        });

    let mutable_record_after_plain_field_assignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/195_mutable_record_after_plain_field_assignment.tpz"
    ));
    assert!(
        mutable_record_after_plain_field_assignment.contains("yield from tpz_array_map__co(")
            && !mutable_record_after_plain_field_assignment
                .contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "mutable callback record fields reassigned to unproven callbacks should use the runtime driver without stale static metadata: {mutable_record_after_plain_field_assignment}"
    );
    assert_generated_python_gates(&mutable_record_after_plain_field_assignment).unwrap_or_else(
        |e| panic!("plain-assigned mutable callback record Python gate failed: {e}"),
    );

    let mutable_array_after_control_flow_slot_assignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/196_mutable_array_after_control_flow_slot_assignment.tpz"
    ));
    assert!(
        mutable_array_after_control_flow_slot_assignment.contains("yield from tpz_array_map__co(")
            && !mutable_array_after_control_flow_slot_assignment
                .contains("__call_cooperative__(__tpz_cb_0)"),
        "control-flow-contained static slot assignments must clear stale array callback metadata while keeping the runtime driver: {mutable_array_after_control_flow_slot_assignment}"
    );
    assert_generated_python_gates(&mutable_array_after_control_flow_slot_assignment)
        .unwrap_or_else(|e| panic!("control-flow static slot assignment Python gate failed: {e}"));

    let mutable_record_after_control_flow_field_assignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/197_mutable_record_after_control_flow_field_assignment.tpz"
    ));
    assert!(
        mutable_record_after_control_flow_field_assignment
            .contains("yield from tpz_array_map__co(")
            && !mutable_record_after_control_flow_field_assignment
                .contains("__call_cooperative__(__tpz_cb_0)"),
        "control-flow-contained record field assignments must clear stale record callback metadata while keeping the runtime driver: {mutable_record_after_control_flow_field_assignment}"
    );
    assert_generated_python_gates(&mutable_record_after_control_flow_field_assignment)
        .unwrap_or_else(|e| panic!("control-flow record field assignment Python gate failed: {e}"));

    let mutable_record_after_compound_assignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/198_mutable_record_after_compound_assignment.tpz"
    ));
    assert!(
        mutable_record_after_compound_assignment.contains("yield from tpz_array_map__co(")
            && !mutable_record_after_compound_assignment
                .contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "mutable callback record compound assignment should clear field metadata while keeping the runtime driver: {mutable_record_after_compound_assignment}"
    );
    assert_generated_python_gates(&mutable_record_after_compound_assignment).unwrap_or_else(|e| {
        panic!("compound-assigned mutable callback record Python gate failed: {e}")
    });

    let mutable_nested_record_after_ancestor_reassignment = emit_source(include_str!(
        "fixtures/concurrent_storage_callbacks/199_mutable_nested_record_after_ancestor_reassignment.tpz"
    ));
    assert!(
        mutable_nested_record_after_ancestor_reassignment.contains("yield from tpz_array_map__co(")
            && !mutable_nested_record_after_ancestor_reassignment
                .contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "mutable nested callback record fields should clear stale descendant metadata after ancestor reassignment while keeping the runtime driver: {mutable_nested_record_after_ancestor_reassignment}"
    );
    assert_generated_python_gates(&mutable_nested_record_after_ancestor_reassignment)
        .unwrap_or_else(|e| {
            panic!("mutable nested callback ancestor reassignment Python gate failed: {e}")
        });
}
