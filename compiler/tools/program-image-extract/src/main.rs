use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use topaz_value::value::{JsonValue, json_parse};

mod manifest;

const PROGRAM_IMAGE_MAGIC: &[u8] = b"TPZIMAGE";
const PROGRAM_IMAGE_FORMAT_VERSION: u8 = 1;
const GENERATED_RUST_IR_PREFIX: &[u8] = b"pub const TOPAZ_COMPILER_IR_JSON: &str = r##\"";
const GENERATED_RUST_IR_SUFFIX: &[u8] = b"\"##;\npub const TOPAZ_COMPILER_RUNTIME_LEAVES";
const GENERATED_RUST_TARGET_FACTS_PREFIX: &[u8] =
    b"pub const TOPAZ_COMPILER_TARGET_FACTS_JSON: &str = r##\"";
const GENERATED_RUST_RAW_STRING_SUFFIX: &[u8] = b"\"##;";
const PAYLOAD_SCHEMA: &str = "topaz.compiler.fixed-point-ir-payload/v1";
const IR_SCHEMA: &str = "topaz.compiler.stage1-ir/v1";
const RUNTIME_TEMPLATE: &str = "compiler-ir-table/v2";
const TARGET_FACTS_SCHEMA: &str = "topaz.self-target-adapter-facts/v1";

struct Arguments {
    generated_rust: PathBuf,
    output_image: PathBuf,
    output_target_facts: PathBuf,
    output_manifest: Option<PathBuf>,
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut generated_rust = None;
    let mut output_image = None;
    let mut output_target_facts = None;
    let mut output_manifest = None;
    let mut arguments = arguments.into_iter();
    while let Some(option) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{} requires a value", option.to_string_lossy()))?;
        let slot = match option.to_str() {
            Some("--generated-rust") => &mut generated_rust,
            Some("--out-image") => &mut output_image,
            Some("--out-target-facts") => &mut output_target_facts,
            Some("--out-manifest") => &mut output_manifest,
            Some(other) => return Err(format!("unknown argument `{other}`")),
            None => {
                return Err(format!(
                    "argument is not UTF-8: {}",
                    option.to_string_lossy()
                ));
            }
        };
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(format!("duplicate {}", option.to_string_lossy()));
        }
    }
    Ok(Arguments {
        generated_rust: generated_rust.ok_or_else(|| "missing --generated-rust".to_string())?,
        output_image: output_image.ok_or_else(|| "missing --out-image".to_string())?,
        output_target_facts: output_target_facts
            .ok_or_else(|| "missing --out-target-facts".to_string())?,
        output_manifest,
    })
}

type JsonObject = BTreeMap<Rc<str>, JsonValue>;

fn object<'a>(value: &'a JsonValue, context: &str) -> Result<&'a JsonObject, String> {
    match value {
        JsonValue::Object(value) => Ok(value),
        _ => Err(format!("{context} is not an object")),
    }
}

fn array<'a>(value: &'a JsonValue, context: &str) -> Result<&'a [JsonValue], String> {
    match value {
        JsonValue::Array(value) => Ok(value),
        _ => Err(format!("{context} is not an array")),
    }
}

fn field<'a>(value: &'a JsonObject, name: &str, context: &str) -> Result<&'a JsonValue, String> {
    value
        .get(name)
        .ok_or_else(|| format!("{context} omitted `{name}`"))
}

fn string<'a>(value: &'a JsonObject, name: &str, context: &str) -> Result<&'a str, String> {
    match field(value, name, context)? {
        JsonValue::String(value) => Ok(value),
        _ => Err(format!("{context}.{name} is not a string")),
    }
}

fn boolean(value: &JsonObject, name: &str, context: &str) -> Result<bool, String> {
    match field(value, name, context)? {
        JsonValue::Bool(value) => Ok(*value),
        _ => Err(format!("{context}.{name} is not a boolean")),
    }
}

fn integer(value: &JsonObject, name: &str, context: &str) -> Result<u32, String> {
    match field(value, name, context)? {
        JsonValue::Number(value) => value
            .lexeme
            .parse()
            .map_err(|_| format!("{context}.{name} is not a u32")),
        _ => Err(format!("{context}.{name} is not a number")),
    }
}

fn strings<'a>(
    value: &'a JsonObject,
    name: &str,
    context: &str,
) -> Result<impl ExactSizeIterator<Item = &'a str>, String> {
    Ok(array(field(value, name, context)?, context)?
        .iter()
        .map(move |value| match value {
            JsonValue::String(value) => Ok(value.as_ref()),
            _ => Err(format!("{context}.{name} contains a non-string")),
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter())
}

fn require_string(
    value: &JsonObject,
    name: &str,
    expected: &str,
    context: &str,
) -> Result<(), String> {
    let observed = string(value, name, context)?;
    if observed == expected {
        Ok(())
    } else {
        Err(format!(
            "{context}.{name} is `{observed}`, expected `{expected}`"
        ))
    }
}

fn find(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start > haystack.len().saturating_sub(needle.len()) {
        return None;
    }
    let first = needle[0];
    haystack[start..]
        .windows(needle.len())
        .position(|window| window[0] == first && window == needle)
        .map(|offset| start + offset)
}

fn generated_constant<'a>(
    source: &'a [u8],
    prefix: &[u8],
    suffix: &[u8],
    label: &str,
) -> Result<&'a [u8], String> {
    let prefix_offset =
        find(source, prefix, 0).ok_or_else(|| format!("generated Rust omitted {label} prefix"))?;
    let value_offset = prefix_offset + prefix.len();
    let suffix_offset = find(source, suffix, value_offset)
        .ok_or_else(|| format!("generated Rust omitted {label} suffix"))?;
    Ok(&source[value_offset..suffix_offset])
}

fn write_u32(output: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "program image exceeds u32".to_string())?;
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_string(output: &mut Vec<u8>, value: &str) -> Result<(), String> {
    write_u32(output, value.len())?;
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn extract_program_image(generated_rust: &[u8]) -> Result<(Vec<u8>, Vec<u8>, String), String> {
    let payload_bytes = generated_constant(
        generated_rust,
        GENERATED_RUST_IR_PREFIX,
        GENERATED_RUST_IR_SUFFIX,
        "compiler IR",
    )?;
    let target_facts = generated_constant(
        generated_rust,
        GENERATED_RUST_TARGET_FACTS_PREFIX,
        GENERATED_RUST_RAW_STRING_SUFFIX,
        "compiler target facts",
    )?;
    let payload_text = std::str::from_utf8(payload_bytes)
        .map_err(|error| format!("compiler IR is not UTF-8: {error}"))?;
    let payload =
        json_parse(payload_text).map_err(|error| format!("compiler IR is not JSON: {error:?}"))?;
    let root = object(&payload, "compiler IR")?;
    require_string(root, "schema", PAYLOAD_SCHEMA, "compiler IR")?;
    require_string(root, "irSchema", IR_SCHEMA, "compiler IR")?;
    require_string(root, "runtimeTemplate", RUNTIME_TEMPLATE, "compiler IR")?;
    let source_set_id = string(root, "sourceSetId", "compiler IR")?.to_string();

    let target_facts_text = std::str::from_utf8(target_facts)
        .map_err(|error| format!("compiler target facts are not UTF-8: {error}"))?;
    let target_facts_json = json_parse(target_facts_text)
        .map_err(|error| format!("compiler target facts are not JSON: {error:?}"))?;
    let target_facts_root = object(&target_facts_json, "compiler target facts")?;
    require_string(
        target_facts_root,
        "schema",
        TARGET_FACTS_SCHEMA,
        "compiler target facts",
    )?;
    require_string(
        target_facts_root,
        "sourceSetId",
        &source_set_id,
        "compiler target facts",
    )?;

    let operation_rows = array(
        field(root, "loweredOperations", "compiler IR")?,
        "compiler IR loweredOperations",
    )?;
    let mut indexes = BTreeMap::new();
    for (index, row) in operation_rows.iter().enumerate() {
        let row = object(row, "compiler IR operation")?;
        let id = string(row, "id", "compiler IR operation")?;
        if indexes.insert(id, index).is_some() {
            return Err(format!("duplicate compiler IR operation id `{id}`"));
        }
    }

    let mut output = Vec::new();
    output.extend_from_slice(PROGRAM_IMAGE_MAGIC);
    output.push(PROGRAM_IMAGE_FORMAT_VERSION);
    write_u32(&mut output, operation_rows.len())?;
    for row in operation_rows {
        let row = object(row, "compiler IR operation")?;
        for field_name in [
            "id",
            "module",
            "kind",
            "detail",
            "referenceIdentity",
            "bindingName",
            "declarationIdentity",
            "callTarget",
            "callMethod",
        ] {
            write_string(
                &mut output,
                string(row, field_name, "compiler IR operation")?,
            )?;
        }
        output.extend_from_slice(&integer(row, "lo", "compiler IR operation")?.to_le_bytes());
        output.extend_from_slice(&integer(row, "hi", "compiler IR operation")?.to_le_bytes());
        let operands = strings(row, "operands", "compiler IR operation")?;
        write_u32(&mut output, operands.len())?;
        for operand in operands {
            let index = indexes
                .get(operand)
                .ok_or_else(|| format!("unknown compiler IR operand `{operand}`"))?;
            write_u32(&mut output, *index)?;
        }
        let labels = strings(row, "operandLabels", "compiler IR operation")?;
        write_u32(&mut output, labels.len())?;
        for label in labels {
            write_string(&mut output, label)?;
        }
    }

    let modules = array(
        field(root, "loweredModules", "compiler IR")?,
        "compiler IR loweredModules",
    )?;
    write_u32(&mut output, modules.len())?;
    for row in modules {
        let row = object(row, "compiler IR module")?;
        write_string(&mut output, string(row, "identity", "compiler IR module")?)?;
        output.push(u8::from(boolean(row, "entry", "compiler IR module")?));
        let operations = strings(row, "operationIds", "compiler IR module")?;
        write_u32(&mut output, operations.len())?;
        for operation in operations {
            let index = indexes
                .get(operation)
                .ok_or_else(|| format!("unknown module operation `{operation}`"))?;
            write_u32(&mut output, *index)?;
        }
    }
    Ok((output, target_facts.to_vec(), source_set_id))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let temporary = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("artifact"),
        std::process::id()
    ));
    fs::write(&temporary, bytes)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| {
        format!(
            "cannot replace {} with {}: {error}",
            path.display(),
            temporary.display()
        )
    })
}

fn sha256_identity(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut identity = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut identity, &digest);
    identity
}

fn main() -> Result<(), String> {
    let arguments = parse_arguments(std::env::args_os().skip(1))?;
    let generated_rust = fs::read(&arguments.generated_rust).map_err(|error| {
        format!(
            "cannot read generated Rust {}: {error}",
            arguments.generated_rust.display()
        )
    })?;
    let (program_image, target_facts, source_set_id) = extract_program_image(&generated_rust)?;
    write_atomic(&arguments.output_image, &program_image)?;
    write_atomic(&arguments.output_target_facts, &target_facts)?;
    if let Some(output_manifest) = &arguments.output_manifest {
        let summary = manifest::write_generated_artifacts_manifest(
            output_manifest,
            &generated_rust,
            &program_image,
            &target_facts,
            &source_set_id,
        )?;
        println!(
            "generated-artifacts-manifest: {} {} {}",
            source_set_id, summary.generated_rust_sha256, summary.program_image_sha256,
        );
    }
    println!(
        concat!(
            "program-image-extract: source-set {} generated-rust {} bytes {} ",
            "program-image {} bytes {} target-facts {} bytes {}"
        ),
        source_set_id,
        generated_rust.len(),
        sha256_identity(&generated_rust),
        program_image.len(),
        sha256_identity(&program_image),
        target_facts.len(),
        sha256_identity(&target_facts),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments, String> {
        parse_arguments(arguments.iter().map(OsString::from))
    }

    #[test]
    fn requires_one_path_for_each_artifact_boundary() {
        let arguments = parse(&[
            "--generated-rust",
            "fresh.rs",
            "--out-image",
            "fresh.bin",
            "--out-target-facts",
            "fresh.json",
        ])
        .expect("complete extraction arguments");
        assert_eq!(arguments.generated_rust, PathBuf::from("fresh.rs"));
        assert_eq!(arguments.output_image, PathBuf::from("fresh.bin"));
        assert_eq!(arguments.output_target_facts, PathBuf::from("fresh.json"));
        assert!(parse(&["--generated-rust", "fresh.rs", "--out-image", "fresh.bin",]).is_err());
    }

    #[test]
    fn rejects_duplicate_and_unknown_arguments() {
        assert!(
            parse(&[
                "--generated-rust",
                "fresh.rs",
                "--generated-rust",
                "other.rs",
                "--out-image",
                "fresh.bin",
                "--out-target-facts",
                "fresh.json",
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "--generated-rust",
                "fresh.rs",
                "--out-image",
                "fresh.bin",
                "--out-target-facts",
                "fresh.json",
                "--stage",
                "1",
            ])
            .is_err()
        );
    }
}
