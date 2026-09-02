use crate::*;

pub(super) fn render_web_types(
    surface: &topaz_check::ModuleExports,
    records: &ExportedRecords,
    enums: &ExportedEnums,
    newtypes: &ExportedNewtypes,
) -> String {
    let mut out = format!(
        "export const TOPAZ_TOOLCHAIN_VERSION: {:?};\n",
        env!("CARGO_PKG_VERSION")
    );
    out.push_str(
        r#"export type TopazInt = { $: "int"; value: string };
export type TopazBool = { $: "bool"; value: boolean };
export type TopazString = { $: "string"; value: string };
export type TopazUnit = { $: "unit" };
export type TopazNull = { $: "null" };
export type TopazNone = { $: "none" };
export type TopazSome<T extends TopazAbiValue = TopazAbiValue> = { $: "some"; value: T };
export type TopazOption<T extends TopazAbiValue = TopazAbiValue> = TopazNone | TopazSome<T>;
export type TopazOk<T extends TopazAbiValue = TopazAbiValue> = { $: "ok"; value: T };
export type TopazErr<E extends TopazAbiValue = TopazAbiValue> = { $: "err"; value: E };
export type TopazResult<T extends TopazAbiValue = TopazAbiValue, E extends TopazAbiValue = TopazAbiValue> =
  | TopazOk<T>
  | TopazErr<E>;
export type TopazArray<T extends TopazAbiValue = TopazAbiValue> = { $: "array"; items: T[] };
export type TopazRecord<F extends Record<string, TopazAbiValue> = Record<string, TopazAbiValue>> = {
  $: "record";
  fields: F;
};
export type TopazNominalRecord<
  Id extends string = string,
  F extends Record<string, TopazAbiValue> = Record<string, TopazAbiValue>,
> = {
  $: "nominal-record";
  id: Id;
  fields: { [K in keyof F]: { name: K; value: F[K] } }[keyof F][];
};
export type TopazEnum<
  Id extends string = string,
  Variant extends string = string,
  Payloads extends TopazAbiValue[] = TopazAbiValue[],
> = {
  $: "enum";
  id: Id;
  variant: Variant;
  index: string;
  payloads: Payloads;
};
export type TopazNewtype<Id extends string = string, T extends TopazAbiValue = TopazAbiValue> = {
  $: "newtype";
  id: Id;
  value: T;
};
export type TopazBytes = { $: "bytes"; hex: string };
export type TopazJson = { $: "json"; value: unknown };
export type TopazUnsupported<Why extends string> = never;

export type TopazAbiValue =
  | TopazInt
  | TopazBool
  | TopazString
  | TopazUnit
  | TopazNull
  | TopazNone
  | TopazSome
  | TopazOk
  | TopazErr
  | TopazArray
  | TopazRecord
  | TopazNominalRecord
  | TopazEnum
  | TopazNewtype
  | TopazBytes
  | TopazJson;

export type TopazOutcome<T extends TopazAbiValue = TopazAbiValue> =
  | { status: "ok"; value: T }
  | { status: "fault"; code: string; message: string; span: { file: number; lo: number; hi: number } }
  | { status: "error"; message: string };

export interface TopazTrace<T extends TopazAbiValue = TopazAbiValue> {
  outcome: TopazOutcome<T>;
  stdout: string[];
  deferErrors: string[];
}

export interface TopazExports {
"#,
    );

    render_web_export_methods(&mut out, surface, records, enums, newtypes, false);
    out.push_str(
        r#"}

export interface TopazWorkerExports {
"#,
    );
    render_web_export_methods(&mut out, surface, records, enums, newtypes, true);
    out.push_str(
        r#"}

export interface TopazWebModule {
  instance: WebAssembly.Instance;
  exportNames: string[];
  exports: TopazExports;
  callExportJson(name: string, argsJson?: string): string;
  callExport<T extends TopazAbiValue = TopazAbiValue>(name: string, args?: TopazAbiValue[]): TopazOutcome<T>;
  callExportTraceJson(name: string, argsJson?: string, input?: string): string;
  callExportTrace<T extends TopazAbiValue = TopazAbiValue>(
    name: string,
    args?: TopazAbiValue[],
    input?: string,
  ): TopazTrace<T>;
}

export function instantiateTopaz(
  source: WebAssembly.Module | BufferSource | Response | string | URL,
  imports?: WebAssembly.Imports,
): Promise<TopazWebModule>;

export interface TopazWorkerOptions {
  wasm?: WebAssembly.Module | BufferSource | string | URL;
  workerOptions?: WorkerOptions;
}

export interface TopazWorkerModule {
  worker: Worker;
  ready: Promise<string[]>;
  readonly exportNames: string[];
  exports: TopazWorkerExports;
  callExportJson(name: string, argsJson?: string): Promise<string>;
  callExport<T extends TopazAbiValue = TopazAbiValue>(
    name: string,
    args?: TopazAbiValue[],
  ): Promise<TopazOutcome<T>>;
  callExportTraceJson(name: string, argsJson?: string, input?: string): Promise<string>;
  callExportTrace<T extends TopazAbiValue = TopazAbiValue>(
    name: string,
    args?: TopazAbiValue[],
    input?: string,
  ): Promise<TopazTrace<T>>;
  terminate(): void;
}

export function createTopazWorker(
  workerOrUrl?: Worker | string | URL,
  options?: TopazWorkerOptions,
): TopazWorkerModule;
"#,
    );
    out
}

pub(super) fn render_web_export_methods(
    out: &mut String,
    surface: &topaz_check::ModuleExports,
    records: &ExportedRecords,
    enums: &ExportedEnums,
    newtypes: &ExportedNewtypes,
    promise: bool,
) {
    let mut values: Vec<_> = surface.values.iter().collect();
    values.sort_by_key(|(name, _)| *name);
    for (name, value) in values {
        let topaz_check::Type::Func {
            params,
            variadic,
            ret,
        } = &value.ty
        else {
            continue;
        };
        let scoped_records = scoped_exported_records(records, value);
        let scoped_enums = scoped_exported_enums(enums, value);
        let scoped_newtypes = scoped_exported_newtypes(newtypes, value);
        out.push_str("  ");
        push_ts_string_literal(out, name);
        out.push('(');
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&ts_param_name(value, i));
            if i >= value.required {
                out.push('?');
            }
            out.push_str(": ");
            out.push_str(&ts_abi_type(
                param,
                &scoped_records,
                &scoped_enums,
                &scoped_newtypes,
            ));
        }
        if let Some(variadic) = variadic {
            if !params.is_empty() {
                out.push_str(", ");
            }
            out.push_str("...rest: ");
            out.push_str(&ts_abi_type(
                variadic,
                &scoped_records,
                &scoped_enums,
                &scoped_newtypes,
            ));
            out.push_str("[]");
        }
        if promise {
            out.push_str("): Promise<TopazOutcome<");
        } else {
            out.push_str("): TopazOutcome<");
        }
        out.push_str(&ts_abi_type(
            ret,
            &scoped_records,
            &scoped_enums,
            &scoped_newtypes,
        ));
        if promise {
            out.push_str(">>;\n");
        } else {
            out.push_str(">;\n");
        }
    }
}

pub(super) fn scoped_exported_records(
    records: &ExportedRecords,
    value: &topaz_check::ExportedValue,
) -> ExportedRecords {
    let mut scoped = records.clone();
    for (id, record) in &value.nominals.records {
        scoped.insert(id.clone(), record.clone());
    }
    scoped
}

pub(super) fn scoped_exported_enums(
    enums: &ExportedEnums,
    value: &topaz_check::ExportedValue,
) -> ExportedEnums {
    let mut scoped = enums.clone();
    for (id, enm) in &value.nominals.enums {
        scoped.insert(id.clone(), enm.clone());
    }
    scoped
}

pub(super) fn scoped_exported_newtypes(
    newtypes: &ExportedNewtypes,
    value: &topaz_check::ExportedValue,
) -> ExportedNewtypes {
    let mut scoped = newtypes.clone();
    for (id, newtype) in &value.nominals.newtypes {
        scoped.insert(id.clone(), newtype.clone());
    }
    scoped
}

pub(super) fn ts_param_name(value: &topaz_check::ExportedValue, index: usize) -> String {
    value
        .names
        .get(index)
        .filter(|name| is_ts_identifier(name))
        .cloned()
        .unwrap_or_else(|| format!("arg{index}"))
}

pub(super) fn is_ts_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
}

pub(super) fn ts_abi_type(
    ty: &topaz_check::Type,
    records: &ExportedRecords,
    enums: &ExportedEnums,
    newtypes: &ExportedNewtypes,
) -> String {
    let mut seen = Vec::new();
    ts_abi_type_with_seen(ty, records, enums, newtypes, &mut seen)
}

pub(super) fn ts_abi_type_with_seen(
    ty: &topaz_check::Type,
    records: &ExportedRecords,
    enums: &ExportedEnums,
    newtypes: &ExportedNewtypes,
    seen: &mut Vec<String>,
) -> String {
    use topaz_check::{Ctor, Lit, Prim, Type};
    match ty {
        Type::Prim(Prim::Int) | Type::Literal(Lit::Int(_)) => "TopazInt".to_string(),
        Type::Prim(Prim::String) | Type::Literal(Lit::Str(_)) => "TopazString".to_string(),
        Type::Prim(Prim::Bool) | Type::Literal(Lit::Bool(_)) => "TopazBool".to_string(),
        Type::Prim(Prim::Unit) => "TopazUnit".to_string(),
        Type::Literal(Lit::Null) => "TopazNull".to_string(),
        Type::Prim(Prim::Float) | Type::Literal(Lit::Float(_)) => {
            "TopazUnsupported<\"float\">".to_string()
        }
        Type::Union(members) => members
            .iter()
            .map(|m| ts_abi_type_with_seen(m, records, enums, newtypes, seen))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Record(fields) => {
            let mut out = String::from("TopazRecord<{ ");
            for (i, (name, field_ty)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str("; ");
                }
                push_ts_string_literal(&mut out, name);
                out.push_str(": ");
                out.push_str(&ts_abi_type_with_seen(
                    field_ty, records, enums, newtypes, seen,
                ));
            }
            out.push_str(" }>");
            out
        }
        Type::Ctor(Ctor::Array, args) => {
            format!(
                "TopazArray<{}>",
                ts_abi_type_with_seen(&args[0], records, enums, newtypes, seen)
            )
        }
        Type::Ctor(Ctor::Option, args) => {
            format!(
                "TopazOption<{}>",
                ts_abi_type_with_seen(&args[0], records, enums, newtypes, seen)
            )
        }
        Type::Ctor(Ctor::Result, args) => format!(
            "TopazResult<{}, {}>",
            ts_abi_type_with_seen(&args[0], records, enums, newtypes, seen),
            ts_abi_type_with_seen(&args[1], records, enums, newtypes, seen)
        ),
        Type::Ctor(Ctor::Map, _) => "TopazUnsupported<\"Map\">".to_string(),
        Type::Ctor(Ctor::Set, _) => "TopazUnsupported<\"Set\">".to_string(),
        Type::Ctor(Ctor::Range, _) => "TopazUnsupported<\"range\">".to_string(),
        Type::Func { .. } => "TopazUnsupported<\"function\">".to_string(),
        Type::Foreign { name, .. } => {
            format!("TopazUnsupported<{}>", ts_type_string_arg(name))
        }
        Type::Skolem { .. } | Type::Unknown | Type::Var(_) => "TopazAbiValue".to_string(),
        Type::Template => "TopazUnsupported<\"template\">".to_string(),
        Type::File => "TopazUnsupported<\"File\">".to_string(),
        Type::JsonValue => "TopazJson".to_string(),
        Type::Bytes => "TopazBytes".to_string(),
        Type::ByteBuffer => "TopazUnsupported<\"ByteBuffer\">".to_string(),
        Type::Path => "TopazUnsupported<\"Path\">".to_string(),
        Type::Regex => "TopazUnsupported<\"Regex\">".to_string(),
        Type::Match => "TopazUnsupported<\"Match\">".to_string(),
        Type::TomlValue => "TopazUnsupported<\"TOMLValue\">".to_string(),
        Type::Url => "TopazUnsupported<\"URL\">".to_string(),
        Type::Date => "TopazUnsupported<\"Date\">".to_string(),
        Type::BigInt => "TopazUnsupported<\"BigInt\">".to_string(),
        Type::Decimal => "TopazUnsupported<\"Decimal\">".to_string(),
        Type::RoundingMode => "TopazEnum<\"RoundingMode\">".to_string(),
        Type::Enum { .. } => {
            let id = ty.to_string();
            ts_enum_type(&id, records, enums, newtypes, seen)
        }
        Type::NominalRecord { .. } => {
            let id = ty.to_string();
            ts_nominal_record_type(&id, records, enums, newtypes, seen)
        }
        Type::Newtype { .. } => {
            let id = ty.to_string();
            ts_newtype_type(&id, records, enums, newtypes, seen)
        }
    }
}

pub(super) fn ts_nominal_record_type(
    id: &str,
    records: &ExportedRecords,
    enums: &ExportedEnums,
    newtypes: &ExportedNewtypes,
    seen: &mut Vec<String>,
) -> String {
    if seen.iter().any(|seen_id| seen_id == id) {
        return format!("TopazNominalRecord<{}>", ts_type_string_arg(id));
    }
    let Some(record) = records.get(id) else {
        return format!("TopazNominalRecord<{}>", ts_type_string_arg(id));
    };
    seen.push(id.to_string());
    let mut out = format!("TopazNominalRecord<{}, {{ ", ts_type_string_arg(&record.id));
    for (i, field) in record.fields.iter().enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        push_ts_string_literal(&mut out, &field.name);
        out.push_str(": ");
        out.push_str(&ts_abi_type_with_seen(
            &field.ty, records, enums, newtypes, seen,
        ));
    }
    out.push_str(" }>");
    seen.pop();
    out
}

pub(super) fn ts_enum_type(
    id: &str,
    records: &ExportedRecords,
    enums: &ExportedEnums,
    newtypes: &ExportedNewtypes,
    seen: &mut Vec<String>,
) -> String {
    if seen.iter().any(|seen_id| seen_id == id) {
        return format!("TopazEnum<{}>", ts_type_string_arg(id));
    }
    let Some(enm) = enums.get(id) else {
        return format!("TopazEnum<{}>", ts_type_string_arg(id));
    };
    if enm.variants.is_empty() {
        return format!("TopazEnum<{}>", ts_type_string_arg(&enm.id));
    }
    seen.push(id.to_string());
    let mut out = String::new();
    for (i, variant) in enm.variants.iter().enumerate() {
        if i > 0 {
            out.push_str(" | ");
        }
        out.push_str("TopazEnum<");
        out.push_str(&ts_type_string_arg(&enm.id));
        out.push_str(", ");
        out.push_str(&ts_type_string_arg(&variant.name));
        out.push_str(", [");
        for (j, payload) in variant.payloads.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            out.push_str(&ts_abi_type_with_seen(
                payload, records, enums, newtypes, seen,
            ));
        }
        out.push_str("]>");
    }
    seen.pop();
    out
}

pub(super) fn ts_newtype_type(
    id: &str,
    records: &ExportedRecords,
    enums: &ExportedEnums,
    newtypes: &ExportedNewtypes,
    seen: &mut Vec<String>,
) -> String {
    if seen.iter().any(|seen_id| seen_id == id) {
        return format!("TopazNewtype<{}>", ts_type_string_arg(id));
    }
    let Some(newtype) = newtypes.get(id) else {
        return format!("TopazNewtype<{}>", ts_type_string_arg(id));
    };
    seen.push(id.to_string());
    let base = ts_abi_type_with_seen(&newtype.base, records, enums, newtypes, seen);
    seen.pop();
    format!(
        "TopazNewtype<{}, {}>",
        ts_type_string_arg(&newtype.id),
        base
    )
}

pub(super) fn ts_type_string_arg(raw: &str) -> String {
    let mut out = String::new();
    push_ts_string_literal(&mut out, raw);
    out
}

pub(super) fn push_ts_string_literal(out: &mut String, raw: &str) {
    push_json_string(out, raw);
}
