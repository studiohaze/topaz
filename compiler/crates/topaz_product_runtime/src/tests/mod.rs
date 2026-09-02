//! Product-runtime tests for admission, execution, and export calls.
//! Compact-program builders remain test-only and feed the same decoder used by
//! installed compiler products.

use super::*;
use crate::diagnostic::*;
use crate::program::{decode_compact::*, model::*, validate::*};
use crate::runtime::{environment::*, machine::*, model::*};
use crate::wire::*;

mod admission;
mod execution;
mod export;

pub(super) fn compact_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn compact_string(bytes: &mut Vec<u8>, value: &str) {
    compact_u32(
        bytes,
        u32::try_from(value.len()).expect("test string length fits u32"),
    );
    bytes.extend_from_slice(value.as_bytes());
}

pub(super) fn compact_operation(bytes: &mut Vec<u8>, id: &str, kind: &str, operands: &[u32]) {
    for value in [id, "test", kind, "42", "", "", "", "", ""] {
        compact_string(bytes, value);
    }
    compact_u32(bytes, 0);
    compact_u32(bytes, 1);
    compact_u32(
        bytes,
        u32::try_from(operands.len()).expect("test operand count fits u32"),
    );
    for operand in operands {
        compact_u32(bytes, *operand);
    }
    compact_u32(
        bytes,
        u32::try_from(operands.len()).expect("test label count fits u32"),
    );
    for _ in operands {
        compact_string(bytes, "");
    }
}

pub(super) fn compact_program(operation_rows: &[(&str, &str, &[u32])]) -> Vec<u8> {
    let mut bytes = b"TPZC2BIN\x01".to_vec();
    compact_u32(
        &mut bytes,
        u32::try_from(operation_rows.len()).expect("test operation count fits u32"),
    );
    for (id, kind, operands) in operation_rows {
        compact_operation(&mut bytes, id, kind, operands);
    }
    compact_u32(&mut bytes, 0);
    bytes
}

pub(super) fn operation(kind: &str, detail: &str) -> Operation {
    Operation {
        id: format!("test:{kind}:{detail}"),
        module: "test".to_string(),
        lo: 0,
        hi: 1,
        kind: kind.to_string(),
        detail: detail.to_string(),
        operands: Vec::new(),
        operand_labels: Vec::new(),
        semantic_type: String::new(),
        pattern_type: None,
        reference_identity: String::new(),
        binding_name: String::new(),
        declaration_identity: String::new(),
        control_target: String::new(),
        call_target: String::new(),
        call_callee_kind: String::new(),
        call_method: String::new(),
        call_optional: false,
        call_shadow_first: false,
        call_stage_method: String::new(),
        call_arguments: Vec::new(),
        call_evaluations: Vec::new(),
    }
}
