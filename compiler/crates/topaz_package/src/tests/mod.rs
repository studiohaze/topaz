//! Package tests divided along manifest, lock, vendor, and strict-I/O boundaries.
//! Shared documents and artifact bytes keep the feature leaves focused on their
//! observable package contracts.

mod io;
mod lock;
mod manifest;
mod vendor;

use crate::*;

pub(super) const HASH: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub(super) const HASH_B: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const HASH_C: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(super) const ARTIFACT_BYTES: &str = "host-math-artifact-v1\n";
pub(super) const ARTIFACT_HASH: &str =
    "sha256:39587311db2c06b2b2e2a038ddeefa717a74f4f991a023770a54002366d94d49";

pub(super) fn manifest_text() -> String {
    format!(
        r#"[package]
name = "user_tools"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"
license = "Apache-2.0"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
csv_tools = {{ version = "1.2.0" }}
local_schema = {{ path = "../schema", hash = "{HASH}" }}

[capabilities.fs]
read = ["./data", "./templates"]
write = ["./out"]

[exports]
module = "src/lib.tpz"
"#
    )
}

pub(super) fn temp_root(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "topaz-package-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temp root");
    root
}

pub(super) fn write_file(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent directories");
    }
    std::fs::write(path, text).expect("write file");
}

pub(super) fn extern_lock_manifest() -> String {
    format!(
        r#"[package]
name = "extern_lock"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"

[extern.host.math]
hash = "{ARTIFACT_HASH}"
abi_hash = "{HASH_B}"

[[extern.host.math.functions]]
name = "twice"
params = ["int"]
result = "int"

[extern.host.math.artifact]
path = "artifacts/host-math.wasm"

[extern.host.math.sandbox]
kind = "wasm"
fuel = 1000
memory_bytes = 65536

[extern.host.math.replay]
fixture = "replay/host-math.jsonl"
"#
    )
}

pub(super) fn write_extern_lock_package(root: &Path) {
    write_file(root, "topaz.toml", &extern_lock_manifest());
    write_file(
        root,
        "main.tpz",
        "import host.math { twice }\nprint(\"ok\")\n",
    );
    write_file(root, "artifacts/host-math.wasm", ARTIFACT_BYTES);
    write_file(
        root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
}

pub(super) fn lispex_manifest(rule_name: &str) -> String {
    format!(
        "{}\n[lispex]\nprofile = \"{}\"\n\n[[lispex.rule]]\nname = \"{}\"\nsource = \"rules/refund.lspx\"\nlimits = \"rules/refund.limits.json\"\n",
        manifest_text(),
        LISPEX_BOUNDED_PROFILE_ID,
        rule_name
    )
}

pub(super) fn lispex_application_manifest(rule_name: &str) -> String {
    lispex_manifest(rule_name).replace(
        &format!("profile = \"{LISPEX_BOUNDED_PROFILE_ID}\""),
        &format!(
            "profile = \"{LISPEX_BOUNDED_PROFILE_ID}\"\napplication = \"{LISPEX_APPLICATION_PROFILE_ID}\"\napplication_quotas = \"rules/application.quotas.json\""
        ),
    )
}
