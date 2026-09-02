//! The shared value core (CDR-006 §3) used by both execution engines. It
//! provides runtime-stop identity, transcript comparison, the §2 data model,
//! and the callable and template ABI.
//!
//! Zero third-party dependencies; no panics on any program-reachable
//! path — a panic here is a bug in either engine equally.

pub mod lispex_application;
pub mod transcript;
pub mod value;

pub use lispex_application::{
    LispexApplicationOperation, LispexApplicationRequest, LispexApplicationResponse,
    LispexApplicationRuleIdentity, LispexApplicationSettlement,
    LispexApplicationSettlementCategory, LispexConsumerArtifactInspection, LispexEvaluationLimits,
    builtin_lispex_application, project_lispex_application_host_value,
};

pub use value::SchemaDeclLookup;
pub use value::{
    ArrayStore, Builtin, CALL_DEPTH_LIMIT, CANONICAL_ARITHMETIC_NAN_BITS, CallDepthGuard,
    CallFuture, CallbackHofExecution, CallbackHofKind, CallbackHofPending, CallbackHofStep,
    CallbackKeyCollection, CallbackKeyPending, CallbackKeyStep, CallbackMapHofExecution,
    CallbackMapHofKind, CallbackMapHofPending, CallbackMapHofStep, CallbackMapUpdatePending,
    CallbackMapUpdateStep, CallbackOkOrElsePending, CallbackOkOrElseStep, CallbackReceiverMapKind,
    CallbackReceiverMapPending, CallbackReceiverMapStep, CallbackRetainExecution,
    CallbackRetainPending, CallbackRetainStep, CmpError, ExternFunction, ExternReplayStore,
    FLOAT_RENDER_GOLDENS, FloatRenderGolden, Host, HostDirEntry, JsonNumber, JsonParseError,
    JsonValue, Key, OrderedMap, OrderedSet, ReceiverBuiltin, ReceiverBuiltinNameShape,
    ReceiverBuiltinRoute, ResourceId, RoundingMode, RtCx, STRUCT_DEPTH, STRUCT_FUEL, Schema,
    SchemaAliasDecl, SchemaDecls, SchemaEnumDecl, SchemaNewtypeDecl, SchemaRecordDecl,
    TemplateData, TomlValue, TpzCall, TpzTemplate, Value, array_spread_extend, binary_value,
    bind_builtin_named_args, bind_named_arg_slots, bind_receiver_builtin, builtin_bigint_div,
    builtin_bigint_from_int, builtin_bigint_mod, builtin_bigint_parse, builtin_bigint_to_int,
    builtin_bigint_to_string, builtin_byte_buffer_allocate, builtin_byte_buffer_copy,
    builtin_byte_buffer_copy_i64, builtin_byte_buffer_fill, builtin_byte_buffer_fill_i64,
    builtin_byte_buffer_from_bytes, builtin_byte_buffer_get, builtin_byte_buffer_get_i64,
    builtin_byte_buffer_get_raw_i64, builtin_byte_buffer_length, builtin_byte_buffer_length_i64,
    builtin_byte_buffer_set, builtin_byte_buffer_set_i64, builtin_byte_buffer_to_bytes,
    builtin_byte_buffer_to_bytes_ref, builtin_bytes_concat, builtin_bytes_decode_utf8,
    builtin_bytes_empty, builtin_bytes_encode_utf8, builtin_bytes_from_array,
    builtin_bytes_from_base64, builtin_bytes_from_hex, builtin_bytes_get, builtin_bytes_get_i64,
    builtin_bytes_is_empty, builtin_bytes_length, builtin_bytes_length_i64, builtin_bytes_slice,
    builtin_bytes_slice_i64, builtin_bytes_to_array, builtin_bytes_to_base64, builtin_bytes_to_hex,
    builtin_cli_has_flag, builtin_cli_option, builtin_cli_options, builtin_cli_positionals,
    builtin_codec_deflate_compress, builtin_codec_deflate_decompress,
    builtin_codec_deflate_fixed_compress, builtin_codec_gzip_compress,
    builtin_codec_gzip_decompress, builtin_codec_reed_solomon_255_223_protect,
    builtin_codec_zlib_fixed_compress, builtin_codec_zstd_compress, builtin_codec_zstd_decompress,
    builtin_csv_parse, builtin_csv_parse_with_header, builtin_csv_stringify,
    builtin_csv_stringify_with_header, builtin_date_add_days, builtin_date_day,
    builtin_date_from_ymd, builtin_date_month, builtin_date_parse_iso, builtin_date_to_iso,
    builtin_date_year, builtin_decimal_div, builtin_decimal_from_int, builtin_decimal_parse,
    builtin_decimal_round, builtin_decimal_scale, builtin_decimal_to_int,
    builtin_decimal_to_string, builtin_default_arg, builtin_extern_call, builtin_from_code_point,
    builtin_fs_list, builtin_fs_read_bytes, builtin_fs_read_text, builtin_fs_write_bytes,
    builtin_fs_write_text, builtin_hash_crc32, builtin_hash_hmac_sha256, builtin_hash_sha256,
    builtin_hash_sha512, builtin_input, builtin_json_decode, builtin_json_parse,
    builtin_json_parse_as, builtin_json_stringify, builtin_map_new, builtin_map_of,
    builtin_map_of_entries, builtin_math_abs, builtin_math_ceil, builtin_math_cos,
    builtin_math_floor, builtin_math_is_finite, builtin_math_is_nan, builtin_math_max,
    builtin_math_min, builtin_math_parse_float, builtin_math_round, builtin_math_sin,
    builtin_math_sqrt, builtin_math_tan, builtin_param_names, builtin_path_cwd_relative,
    builtin_path_extension, builtin_path_file_name, builtin_path_from, builtin_path_join,
    builtin_path_normalize, builtin_path_parent, builtin_path_project, builtin_path_to_string,
    builtin_path_with_extension, builtin_print, builtin_protocol_dispatch, builtin_regex_compile,
    builtin_regex_find, builtin_regex_find_all, builtin_regex_is_match, builtin_regex_replace_all,
    builtin_regex_split, builtin_set_of, builtin_test_assert, builtin_test_assert_contains,
    builtin_test_assert_eq, builtin_test_assert_err, builtin_test_assert_golden,
    builtin_test_assert_ne, builtin_test_assert_none, builtin_test_assert_ok,
    builtin_test_assert_some, builtin_test_dispatch, builtin_to_float, builtin_to_int,
    builtin_to_int_radix, builtin_toml_from_json, builtin_toml_parse, builtin_toml_stringify,
    builtin_toml_to_json, builtin_url_fragment, builtin_url_host, builtin_url_parse,
    builtin_url_path, builtin_url_query, builtin_url_scheme, builtin_url_to_string,
    bytes_to_hex_into, call_host_builtin, call_method, call_method_named, call_pure_builtin,
    call_resource_method, call_resource_method_named, call_spread_extend,
    canonical_abi_args_encode, canonical_abi_completed, canonical_abi_decode,
    canonical_abi_decode_args, canonical_abi_encode, canonical_abi_error, canonical_abi_faulted,
    canonical_key, case_guard_bool, check_member_method, cmp_guard, condition_bool, decode_escapes,
    exact_args, filter_keep, float_arith, float_cmp, for_items, index_slot, index_value, int_add,
    int_cmp, int_div, int_mul, int_neg, int_pow, int_rem, int_sub, iterable_items, json_parse,
    json_stringify, key_to_value, make_range, make_template, member_value, member_value_required,
    newtype_value_with_identity, no_member_fault, nominal_declaration_identity,
    nominal_record_field_required, nominal_spread_base, nominal_spread_base_required,
    nominal_spread_base_with_identity, optional_member, prepare_callback_hof,
    prepare_callback_key_collection, prepare_callback_map_hof, prepare_callback_map_update,
    prepare_callback_ok_or_else, prepare_callback_receiver_flat_map, prepare_callback_receiver_map,
    prepare_callback_receiver_map_kind, prepare_callback_retain, range_items, receiver_builtin,
    receiver_builtin_by_kind, receiver_builtin_name_shape, record_update_base, record_update_merge,
    recursion_fault, render, render_float, rounding_mode_value, rounding_mode_variant, schema_of,
    short_circuit_lhs, sorted_by_keys, toml_parse_document, try_value, unary_value,
    update_fields_value, url_value, values_compare, values_equal, walk_fields_value, wrap_optional,
    write_json_node,
};
pub use value::{ExternSandboxKind, ExternSandboxPolicy};
// The operator enums and span types are part of the surface EMITTED
// code names (it calls the shared `binary_value`/`unary_value` and
// threads `Span` for faults), so the value core re-exports them — both
// engines and the generated crate see one set of types.
pub use topaz_diag::{FileId, Span};
pub use topaz_syntax::{
    ast::{BinaryOp, UnaryOp},
    parse_duration_milliseconds,
};

/// A runtime stop: a §13a fault (TPZ4xxx) or a dynamic guard
/// (TPZ5xxx — the same identity the checker reports statically).
/// CDR-003 §4 keeps the classes strictly separate; CDR-006 §3 makes
/// the type itself shared so fault identity cannot drift between
/// engines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl RtError {
    pub fn is_fault(&self) -> bool {
        self.code.starts_with("TPZ4")
    }
}

/// The shared constructor: every runtime stop in either engine is
/// built here, so code/message identity is shared by construction.
pub fn fault(code: &'static str, message: impl Into<String>, span: Span) -> RtError {
    RtError {
        code,
        message: message.into(),
        span,
    }
}

/// The structured result of a run (CDR-006 §4): normal completion
/// carrying the program's final value, or a runtime stop. The
/// differential harness compares outcome identity as DATA — fault
/// code, message, and span — never by parsing output streams; the
/// native adapter is the boundary that renders a fault to stderr and
/// a process exit status.
#[derive(Debug, Clone)]
pub enum RunOutcome {
    Completed(value::Value),
    Faulted(RtError),
}

pub mod codes {
    //! TPZ4xxx: the closed §13a fault list (CDR-003 §4). TPZ5xxx:
    //! dynamic guards; the checker reports the same codes
    //! statically (CDR-004 §6).
    pub const FAULT_INDEX: &str = "TPZ4001";
    pub const FAULT_DIV_ZERO: &str = "TPZ4002";
    pub const FAULT_RANGE_STEP: &str = "TPZ4003";
    pub const FAULT_OVERFLOW: &str = "TPZ4004";
    pub const FAULT_NEG_EXPONENT: &str = "TPZ4005";
    pub const FAULT_MATCH_MISS: &str = "TPZ4006";
    pub const FAULT_ASSERT: &str = "TPZ4007";
    /// §6 (v5.4) a `map { … }` LITERAL with a DUPLICATE key (the same key value
    /// supplied twice). Distinct from `Map.insert`'s silent overwrite: a literal
    /// asserts its keys are unique, so a runtime duplicate FAULTS. Both engines
    /// build the literal through `builtin_map_of`, so the fault is byte-identical.
    pub const FAULT_MAP_DUP_KEY: &str = "TPZ4601";

    use topaz_diag::guard_codes;

    pub const GUARD_TYPE: &str = guard_codes::TYPE;
    pub const GUARD_UNBOUND: &str = guard_codes::UNBOUND;
    pub const GUARD_IMMUTABLE: &str = guard_codes::IMMUTABLE;
    pub const GUARD_ARITY: &str = guard_codes::ARITY;
    pub const GUARD_NOT_CALLABLE: &str = guard_codes::NOT_CALLABLE;
    pub const GUARD_NO_FIELD: &str = guard_codes::NO_FIELD;
    pub const GUARD_COMPARE: &str = guard_codes::COMPARE;
    pub const GUARD_REDECLARE: &str = guard_codes::REDECLARE;
    pub const GUARD_RECURSION: &str = guard_codes::RECURSION;
    pub const GUARD_UNIMPLEMENTED: &str = "TPZ5099";
}
