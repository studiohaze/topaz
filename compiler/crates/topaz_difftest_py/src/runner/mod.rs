//! Execution boundary for differential cases and their two reference engines.
//! Case loading, CPython process control, and checked Rust execution remain
//! separate; the test suite consumes their shared results.

mod cases;
mod python;
mod reference;

#[cfg(test)]
pub(crate) use cases::{
    compare_python_trace, compiler_dir, emit_module_for_python_witness, load_cases, run_fixture,
    temp_dir, trace_file_string_map,
};
#[cfg(test)]
pub(crate) use python::{cpython_31314, hex_encode, run_python_batch, run_python_once_with_files};
