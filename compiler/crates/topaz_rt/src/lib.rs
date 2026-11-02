//! Runtime support for generated Topaz programs.
//!
//! Re-exports shared values and the product runtime, and provides cooperative
//! execution, call dispatch, captured cells and deferred actions.
//! There are no third-party dependencies; the workspace forbids unsafe code.

pub use topaz_product_runtime::{
    TARGET_ADAPTER_FACTS_SCHEMA, execute_product_export, execute_product_export_in_place,
    execute_product_export_in_place_with_facts, execute_product_export_in_place_with_host_facts,
    execute_product_program_with_facts, execute_product_program_with_facts_and_input,
    execute_product_program_with_host_facts_and_input,
};
pub use topaz_value::*;

mod executor;
pub use executor::{DeadlineExceeded, block_on, block_on_until};

mod closure;
pub use closure::{
    __native_enter_call, EmittedClosure, EmittedDefault, SpreadNamedCall, call_callback_hof,
    call_callback_map_hof, call_callback_map_update, call_callback_ok_or_else,
    call_callback_receiver_flat_map, call_callback_receiver_map, call_value, call_value_named,
    call_value_spread, call_value_spread_named, call_value_uncounted, callable_shape_matches,
    collect_callback_keys, collect_retained_items, native_array_len, native_index_bool,
    native_index_float, native_index_int, native_index_string, native_unbox_bool,
    native_unbox_float, native_unbox_int, native_unbox_string,
};

mod cell;
pub use cell::{cell_get, cell_new, cell_set};

mod concurrent;
pub use concurrent::{
    Checkpoint, checkpoint, concurrent_join, concurrent_join_timeout, depth_scoped,
};

mod defer;
pub use defer::{DeferStack, defer_push, defer_stack, run_defers};

mod top_cell;
pub use top_cell::{TopCell, top_cell, top_cell_get, top_cell_set, top_cell_value};
