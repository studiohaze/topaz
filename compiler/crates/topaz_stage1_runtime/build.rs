use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/compiler_image.rs"]
mod compiler_image;

use compiler_image::{
    IR_SCHEMA, PROGRAM_IMAGE_HEADER, RUNTIME_TEMPLATE, decode_generated_artifacts_manifest,
    validate_target_facts,
};

const INPUT_CONFIGURATION: &str = "compiler-artifact-inputs.toml";
const INPUT_ENVIRONMENT: &[(&str, &str)] = &[
    ("TOPAZ_COMPILER_SOURCE_ROOT", "compiler_source_root"),
    (
        "TOPAZ_COMPILER_GENERATED_ARTIFACTS_MANIFEST",
        "generated_artifacts_manifest",
    ),
    ("TOPAZ_COMPILER_PROGRAM_IMAGE", "program_image"),
    ("TOPAZ_COMPILER_TARGET_FACTS", "target_facts"),
];

fn declared_inputs(manifest_dir: &Path) -> BTreeMap<String, String> {
    let configuration_path = manifest_dir.join(INPUT_CONFIGURATION);
    println!("cargo:rerun-if-changed={}", configuration_path.display());
    let text = fs::read_to_string(&configuration_path).unwrap_or_else(|error| {
        panic!(
            "cannot read explicit compiler artifact inputs {}: {error}",
            configuration_path.display()
        )
    });
    let mut values = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or_else(|| {
            panic!(
                "{}:{} is not a key/value compiler artifact input",
                configuration_path.display(),
                index + 1
            )
        });
        let key = key.trim();
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                panic!(
                    "{}:{} compiler artifact input is not a nonempty quoted string",
                    configuration_path.display(),
                    index + 1
                )
            });
        if values.insert(key.to_string(), value.to_string()).is_some() {
            panic!(
                "{}:{} duplicates compiler artifact input `{key}`",
                configuration_path.display(),
                index + 1
            );
        }
    }
    let mut expected = vec![
        "schema",
        "compiler_source_root",
        "generated_artifacts_manifest",
        "program_image",
        "target_facts",
    ];
    expected.sort_unstable();
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        expected,
        "explicit compiler artifact input fields drifted"
    );
    assert_eq!(
        values.get("schema").map(String::as_str),
        Some("topaz.compiler.artifact-inputs/v1"),
        "explicit compiler artifact input schema drifted"
    );
    values
}

fn input_paths(manifest_dir: &Path) -> Vec<PathBuf> {
    let overrides = INPUT_ENVIRONMENT
        .iter()
        .map(|(name, _)| {
            println!("cargo:rerun-if-env-changed={name}");
            env::var_os(name)
        })
        .collect::<Vec<Option<OsString>>>();
    let override_count = overrides.iter().filter(|value| value.is_some()).count();
    if override_count != 0 && override_count != INPUT_ENVIRONMENT.len() {
        panic!(
            "fresh compiler artifact inputs must explicitly override all four paths; {override_count} of {} were provided",
            INPUT_ENVIRONMENT.len()
        );
    }
    if override_count == INPUT_ENVIRONMENT.len() {
        return overrides
            .into_iter()
            .zip(INPUT_ENVIRONMENT)
            .map(|(value, (name, _))| {
                let value = value.expect("complete compiler artifact input override");
                if value.is_empty() {
                    panic!("{name} is empty");
                }
                PathBuf::from(value)
            })
            .collect();
    }
    let declared = declared_inputs(manifest_dir);
    INPUT_ENVIRONMENT
        .iter()
        .map(|(_, field)| manifest_dir.join(&declared[*field]))
        .collect()
}

fn sha256_identity(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut identity = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut identity, &digest);
    identity
}

fn require_bytes(actual: usize, expected: u64, label: &str) {
    let expected = usize::try_from(expected)
        .unwrap_or_else(|_| panic!("{label} manifest byte count exceeds usize"));
    assert_eq!(actual, expected, "{label} byte count drifted");
}

fn require_identity(bytes: &[u8], expected: &str, label: &str) {
    let actual = sha256_identity(bytes);
    assert_eq!(actual, expected, "{label} identity drifted");
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let inputs = input_paths(&manifest_dir);
    let [
        _source_root,
        manifest_path,
        program_image_path,
        target_facts_path,
    ] = inputs.try_into().expect("four compiler artifact inputs");
    for path in [&manifest_path, &program_image_path, &target_facts_path] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let manifest_bytes = fs::read(&manifest_path).unwrap_or_else(|error| {
        panic!(
            "cannot read generated artifacts manifest {}: {error}",
            manifest_path.display()
        )
    });
    let manifest = decode_generated_artifacts_manifest(&manifest_bytes)
        .unwrap_or_else(|error| panic!("{error}"));

    let program_image = fs::read(&program_image_path).unwrap_or_else(|error| {
        panic!(
            "cannot read compiler program image {}: {error}",
            program_image_path.display()
        )
    });
    require_bytes(
        program_image.len(),
        manifest.program_image_bytes,
        "compiler program image",
    );
    require_identity(
        &program_image,
        &manifest.program_image_sha256,
        "compiler program image",
    );
    assert!(
        program_image.starts_with(PROGRAM_IMAGE_HEADER),
        "compiler program image is not the stage-neutral TPZIMAGE format version 1"
    );

    let target_facts = fs::read(&target_facts_path).unwrap_or_else(|error| {
        panic!(
            "cannot read compiler target facts sidecar {}: {error}",
            target_facts_path.display()
        )
    });
    require_bytes(
        target_facts.len(),
        manifest.target_facts_bytes,
        "compiler target facts sidecar",
    );
    require_identity(
        &target_facts,
        &manifest.target_facts_sha256,
        "compiler target facts sidecar",
    );
    validate_target_facts(&target_facts, &manifest.source_set_id)
        .unwrap_or_else(|error| panic!("{error}"));

    let manifest_sha256 = sha256_identity(&manifest_bytes);
    let payload_sha256 = sha256_identity(&program_image[PROGRAM_IMAGE_HEADER.len()..]);
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(
        out_dir.join("topaz_compiler_program_image.bin"),
        &program_image,
    )
    .expect("write compiler program image for rustc embedding");
    fs::write(
        out_dir.join("topaz_compiler_target_facts.json"),
        &target_facts,
    )
    .expect("write compiler target facts for rustc embedding");

    let descriptors = format!(
        concat!(
            "pub const GENERATED_RUST_BYTES: u64 = {};\n",
            "pub const GENERATED_RUST_SHA256: &str = {:?};\n",
            "pub const GENERATED_ARTIFACTS_MANIFEST_SHA256: &str = {:?};\n",
            "pub const COMPILATION_REQUEST_SHA256: &str = {:?};\n",
            "pub const PROGRAM_IMAGE_SHA256: &str = {:?};\n",
            "pub const PROGRAM_IMAGE_PAYLOAD_SHA256: &str = {:?};\n",
            "pub const TARGET_FACTS_SHA256: &str = {:?};\n",
            "pub const SOURCE_SET_ID: &str = {:?};\n",
            "pub const RUNTIME_TEMPLATE: &str = {:?};\n",
            "pub const IR_SCHEMA: &str = {:?};\n",
        ),
        manifest.generated_rust_bytes,
        manifest.generated_rust_sha256,
        manifest_sha256,
        manifest.compilation_request_sha256,
        manifest.program_image_sha256,
        payload_sha256,
        manifest.target_facts_sha256,
        manifest.source_set_id,
        RUNTIME_TEMPLATE,
        IR_SCHEMA,
    );
    fs::write(out_dir.join("compiler_image_descriptors.rs"), descriptors)
        .expect("write compiler image descriptors");
}
