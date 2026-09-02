//! Explicit Python multi-module fixture index. Payload discovery is intentionally not directory-based.

use crate::build_support::model::{FixtureFile, ModuleFixture};

macro_rules! module_fixture {
($name:literal, $entry:literal, [$($path:literal),+ $(,)?]) => {
module_fixture!($name, $name, $entry, [$($path),+])
};
($name:literal, $dir:literal, $entry:literal, [$($path:literal),+ $(,)?]) => {
ModuleFixture {
name: $name,
entry: $entry,
files: &[
$(FixtureFile {
path: $path,
source: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/python-modules/", $dir, "/", $path)),
source_path: concat!("fixtures/python-modules/", $dir, "/", $path),
}),+
],
}
};
}

pub(crate) const MODULE_FIXTURES: &[ModuleFixture] = &[
    module_fixture!(
        "module_init_boundary_fault",
        "main.tpz",
        ["lib.tpz", "main.tpz",]
    ),
    module_fixture!(
        "module_local_protocols_disjoint",
        "main.tpz",
        ["main.tpz", "left.tpz", "right.tpz",]
    ),
    module_fixture!(
        "module_imported_derived_show",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_exported_receiver_method_value_capture",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_exported_receiver_methods_disjoint",
        "main.tpz",
        ["main.tpz", "left.tpz", "right.tpz",]
    ),
    module_fixture!(
        "module_namespace_values",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_function",
        "main.tpz",
        ["main.tpz", "math.tpz",]
    ),
    module_fixture!(
        "module_namespace_function_named_call",
        "main.tpz",
        ["main.tpz", "math.tpz",]
    ),
    module_fixture!(
        "module_selected_function_alias",
        "main.tpz",
        ["main.tpz", "math.tpz",]
    ),
    module_fixture!(
        "module_function_named_calls",
        "main.tpz",
        ["main.tpz", "ops.tpz",]
    ),
    module_fixture!(
        "module_function_return_receiver_shapes",
        "main.tpz",
        ["main.tpz", "data.tpz",]
    ),
    module_fixture!(
        "module_transitive_namespace_function",
        "main.tpz",
        ["main.tpz", "util.tpz", "leaf.tpz",]
    ),
    module_fixture!(
        "module_transitive_selected_function_alias",
        "main.tpz",
        ["main.tpz", "util.tpz", "leaf.tpz",]
    ),
    module_fixture!(
        "module_selected_function_concurrent_helper_loop_fault_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_function_concurrent_helper_loop_fault_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_function_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_function_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_function_value_alias_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_function_value_alias_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_record_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_spread_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_empty_spread_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_spread_array_dynamic_index_named_default_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_spread_array_dynamic_index_named_default_function_value_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_spread_array_alias_dynamic_index_named_default_function_value_direct_call",
        "module_sel_imp_spread_arr_alias_dyn_idx_named_def_fn_val_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_spread_array_alias_dynamic_index_named_default_function_value_pipe_call",
        "module_sel_imp_spread_arr_alias_dyn_idx_named_def_fn_val_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_record_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_spread_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_empty_spread_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_record_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_spread_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_empty_spread_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_spread_array_dynamic_index_named_default_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_spread_array_dynamic_index_named_default_function_value_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_spread_array_alias_dynamic_index_named_default_function_value_direct_call",
        "module_ns_imp_spread_arr_alias_dyn_idx_named_def_fn_val_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_spread_array_alias_dynamic_index_named_default_function_value_pipe_call",
        "module_ns_imp_spread_arr_alias_dyn_idx_named_def_fn_val_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_record_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_spread_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_empty_spread_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_array_dynamic_index_named_default_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_origin_array_dynamic_index_named_default_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_origin_array_dynamic_index_named_default_function_value_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_record_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_spread_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_empty_spread_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_spread_array_dynamic_index_named_default_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_spread_array_dynamic_index_named_default_function_value_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_record_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_spread_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_spread_array_dynamic_index_array_map_hof_spread_origin_value",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_spread_array_dynamic_index_array_map_hof_appended_value",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_spread_array_dynamic_index_array_map_hof_out_of_range_fault",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_alias_empty_spread_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_manual_forwarded_selected_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "base.tpz", "facade.tpz",]
    ),
    module_fixture!(
        "module_manual_forwarded_namespace_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "base.tpz", "facade.tpz",]
    ),
    module_fixture!(
        "module_manual_forwarded_namespace_alias_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "base.tpz", "facade.tpz",]
    ),
    module_fixture!(
        "module_manual_forwarded_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "base.tpz", "selected_facade.tpz", "facade.tpz",]
    ),
    module_fixture!(
        "module_manual_forwarded_array_record_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "base.tpz", "facade.tpz",]
    ),
    module_fixture!(
        "module_manual_forwarded_array_record_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "base.tpz", "facade.tpz",]
    ),
    module_fixture!(
        "module_mutable_namespace_member_array_record_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mutable_namespace_member_array_record_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mutable_namespace_member_reserved_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mutable_namespace_member_reserved_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_member_single_function_alias_chain_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_namespace_member_single_function_alias_chain_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mixed_mutability_namespace_member_single_function_alias_chain_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mixed_mutability_namespace_member_single_function_alias_chain_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mutable_source_namespace_member_single_function_alias_chain_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mutable_source_namespace_member_single_function_alias_chain_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mutable_source_to_mutable_namespace_member_single_function_alias_chain_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mutable_source_to_mutable_namespace_member_single_function_alias_chain_hof_callback_yields",
        "module_mut_src_to_mut_ns_mem_one_fn_alias_chain_hof_cb_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_source_namespace_member_single_function_alias_snapshot_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_source_namespace_member_single_function_alias_snapshot_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_source_to_mutable_namespace_member_single_function_alias_snapshot_direct_call",
        "module_reassign_src_to_mut_ns_mem_one_fn_alias_snap_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_source_to_mutable_namespace_member_single_function_alias_snapshot_hof_callback_yields",
        "module_reassign_src_to_mut_ns_mem_one_fn_alias_snap_hof_cb_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_source_namespace_member_single_function_current_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_source_namespace_member_single_function_current_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_conditional_namespace_member_single_function_value_distinct_default_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_conditional_namespace_member_single_function_value_distinct_default_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_conditional_namespace_member_single_function_value_variadic_default_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_conditional_namespace_member_single_function_value_named_tail_variadic_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_statementful_conditional_namespace_member_single_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_statementful_conditional_namespace_member_single_function_value_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_conditional_namespace_member_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_conditional_namespace_member_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mismatched_conditional_namespace_member_array_function_value_hof_callback_value",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_statementful_conditional_namespace_member_array_function_value_hof_callback_value",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_mismatched_match_namespace_member_array_function_value_hof_callback_value",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_non_catch_all_match_namespace_member_array_function_value_hof_callback_value",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_statementful_mismatched_conditional_namespace_member_array_function_value_hof_callback_value",
        "module_stmtful_mismatch_cond_ns_mem_arr_fn_val_hof_cb_val",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_match_namespace_member_single_function_value_distinct_default_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_non_catch_all_match_namespace_member_single_function_value_distinct_default_direct_call",
        "module_non_catch_all_match_ns_mem_one_fn_val_dist_def_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_non_catch_all_match_namespace_member_single_function_value_positional_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_non_catch_all_match_namespace_member_single_function_value_distinct_default_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_non_catch_all_match_namespace_member_single_function_value_variadic_default_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_non_catch_all_match_namespace_member_single_function_value_named_tail_variadic_pipe_call",
        "module_non_catch_all_match_ns_mem_one_fn_val_named_tail_vararg_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_match_namespace_member_single_function_value_distinct_default_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_match_namespace_member_single_function_value_variadic_default_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_match_namespace_member_single_function_value_named_tail_variadic_pipe_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_match_namespace_member_array_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_match_namespace_member_array_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_mutable_storage_function_value_direct_call",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_reassigned_mutable_storage_function_value_hof_callback_yields",
        "main.tpz",
        ["main.tpz", "util.tpz",]
    ),
    module_fixture!(
        "module_init_effect_order_linear",
        "main.tpz",
        ["main.tpz", "a.tpz", "b.tpz", "c.tpz",]
    ),
    module_fixture!(
        "module_init_effect_once_diamond",
        "main.tpz",
        ["main.tpz", "b.tpz", "c.tpz", "d.tpz",]
    ),
    module_fixture!(
        "module_init_value_dependency",
        "main.tpz",
        ["main.tpz", "value.tpz", "base.tpz",]
    ),
    module_fixture!(
        "module_init_fault_direct",
        "main.tpz",
        ["main.tpz", "bad.tpz",]
    ),
    module_fixture!(
        "module_init_fault_transitive",
        "main.tpz",
        ["main.tpz", "a.tpz", "bad.tpz",]
    ),
    module_fixture!(
        "module_init_fault_after_partial_effects",
        "main.tpz",
        ["main.tpz", "bad.tpz",]
    ),
    module_fixture!(
        "module_init_fault_pending_defer_no_drain",
        "main.tpz",
        ["main.tpz", "bad.tpz",]
    ),
    module_fixture!(
        "module_init_guard_fault",
        "main.tpz",
        ["main.tpz", "bad.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_spread_pattern",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_pattern_alias",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_generic_nominal_record_spread",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_private_top_level_binding_or",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_map_key",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_union_map_key_identity",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_literal_default",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_reference_default",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_imported_const_default",
        "main.tpz",
        ["main.tpz", "model.tpz", "config.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_private_const_default",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_namespace_const_default",
        "main.tpz",
        ["main.tpz", "model.tpz", "config.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_namespace_runtime_default",
        "main.tpz",
        ["main.tpz", "model.tpz", "config.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_namespace_runtime_record_field_default",
        "main.tpz",
        ["main.tpz", "model.tpz", "config.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_namespace_private_runtime_default",
        "main.tpz",
        ["main.tpz", "model.tpz", "config.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_namespace_private_runtime_record_field_default",
        "main.tpz",
        ["main.tpz", "model.tpz", "config.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_runtime_value_default",
        "main.tpz",
        ["main.tpz", "model.tpz", "config.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_own_runtime_default",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_private_own_runtime_default",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_nominal_record_mutable_default_current_binding",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_nominal_record_private_runtime_default_preinit_fault",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_nominal_record_typed_patterns",
        "main.tpz",
        ["main.tpz", "alias_model.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_newtype_runtime_ops",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_generic_newtype_runtime_ops",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_newtype_same_source_id_collision",
        "main.tpz",
        ["main.tpz", "ids_a.tpz", "ids_b.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_enum_runtime_ops",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_namespace_imported_enum_runtime_ops",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_imported_generic_enum_runtime_ops",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_generic_nominal_record",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_generic_nominal_record_function_annotations",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_generic_nominal_record_typed_let",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_generic_nominal_record_typed_let_mismatch_fault",
        "main.tpz",
        ["main.tpz", "model.tpz",]
    ),
    module_fixture!(
        "module_selected_qualified_generic_nominal_typed_pattern",
        "main.tpz",
        ["main.tpz", "selected_model.tpz", "qualified_model.tpz",]
    ),
    module_fixture!(
        "module_local_alias_typed_json_decode",
        "main.tpz",
        ["main.tpz", "codec.tpz",]
    ),
    module_fixture!(
        "package_shape_server_contract",
        "src/main.tpz",
        [
            "src/main.tpz",
            "src/contract/request.tpz",
            "src/contract/response.tpz",
            "src/contract/handler.tpz",
        ]
    ),
];

pub(crate) const SERVER_CONTRACT_DEMO: ModuleFixture = module_fixture!(
    "pym23_server_contract_demo",
    "src/main.tpz",
    [
        "src/main.tpz",
        "src/contract/request.tpz",
        "src/contract/response.tpz",
        "src/contract/handler.tpz",
    ]
);
