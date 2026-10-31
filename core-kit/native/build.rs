use std::env;
use std::fs;
use std::path::PathBuf;
use topaz_value::value::{bytes_to_hex_into, sha256};

fn main() {
    let target = env::var("TARGET").expect("Cargo supplies TARGET");
    println!("cargo:rustc-env=TOPAZ_CORE_KIT_TARGET={target}");
    println!("cargo:rerun-if-env-changed=TOPAZ_CORE_KIT_GENERATED_CLI");
    if env::var_os("CARGO_FEATURE_NATIVE_CLI").is_none() {
        return;
    }
    if target == "x86_64-pc-windows-msvc"
        && env::var("PROFILE").as_deref() == Ok("release")
        && env::var("DEBUG").as_deref() == Ok("false")
    {
        // rustc requests a PDB even for the stripped Kit. Temporary object paths
        // can change its GUID, which the linker also embeds in the executable.
        println!("cargo:rustc-link-arg-bin=topaz-core-kit=/INCREMENTAL:NO");
        println!("cargo:rustc-link-arg-bin=topaz-core-kit=/Brepro");
        println!("cargo:rustc-link-arg-bin=topaz-core-kit=/DEBUG:NONE");
    }
    let source = env::var_os("TOPAZ_CORE_KIT_GENERATED_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../out/core-kit-bootstrap/generated_cli.rs")
        });
    let source = source.canonicalize().unwrap_or_else(|error| {
        panic!(
            "CLI Rust is missing at {}: {error}. Run ./bootstrap or bootstrap.cmd first, or set TOPAZ_CORE_KIT_GENERATED_CLI to freshly generated CLI Rust.",
            source.display()
        )
    });
    println!("cargo:rerun-if-changed={}", source.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    // The caller's chosen output directory must not become the compiled
    // module's source identity. Both bootstrap and kit build stage identical
    // selected bytes at this stable Cargo path before compiling the module.
    let staged = output.join("generated_cli.rs");
    let bytes = fs::read(&source).expect("cannot read selected generated CLI Rust");
    let mut digest = String::with_capacity(64);
    bytes_to_hex_into(&mut digest, &sha256(&bytes));
    fs::write(&staged, bytes).expect("cannot stage selected generated CLI Rust");
    println!("cargo:rustc-env=TOPAZ_CORE_KIT_GENERATED_SHA256={digest}");
    let staged = staged.to_str().expect("staged CLI path must be UTF-8");
    // A module path preserves the emitter's inner attributes. include! of the
    // emitted source itself would reject those attributes. Suppress warnings
    // only inside generated code: its long source lines make incidental Rust
    // lints overwhelm build diagnostics, while handwritten adapters stay linted.
    fs::write(
        output.join("generated_cli_module.rs"),
        format!("#[allow(warnings)]\n#[path = {staged:?}]\nmod generated_cli;\n"),
    )
    .expect("cannot write generated CLI module wrapper");
}
