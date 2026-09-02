use super::support::*;

#[test]
fn self_rust_native_product_is_source_free_provenanced_and_fail_closed() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/self-hosting");
    let entry = fixture_root.join("dual-toolchain-simple.tpz");
    let root = std::env::temp_dir().join(format!("topaz_cli_self_native_{}", std::process::id()));
    let output = root.join("product");
    let rejected = root.join("rejected");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create DT-K4 tempdir");

    let emitted = topaz()
        .arg("emit")
        .arg(&entry)
        .args(["--compiler", "self"])
        .output()
        .expect("binary runs");
    assert!(emitted.status.success(), "{emitted:?}");
    assert!(emitted.stderr.is_empty(), "{emitted:?}");
    let source = String::from_utf8_lossy(&emitted.stdout);
    assert!(source.contains("TOPAZ_COMPILER_IR_JSON"), "{source}");
    assert!(source.contains("TOPAZ_EXPLICIT_MAIN"), "{source}");
    assert!(source.contains("run_with_host_and_input"), "{source}");
    assert!(source.contains("execute_product_program"), "{source}");

    let built = topaz()
        .arg("build")
        .arg(&entry)
        .args(["--compiler", "self", "--run"])
        .arg("--out-dir")
        .arg(&output)
        .output()
        .expect("binary runs");
    assert!(built.status.success(), "{built:?}");
    assert!(built.stdout.is_empty(), "{built:?}");
    let binary = output.join("target/debug/program");
    let executed = Command::new(&binary)
        .output()
        .expect("source-free binary runs");
    assert!(executed.status.success(), "{executed:?}");
    assert!(executed.stdout.is_empty(), "{executed:?}");
    assert!(executed.stderr.is_empty(), "{executed:?}");

    let manifest =
        std::fs::read_to_string(output.join("topaz-artifact.json")).expect("artifact manifest");
    assert!(manifest.contains("\"selector\": \"self\""), "{manifest}");
    assert!(
        manifest.contains("\"producer\": \"topaz-stage2\""),
        "{manifest}"
    );
    assert!(
        manifest.contains("\"selectionOrigin\": \"explicit\""),
        "{manifest}"
    );
    assert!(
        manifest.contains("\"targetCompilerFallback\": false"),
        "{manifest}"
    );
    for identity in [
        "compilerSourceSetId",
        "targetSourceSetId",
        "compileProductId",
        "generatedSourceSha256",
    ] {
        assert!(
            manifest.contains(&format!("\"{identity}\": \"sha256:")),
            "{manifest}"
        );
    }

    for arguments in [
        vec!["emit", entry.to_str().unwrap(), "--backend", "native"],
        vec!["build", entry.to_str().unwrap(), "--unchecked"],
    ] {
        let declined = topaz()
            .args(&arguments)
            .args(["--compiler", "self"])
            .arg("--out-dir")
            .arg(&rejected)
            .output()
            .expect("binary runs");
        assert!(!declined.status.success(), "{declined:?}");
        assert!(declined.stdout.is_empty(), "{declined:?}");
        let stderr = String::from_utf8_lossy(&declined.stderr);
        assert!(stderr.contains("--compiler rust"), "{stderr}");
        assert!(stderr.contains("not executed"), "{stderr}");
        assert!(!rejected.exists(), "{declined:?}");
    }

    let _ = std::fs::remove_dir_all(&root);
}

fn assert_namespace_not_value_rejection(out: &std::process::Output) {
    assert!(!out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("TPZ3012"), "{stderr}");
    assert!(stderr.contains("namespace, not a value"), "{stderr}");
}

#[test]
fn emit_lowers_a_program_to_rust() {
    // CDR-006 E-3: `emit` prints the lowered Rust module for a v5.2
    // single-module program — the hostable entry plus the program body.
    let out = rust_topaz()
        .arg("emit")
        .arg(emit_fixture())
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let rust = String::from_utf8_lossy(&out.stdout);
    assert!(rust.contains("pub fn run_with_host"), "{rust}");
    assert!(
        rust.contains("pub const TOPAZ_EXPLICIT_MAIN: bool = false"),
        "{rust}"
    );
    assert!(rust.contains("pub fn run_with_host_and_input"), "{rust}");
    assert!(
        rust.contains("async fn entry(cx: RtCx, __topaz_args: Value, __topaz_stdin: Value)"),
        "{rust}"
    );
    // the program body lowered (the `let` and the `print` effect).
    assert!(
        rust.contains("Value::Int(21)") && rust.contains("builtin_print"),
        "{rust}"
    );
}

#[test]
fn native_report_is_deterministic_and_observational_for_emit() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_native_report_emit_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("native report test dir");
    let report = dir.join("native-report.json");

    let baseline = topaz()
        .arg("emit")
        .arg("--backend")
        .arg("native")
        .arg(emit_fixture())
        .output()
        .expect("baseline emit runs");
    assert!(baseline.status.success(), "{baseline:?}");

    std::fs::write(&report, "replace-me").expect("seed previous report");
    let reported = topaz()
        .arg("emit")
        .arg("--backend")
        .arg("native")
        .arg("--native-report-json")
        .arg(&report)
        .arg(emit_fixture())
        .output()
        .expect("reported emit runs");
    assert!(reported.status.success(), "{reported:?}");
    assert_eq!(reported.stdout, baseline.stdout);
    assert_eq!(reported.stderr, baseline.stderr);

    let first = std::fs::read_to_string(&report).expect("report exists");
    assert!(
        first.starts_with(
            "{\"schemaVersion\":\"topaz.native-lowering-report.v1\",\"toolchainVersion\":"
        ),
        "{first}"
    );
    assert!(first.contains("\"command\":\"emit\""), "{first}");
    assert!(first.contains("\"target\":\"rust\""), "{first}");
    assert!(first.contains("\"requestedBackend\":\"native\""), "{first}");
    assert!(first.contains("\"selectionScope\":\"unit\""), "{first}");
    assert!(first.contains("\"moduleCount\":1"), "{first}");
    assert!(first.contains("\"containsExtern\":false"), "{first}");
    assert!(first.ends_with("]}\n"), "{first}");

    let repeated = topaz()
        .arg("emit")
        .arg("--backend")
        .arg("native")
        .arg("--native-report-json")
        .arg(&report)
        .arg(emit_fixture())
        .output()
        .expect("repeated reported emit runs");
    assert!(repeated.status.success(), "{repeated:?}");
    assert_eq!(repeated.stdout, baseline.stdout);
    assert_eq!(
        std::fs::read_to_string(&report).expect("replacement report exists"),
        first
    );
    assert!(
        std::fs::read_dir(&dir)
            .expect("report dir readable")
            .all(|entry| !entry
                .expect("report dir entry")
                .file_name()
                .to_string_lossy()
                .contains("topaz-native-report")),
        "atomic report write must not leave temporary or backup files"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn native_report_rejects_misuse_and_failed_lowering_without_artifacts() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_native_report_reject_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("native report rejection dir");

    let boxed_report = dir.join("boxed.json");
    let boxed = topaz()
        .arg("emit")
        .arg("--native-report-json")
        .arg(&boxed_report)
        .arg(emit_fixture())
        .output()
        .expect("boxed misuse runs");
    assert!(!boxed.status.success(), "{boxed:?}");
    assert!(
        String::from_utf8_lossy(&boxed.stderr).contains("requires `--backend native`"),
        "{boxed:?}"
    );
    assert!(!boxed_report.exists());

    let managed = dir.join("managed");
    let managed_report = managed.join("report.json");
    let collision = topaz()
        .arg("emit")
        .arg("--backend")
        .arg("native")
        .arg("--out-dir")
        .arg(&managed)
        .arg("--native-report-json")
        .arg(&managed_report)
        .arg(emit_fixture())
        .output()
        .expect("managed collision runs");
    assert!(!collision.status.success(), "{collision:?}");
    assert!(
        String::from_utf8_lossy(&collision.stderr).contains("must be outside managed output"),
        "{collision:?}"
    );
    assert!(!managed.exists(), "rejected path must not create output");

    let failed_report = dir.join("failed.json");
    let failed = topaz()
        .arg("emit")
        .arg("--backend")
        .arg("native")
        .arg("--native-report-json")
        .arg(&failed_report)
        .arg(repo_root().join("does/not/exist.tpz"))
        .output()
        .expect("failed lowering runs");
    assert!(!failed.status.success(), "{failed:?}");
    assert!(
        !failed_report.exists(),
        "failed lowering must not publish a success report"
    );
    assert!(
        std::fs::read_dir(&dir)
            .expect("report dir readable")
            .all(|entry| !entry
                .expect("report dir entry")
                .file_name()
                .to_string_lossy()
                .contains("topaz-native-report")),
        "failed lowering must clean its reserved temporary report"
    );

    let service_out = dir.join("service-out");
    let service_report = dir.join("service.json");
    let service = topaz()
        .arg("build")
        .arg("--target")
        .arg("http-service")
        .arg("--backend")
        .arg("native")
        .arg("--native-report-json")
        .arg(&service_report)
        .arg("--out-dir")
        .arg(&service_out)
        .arg(emit_fixture())
        .output()
        .expect("native service refusal runs");
    assert!(!service.status.success(), "{service:?}");
    assert!(
        String::from_utf8_lossy(&service.stderr).contains("requires `--backend boxed`"),
        "{service:?}"
    );
    assert!(!service_report.exists());
    assert!(!service_out.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn native_backend_runs_a_mixed_package_through_the_bounded_hybrid() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_native_hybrid_package_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("hybrid package src");
    let manifest = r#"[package]
name = "pkg_mode"
version = "0.1.0"
language = "5.9"
entry = "src/main.tpz"

[dependencies]
std = "5.9"
"#;
    std::fs::write(dir.join("topaz.toml"), manifest).expect("hybrid manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(manifest)).expect("hybrid lock");
    std::fs::write(
        dir.join("src/main.tpz"),
        r#"import util

let values = [1, 2, 3]
let word = util.keep("ok")
let result = util.twice(values.length)
print("{result}:{word}")
result
"#,
    )
    .expect("hybrid entry");
    std::fs::write(
        dir.join("util.tpz"),
        r#"export function twice(x: int) -> int {
  x * 2
}

export function keep(value: string) -> string {
  value
}

export function divide(a: int, b: int) -> int {
  a / b
}
"#,
    )
    .expect("hybrid module");

    let hybrid_emit = topaz()
        .arg("emit")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--backend")
        .arg("native")
        .output()
        .expect("hybrid emit runs");
    assert!(hybrid_emit.status.success(), "{hybrid_emit:?}");
    let emit_report = dir.join("hybrid-emit-report.json");
    let reported_hybrid_emit = topaz()
        .arg("emit")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--backend")
        .arg("native")
        .arg("--native-report-json")
        .arg(&emit_report)
        .output()
        .expect("reported hybrid emit runs");
    assert!(
        reported_hybrid_emit.status.success(),
        "{reported_hybrid_emit:?}"
    );
    assert_eq!(reported_hybrid_emit.stdout, hybrid_emit.stdout);
    assert_eq!(reported_hybrid_emit.stderr, hybrid_emit.stderr);
    assert!(
        std::fs::read_to_string(&emit_report)
            .expect("hybrid emit report")
            .contains("\"selectedBackend\":\"hybrid-native\"")
    );

    let boxed_out = dir.join("boxed-out");
    let boxed = topaz()
        .arg("build")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--out-dir")
        .arg(&boxed_out)
        .arg("--run")
        .output()
        .expect("boxed package build runs");
    assert!(boxed.status.success(), "{boxed:?}");

    let hybrid_out = dir.join("hybrid-out");
    let report = dir.join("hybrid-report.json");
    let hybrid = topaz()
        .arg("build")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--backend")
        .arg("native")
        .arg("--native-report-json")
        .arg(&report)
        .arg("--out-dir")
        .arg(&hybrid_out)
        .arg("--run")
        .output()
        .expect("hybrid package build runs");
    assert!(hybrid.status.success(), "{hybrid:?}");
    assert_eq!(hybrid.stdout, boxed.stdout);
    assert!(
        String::from_utf8_lossy(&hybrid.stdout).contains("6:ok"),
        "{hybrid:?}"
    );

    let report = std::fs::read_to_string(&report).expect("hybrid report");
    assert!(
        report.contains("\"selectedBackend\":\"hybrid-native\""),
        "{report}"
    );
    assert!(
        report.contains("\"name\":\"twice\",\"span\":")
            && report.contains("\"name\":\"keep\",\"span\":"),
        "{report}"
    );
    assert!(
        report.contains("\"declineReason\":\"non_scalar_signature\""),
        "{report}"
    );

    let web_out = dir.join("hybrid-web-out");
    let web_report = dir.join("hybrid-web-report.json");
    let web = topaz()
        .arg("build")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--target")
        .arg("web")
        .arg("--backend")
        .arg("native")
        .arg("--native-report-json")
        .arg(&web_report)
        .arg("--out-dir")
        .arg(&web_out)
        .output()
        .expect("hybrid Web build runs");
    assert!(web.status.success(), "{web:?}");
    assert!(web_out.join("topaz-web.wasm").is_file(), "{web:?}");
    assert!(web_out.join("topaz-web.js").is_file(), "{web:?}");
    assert!(
        std::fs::read_to_string(&web_report)
            .expect("hybrid Web report")
            .contains("\"selectedBackend\":\"hybrid-native\"")
    );

    std::fs::write(
        dir.join("src/main.tpz"),
        "import util\nlet values = [1]\nlet zero = values.length - values.length\nutil.divide(values.length, zero)\n",
    )
    .expect("hybrid fault entry");
    let interpreted_fault = topaz()
        .arg("run")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .output()
        .expect("interpreted fault runs");
    assert!(!interpreted_fault.status.success(), "{interpreted_fault:?}");
    let hybrid_fault = topaz()
        .arg("build")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--backend")
        .arg("native")
        .arg("--out-dir")
        .arg(dir.join("hybrid-fault-out"))
        .arg("--run")
        .output()
        .expect("hybrid fault build runs");
    assert!(!hybrid_fault.status.success(), "{hybrid_fault:?}");
    let interpreted_fault = String::from_utf8_lossy(&interpreted_fault.stderr);
    let hybrid_fault = String::from_utf8_lossy(&hybrid_fault.stderr);
    assert!(
        interpreted_fault.contains("error[TPZ4002]: integer division by zero"),
        "{interpreted_fault}"
    );
    assert!(
        interpreted_fault.contains("util.tpz:10:3"),
        "{interpreted_fault}"
    );
    assert!(
        hybrid_fault.contains("topaz fault: integer division by zero"),
        "{hybrid_fault}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_out_dir_scaffolds_a_runnable_crate() {
    // CDR-006 E-3 step 2: `emit --out-dir` writes a complete, self-contained
    // Cargo crate (Cargo.toml with its own [workspace] + path deps, a toolchain
    // pin, the emitted module, and a host harness). This assertion covers the
    // complete scaffold shape.
    let dir = std::env::temp_dir().join("topaz_emit_scaffold_test");
    let _ = std::fs::remove_dir_all(&dir);
    let out = topaz()
        .arg("emit")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml written");
    assert!(cargo.contains("[workspace]"), "{cargo}");
    // The emitted crate depends on the runtime + native host only — NOT the
    // interpreter (CDR-006 §7: emitted binaries don't carry the tree-walker).
    assert!(
        cargo.contains("topaz_rt = { path") && cargo.contains("topaz_host_native = { path"),
        "{cargo}"
    );
    assert!(
        !cargo.contains("topaz_interp"),
        "emitted crate must not pull in the interpreter: {cargo}"
    );
    let main = std::fs::read_to_string(dir.join("src/main.rs")).expect("main.rs written");
    assert!(
        main.contains("run_with_host") && main.contains("NativeHost"),
        "{main}"
    );
    let emitted = std::fs::read_to_string(dir.join("src/emitted.rs")).expect("emitted.rs written");
    assert!(emitted.contains("pub fn run_with_host"), "{emitted}");
    assert!(
        dir.join("rust-toolchain.toml").exists(),
        "toolchain pin written"
    );
    // CDR-006 §7: the emitted tree carries the vendored runtime closure and a
    // version-exact Cargo.lock, so `cargo build --offline --locked` works.
    assert!(
        dir.join("vendor/Cargo.toml").exists(),
        "vendored runtime workspace written"
    );
    assert!(
        dir.join("Cargo.lock").exists(),
        "carried Cargo.lock written"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_web_target_writes_wasm_package() {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("rustup runs");
    if !String::from_utf8_lossy(&installed.stdout).contains("wasm32-unknown-unknown") {
        return;
    }

    let dir = std::env::temp_dir().join("topaz_web_build_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    std::fs::write(
        dir.join("dep.tpz"),
        "export record Box<T> { foreign: T }\n\
         export enum Msg { Foreign(bool) }\n\
         export enum ForeignMsg { Foreign(bool) }\n\
         export newtype ForeignId = int\n\
         export function foreignBox(value: int) -> Box<int> { Box { foreign: value } }\n\
         export function marker() -> Msg { Msg.Foreign(true) }\n\
         export function foreign() -> ForeignMsg { ForeignMsg.Foreign(true) }\n\
         export function foreignId(n: int) -> ForeignId { ForeignId(n) }\n",
    )
    .expect("dep");
    std::fs::write(
        dir.join("sel.tpz"),
        "export enum SelectedMsg { Selected(bool) }\n\
         export record SelectedUser { name: string }\n\
         export newtype SelectedId = int\n\
         export function selected() -> SelectedMsg { SelectedMsg.Selected(true) }\n\
         export function selectedUser() -> SelectedUser { SelectedUser { name: \"Ada\" } }\n\
         export function selectedId(n: int) -> SelectedId { SelectedId(n) }\n",
    )
    .expect("selected import module");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        "import dep as D\n\
         import sel { SelectedMsg as SM, SelectedUser as SU, SelectedId as SID, selected, selectedUser, selectedId }\n\
         let importedMsg: D.ForeignMsg = D.foreign()\n\
         let importedId: D.ForeignId = D.foreignId(1)\n\
         let selectedMsg: SM = selected()\n\
         let selectedMsg2: SM = SM.Selected(false)\n\
         let selectedText = match selectedMsg2 {\n\
           case Selected(value) => \"{value}\"\n\
         }\n\
         let selectedUserValue: SU = selectedUser()\n\
         let selectedName = match selectedUserValue {\n\
           case SU { name } => name\n\
         }\n\
         let selectedIdValue: SID = selectedId(2)\n\
         let selectedIdInner = match selectedIdValue {\n\
           case SID(n) => n\n\
         }\n\
         export record User derives Show { name: string, age: int = 0 }\n\
         export record Box<T> { value: T }\n\
         export enum Msg { Noop, Inc(int), Rename(string, int) }\n\
         export newtype UserId = int\n\
         export function add(x: int) -> int { x + 1 }\n\
         export function make(name: string) -> User { User { name: name } }\n\
         export function makeBox(value: int) -> Box<int> { Box { value: value } }\n\
         export function event(n: int) -> Msg { Msg.Inc(n) }\n\
         export function wrap(id: int) -> UserId { UserId(id) }\n\
         export function acceptForeign(msg: D.ForeignMsg, id: D.ForeignId) -> string { \"ok\" }\n\
         export function acceptForeignBox(box: D.Box<int>) -> string { \"box\" }\n\
         export function acceptDuplicate(msg: D.Msg) -> string { \"dup\" }\n\
         export const K = 2\n",
    )
    .expect("entry");
    let out_dir = dir.join("web-out");
    let out = rust_topaz()
        .arg("build")
        .arg("--target")
        .arg("web")
        .arg(&entry)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    assert!(!out_dir.join("Cargo.toml").exists(), "{out:?}");
    assert!(!out_dir.join("Cargo.lock").exists(), "{out:?}");
    assert!(!out_dir.join("src").exists(), "{out:?}");
    assert!(!out_dir.join("vendor").exists(), "{out:?}");
    assert!(!out_dir.join("target").exists(), "{out:?}");
    let loader = std::fs::read_to_string(out_dir.join("topaz-web.js")).expect("loader");
    assert!(loader.contains("instantiateTopaz"), "{loader}");
    assert!(loader.contains("exports: callableExports"), "{loader}");
    assert!(loader.contains("callExportTrace"), "{loader}");
    let dts = std::fs::read_to_string(out_dir.join("topaz-web.d.ts")).expect("types written");
    assert!(
        dts.contains("\"add\"(x: TopazInt): TopazOutcome<TopazInt>;"),
        "{dts}"
    );
    assert!(
        dts.contains(
            "\"make\"(name: TopazString): TopazOutcome<TopazNominalRecord<\"User\", { \"name\": TopazString; \"age\": TopazInt }>>;"
        ),
        "{dts}"
    );
    assert!(
        dts.contains(
            "\"makeBox\"(value: TopazInt): TopazOutcome<TopazNominalRecord<\"Box<int>\", { \"value\": TopazInt }>>;"
        ),
        "{dts}"
    );
    assert!(
        dts.contains(
            "\"event\"(n: TopazInt): TopazOutcome<TopazEnum<\"Msg\", \"Noop\", []> | TopazEnum<\"Msg\", \"Inc\", [TopazInt]> | TopazEnum<\"Msg\", \"Rename\", [TopazString, TopazInt]>>;"
        ),
        "{dts}"
    );
    assert!(
        dts.contains("\"wrap\"(id: TopazInt): TopazOutcome<TopazNewtype<\"UserId\", TopazInt>>;"),
        "{dts}"
    );
    assert!(
        dts.contains(
            "\"acceptForeign\"(msg: TopazEnum<\"ForeignMsg\", \"Foreign\", [TopazBool]>, id: TopazNewtype<\"ForeignId\", TopazInt>): TopazOutcome<TopazString>;"
        ),
        "{dts}"
    );
    assert!(
        dts.contains(
            "\"acceptForeignBox\"(box: TopazNominalRecord<\"D.Box<int>\", { \"foreign\": TopazInt }>): TopazOutcome<TopazString>;"
        ),
        "{dts}"
    );
    assert!(
        dts.contains(
            "\"acceptDuplicate\"(msg: TopazEnum<\"Msg\", \"Foreign\", [TopazBool]>): TopazOutcome<TopazString>;"
        ),
        "{dts}"
    );
    assert!(dts.contains("exports: TopazExports;"), "{dts}");
    assert!(out_dir.join("topaz-web.wasm").exists(), "wasm copied");
    let wasm = std::fs::read(out_dir.join("topaz-web.wasm")).unwrap();
    for forbidden in [
        b"topaz-storage".as_slice(),
        b"/var/folders/".as_slice(),
        b"/private/var/".as_slice(),
        b"\\Users\\".as_slice(),
    ] {
        assert!(
            !wasm
                .windows(forbidden.len())
                .any(|window| window == forbidden),
            "temporary host path leaked into WASM: {}",
            String::from_utf8_lossy(forbidden)
        );
    }
    assert!(out_dir.join("LICENSE").is_file(), "license written");
    assert!(out_dir.join("NOTICE").is_file(), "notice written");
    let manifest =
        std::fs::read_to_string(out_dir.join("topaz-artifact.json")).expect("manifest written");
    assert!(manifest.contains("\"target\": \"web\""), "{manifest}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_target_value_trace_matches_input_print_stepper() {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("rustup runs");
    if !String::from_utf8_lossy(&installed.stdout).contains("wasm32-unknown-unknown")
        || !node_available()
    {
        return;
    }

    let dir = std::env::temp_dir().join("topaz_web_trace_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        "export function step() -> string {\n\
           let event = input()\n\
           print(\"cmd {event}\")\n\
           return \"value {event}\"\n\
         }\n",
    )
    .expect("entry");
    let out_dir = dir.join("web-out");
    let out = rust_topaz()
        .arg("build")
        .arg("--target")
        .arg("web")
        .arg(&entry)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    let loader = out_dir.join("topaz-web.js");
    let node_loader = out_dir.join("topaz-web.mjs");
    std::fs::copy(&loader, &node_loader).expect("copy browser module for Node ESM test");
    let wasm = out_dir.join("topaz-web.wasm");
    let script = format!(
        r#"
import {{ readFile }} from "node:fs/promises";
import {{ pathToFileURL }} from "node:url";
const {{ instantiateTopaz }} = await import(pathToFileURL({loader:?}).href);
const bytes = await readFile({wasm:?});
const mod = await instantiateTopaz(bytes);
const trace = mod.callExportTrace("step", [], "{{\"kind\":\"inc\",\"n\":1}}");
const wasm = mod.instance.exports;
if (wasm.topaz_live_allocations() !== 0) throw new Error("loader leaked allocations");
const ptr = wasm.topaz_alloc(30);
if (wasm.topaz_free_checked(ptr, 14) !== 2) throw new Error("length mismatch accepted");
if (wasm.topaz_live_allocations() !== 1) throw new Error("mismatch freed allocation");
if (wasm.topaz_free_checked(ptr, 30) !== 0) throw new Error("valid free failed");
if (wasm.topaz_free_checked(ptr, 30) !== 1) throw new Error("double free accepted");
console.log(JSON.stringify({{ trace, allocator: "checked" }}));
"#,
        loader = node_loader.to_string_lossy(),
        wasm = wasm.to_string_lossy(),
    );
    let node = Command::new("node")
        .arg("--input-type=module")
        .arg("-e")
        .arg(script)
        .output()
        .expect("node runs");
    assert!(node.status.success(), "{node:?}");
    let stdout = String::from_utf8_lossy(&node.stdout);
    assert!(
        stdout.contains(
            r#""outcome":{"status":"ok","value":{"$":"string","value":"value {\"kind\":\"inc\",\"n\":1}"}}"#
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#""stdout":["cmd {\"kind\":\"inc\",\"n\":1}"]"#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""deferErrors":[]"#), "{stdout}");
    assert!(stdout.contains(r#""allocator":"checked""#), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_web_worker_target_writes_worker_package() {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("rustup runs");
    if !String::from_utf8_lossy(&installed.stdout).contains("wasm32-unknown-unknown") {
        return;
    }

    let dir = std::env::temp_dir().join("topaz_web_worker_build_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    let entry = dir.join("main.tpz");
    std::fs::write(&entry, "export function add(x: int) -> int { x + 1 }\n").expect("entry");
    let out_dir = dir.join("web-worker-out");
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("web-worker")
        .arg(&entry)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    assert!(out_dir.join("topaz-web.wasm").exists(), "wasm copied");
    let worker =
        std::fs::read_to_string(out_dir.join("topaz-web-worker.js")).expect("worker written");
    assert!(worker.contains("instantiateTopaz"), "{worker}");
    let client = std::fs::read_to_string(out_dir.join("topaz-web-worker-client.js"))
        .expect("worker client written");
    assert!(client.contains("createTopazWorker"), "{client}");
    assert!(client.contains("new Worker"), "{client}");
    let dts = std::fs::read_to_string(out_dir.join("topaz-web.d.ts")).expect("types written");
    assert!(dts.contains("export interface TopazWorkerExports"), "{dts}");
    assert!(
        dts.contains("\"add\"(x: TopazInt): Promise<TopazOutcome<TopazInt>>;"),
        "{dts}"
    );
    assert!(dts.contains("createTopazWorker"), "{dts}");
    assert!(dts.contains("callExportTrace"), "{dts}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_web_target_rejects_run() {
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("web")
        .arg("--run")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(std::env::temp_dir().join("topaz_web_run_reject_test"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cannot be combined with `--run`"),
        "{out:?}"
    );
}

#[test]
fn build_web_worker_target_rejects_run() {
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("web-worker")
        .arg("--run")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(std::env::temp_dir().join("topaz_web_worker_run_reject_test"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("`build --target web-worker`"),
        "{out:?}"
    );
}

#[test]
fn build_web_targets_reject_unchecked() {
    for target in ["web", "web-worker"] {
        let out = topaz()
            .arg("build")
            .arg("--target")
            .arg(target)
            .arg("--unchecked")
            .arg(emit_fixture())
            .arg("--out-dir")
            .arg(std::env::temp_dir().join(format!("topaz_{target}_unchecked_reject_test")))
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{target}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("requires the checked build"),
            "{target}: {out:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn build_python_migration_preserves_non_unicode_user_cache_file() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_python_legacy_cache_ownership_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    let cache = dir.join("__pycache__");
    std::fs::create_dir_all(&cache).expect("legacy Python cache");
    std::fs::write(
        dir.join("program.py"),
        "# Topaz Python backend parity artifact.\n",
    )
    .expect("legacy generated program");
    std::fs::write(dir.join("topaz_py_rt.py"), "# legacy runtime\n")
        .expect("legacy generated runtime");
    let owned_cache = cache.join("program.cpython-313.pyc");
    let user_cache = cache.join("user.cpython-313.pyc");
    let non_unicode_cache = cache.join(std::ffi::OsString::from_vec(b"program.\xff.pyc".to_vec()));
    std::fs::write(&owned_cache, "generated cache").expect("owned cache file");
    std::fs::write(&user_cache, "ordinary user cache").expect("ordinary user cache file");
    std::fs::write(&non_unicode_cache, "non-Unicode user cache")
        .expect("non-Unicode user cache file");

    let out = rust_topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("migrating recognized pre-v5.6.1 python output"),
        "{stderr}"
    );
    let manifest = std::fs::read_to_string(dir.join("topaz-artifact.json"))
        .expect("current Python artifact manifest");
    assert!(manifest.contains("\"target\": \"python\""), "{manifest}");
    assert!(!owned_cache.exists(), "{out:?}");
    assert_eq!(
        std::fs::read(&user_cache).expect("ordinary user cache preserved"),
        b"ordinary user cache"
    );
    assert_eq!(
        std::fs::read(&non_unicode_cache).expect("non-Unicode user cache preserved"),
        b"non-Unicode user cache"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn build_python_target_writes_artifacts_without_legacy_experimental_flag() {
    let dir = std::env::temp_dir().join("topaz_python_no_experimental_test");
    let _ = std::fs::remove_dir_all(&dir);
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Python deployment bundle"), "{stderr}");
    assert!(!stderr.contains("deprecated"), "{stderr}");
    assert!(dir.join("program.py").exists(), "{out:?}");
    assert!(dir.join("topaz_py_rt.py").exists(), "{out:?}");
    assert!(dir.join("LICENSE").is_file(), "{out:?}");
    assert!(dir.join("NOTICE").is_file(), "{out:?}");
    assert!(dir.join("GENERATED-OUTPUT-NOTICE.txt").is_file(), "{out:?}");
    let manifest = std::fs::read_to_string(dir.join("topaz-artifact.json"))
        .expect("artifact manifest written last");
    assert!(
        manifest.contains("\"schema\": \"topaz.artifact.v1\""),
        "{manifest}"
    );
    assert!(manifest.contains("\"target\": \"python\""), "{manifest}");
    assert!(!dir.join("target").exists(), "{out:?}");
    if let Some(python) = cpython_31314() {
        for optimization in [None, Some("-O"), Some("-OO")] {
            let mut command = Command::new(&python);
            if let Some(flag) = optimization {
                command.arg(flag);
            }
            let run = command
                .arg("program.py")
                .current_dir(&dir)
                .output()
                .expect("generated Python runs");
            assert!(run.status.success(), "{run:?}");
            assert!(!dir.join("__pycache__").exists(), "{run:?}");
        }
    }
    let program_mtime = std::fs::metadata(dir.join("program.py"))
        .unwrap()
        .modified()
        .unwrap();
    let runtime_mtime = std::fs::metadata(dir.join("topaz_py_rt.py"))
        .unwrap()
        .modified()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let rebuilt = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(rebuilt.status.success(), "{rebuilt:?}");
    assert_eq!(
        std::fs::metadata(dir.join("program.py"))
            .unwrap()
            .modified()
            .unwrap(),
        program_mtime
    );
    assert_eq!(
        std::fs::metadata(dir.join("topaz_py_rt.py"))
            .unwrap()
            .modified()
            .unwrap(),
        runtime_mtime
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_python_target_supports_stdout_and_source_set() {
    let stdout = topaz()
        .arg("emit")
        .arg("--target")
        .arg("python")
        .arg(emit_fixture())
        .output()
        .expect("binary runs");
    assert!(stdout.status.success(), "{stdout:?}");
    let text = String::from_utf8_lossy(&stdout.stdout);
    assert!(
        text.contains("Generated Topaz Python application artifact"),
        "{text}"
    );
    assert!(text.contains("topaz emit --target python"), "{text}");

    let dir = std::env::temp_dir().join("topaz_python_emit_source_set_test");
    let _ = std::fs::remove_dir_all(&dir);
    let out = topaz()
        .arg("emit")
        .arg("--target")
        .arg("python")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(dir.join("program.py").is_file(), "{out:?}");
    assert!(dir.join("topaz_py_rt.py").is_file(), "{out:?}");
    assert!(dir.join("LICENSE-RUNTIME").is_file(), "{out:?}");
    assert!(!dir.join("topaz-artifact.json").exists(), "{out:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn emit_and_build_target_names_fail_closed() {
    for (command, target, expected) in [
        ("emit", "native", "unknown emit `--target`"),
        ("build", "rust", "unknown build `--target`"),
    ] {
        let out = topaz()
            .arg(command)
            .arg("--target")
            .arg(target)
            .arg(emit_fixture())
            .arg("--out-dir")
            .arg(std::env::temp_dir().join("topaz_target_type_confusion"))
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expected),
            "{out:?}"
        );
    }

    let out = topaz()
        .arg("emit")
        .arg("--target")
        .arg("python")
        .arg("--backend")
        .arg("native")
        .arg(emit_fixture())
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("applies to Rust targets"),
        "{out:?}"
    );
}

#[test]
fn build_python_target_rejects_inapplicable_flags() {
    for (name, flag, expected) in [
        ("run", "--run", "cannot be combined with `--run`"),
        ("release", "--release", "`--release` does not apply"),
        (
            "backend",
            "--backend",
            "`--backend` applies to Rust targets",
        ),
    ] {
        let dir = std::env::temp_dir().join(format!("topaz_python_{name}_reject_test"));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cmd = topaz();
        cmd.arg("build").arg("--target").arg("python");
        if flag == "--backend" {
            cmd.arg("--backend").arg("native");
        } else {
            cmd.arg(flag);
        }
        let out = cmd
            .arg(emit_fixture())
            .arg("--out-dir")
            .arg(&dir)
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{name}: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(expected), "{name}: {stderr}");
        assert!(!dir.join("program.py").exists(), "{name}: {out:?}");
    }
}

#[test]
fn build_python_target_accepts_legacy_experimental_flag() {
    let dir = std::env::temp_dir().join("topaz_python_parity_build_test");
    let _ = std::fs::remove_dir_all(&dir);
    let out = rust_topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg("--experimental")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Python deployment bundle"), "{stderr}");
    assert!(!stderr.contains("experimental Python"), "{stderr}");

    let program = std::fs::read_to_string(dir.join("program.py")).expect("program.py written");
    assert!(
        program.contains("Generated Topaz Python application artifact"),
        "{program}"
    );
    assert!(program.contains("Replaceable compiler output"), "{program}");
    assert!(program.contains("def run(stdin_text: str"), "{program}");
    assert!(
        program.contains("from topaz_py_rt import Err, Host"),
        "{program}"
    );
    assert!(!program.contains("\nopen("), "{program}");
    assert!(!program.contains(" open("), "{program}");
    assert!(!program.contains("subprocess"), "{program}");

    let runtime =
        std::fs::read_to_string(dir.join("topaz_py_rt.py")).expect("topaz_py_rt.py written");
    assert!(runtime.contains("class Host"), "{runtime}");
    assert!(runtime.contains("def tpz_trace_line"), "{runtime}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_python_target_accepts_locked_package_mode_with_local_module() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_python_package_mode_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/contract")).expect("mkdir");
    let manifest = package_manifest();
    std::fs::write(dir.join("topaz.toml"), &manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(&manifest)).expect("lock");
    std::fs::write(
        dir.join("src/contract/response.tpz"),
        "export function label(n: int) -> string { \"status:{n}\" }\n",
    )
    .expect("response module");
    std::fs::write(
        dir.join("src/main.tpz"),
        "import src.contract.response { label }\nprint(label(201))\n",
    )
    .expect("entry");

    let out_dir = dir.join("py-out");
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Python deployment bundle"), "{stderr}");
    assert!(!stderr.contains("experimental Python"), "{stderr}");

    let program = std::fs::read_to_string(out_dir.join("program.py")).expect("program.py written");
    assert!(
        program.contains("Generated Topaz Python application artifact"),
        "{program}"
    );
    assert!(program.contains("src.contract.response"), "{program}");
    assert!(program.contains("def _tpz_init_"), "{program}");
    assert!(program.contains("def run(stdin_text: str"), "{program}");
    assert!(!program.contains("\nopen("), "{program}");
    assert!(out_dir.join("topaz_py_rt.py").exists(), "{out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_python_target_preserves_namespace_type_only_record_policy() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_python_namespace_record_policy_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(
        dir.join("model.tpz"),
        "export record User { name: string }\n",
    )
    .expect("model");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        "import model\nlet u = model.User { name: \"Ada\" }\nprint(u.name)\n",
    )
    .expect("entry");

    let out_dir = dir.join("py-out");
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg(&entry)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("TPZ3013"), "{stderr}");
    assert!(stderr.contains("not a value"), "{stderr}");
    assert!(stderr.contains("use it in type position"), "{stderr}");
    assert!(!out_dir.join("program.py").exists(), "{out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_and_build_python_target_rejects_optional_access_on_namespace_before_emission() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_python_optional_namespace_reject_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("config.tpz"), "export const base = 36\n").expect("config");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        "import config\nlet n = config?.base\nprint(\"{n}\")\n",
    )
    .expect("entry");

    let run = topaz()
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert_namespace_not_value_rejection(&run);

    let unchecked_run = topaz()
        .arg("--unchecked")
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert_namespace_not_value_rejection(&unchecked_run);

    let out_dir = dir.join("py-out");
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg(&entry)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert_namespace_not_value_rejection(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("TPZ6PY0001"), "{stderr}");
    assert!(!out_dir.join("program.py").exists(), "{out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_python_target_rejects_nominal_impossible_equality_before_emission() {
    let cases = [
        (
            "cross_id",
            r#"
record User { name: string, age: int }
record Admin { name: string, age: int }
let u = User { name: "Ada", age: 36 }
let admin = Admin { name: "Ada", age: 36 }
let same = u == admin
"#,
            "`User` and `Admin` are never equal",
        ),
        (
            "structural",
            r#"
record User { name: string, age: int }
let u = User { name: "Ada", age: 36 }
let structural = { name: "Ada", age: 36 }
let same = u == structural
"#,
            "`User` and `{ age: int, name: string }` are never equal",
        ),
    ];

    for (name, source, expected) in cases {
        let dir = std::env::temp_dir().join(format!(
            "topaz_python_nominal_impossible_eq_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let entry = dir.join("main.tpz");
        std::fs::write(&entry, source).expect("entry");

        let out_dir = dir.join("py-out");
        let out = rust_topaz()
            .arg("build")
            .arg("--target")
            .arg("python")
            .arg(&entry)
            .arg("--out-dir")
            .arg(&out_dir)
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("TPZ5007"), "{stderr}");
        assert!(stderr.contains(expected), "{stderr}");
        assert!(!out_dir.join("program.py").exists(), "{out:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn build_python_target_rejects_v51_package_mode() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_python_v51_package_mode_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    std::fs::write(
        dir.join("topaz.toml"),
        r#"[package]
name = "old_pkg"
version = "0.1.0"
language = "5.1"
entry = "src/main.tpz"
"#,
    )
    .expect("manifest");
    std::fs::write(dir.join("src/main.tpz"), "print(\"old\")\n").expect("entry");

    let out_dir = dir.join("py-out");
    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg("--root")
        .arg(&dir)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[package].language must be 5.2 or newer"),
        "{stderr}"
    );
    assert!(!out_dir.join("program.py").exists(), "{out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_python_target_accepts_extern_package_replay_mode() {
    let package_root = repo_root().join("corpus/v5_4/extern/extern-replay-package");
    let out_dir = std::env::temp_dir().join(format!(
        "topaz_python_extern_package_replay_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out_dir);

    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg("--root")
        .arg(&package_root)
        .arg("--locked")
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Python deployment bundle"), "{stderr}");
    assert!(!stderr.contains("experimental Python"), "{stderr}");
    let program = std::fs::read_to_string(out_dir.join("program.py")).expect("program.py written");
    assert!(program.contains("host.math"), "{program}");
    assert!(program.contains("tpz_extern_function"), "{program}");
    assert!(
        program.contains("_TPZ_EXTERN_SANDBOX_POLICIES_JSON"),
        "{program}"
    );
    assert!(program.contains("\\\"fuel\\\":1000"), "{program}");
    assert!(program.contains("\\\"memory_bytes\\\":65536"), "{program}");
    assert!(out_dir.join("topaz_py_rt.py").exists(), "{out:?}");

    let Some(python) = cpython_31314() else {
        eprintln!("skipping generated extern replay execution: CPython 3.13.14 was not found");
        let _ = std::fs::remove_dir_all(&out_dir);
        return;
    };
    let runner = out_dir.join("run_extern_main.py");
    std::fs::write(
        &runner,
        r#"
from __future__ import annotations

import importlib.util
import json
import sys

root = sys.argv[1]
spec = importlib.util.spec_from_file_location("topaz_generated_extern", root + "/program.py")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

module.run("")

def policy(fuel, memory_bytes):
    return json.dumps(
        [{
            "artifact_path": "artifacts/host-math.wasm",
            "fuel": fuel,
            "kind": "wasm",
            "memory_bytes": memory_bytes,
            "module": "host.math",
        }],
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )

def call(replay_jsonl, policies_json):
    host = module.Host("", None, replay_jsonl, policies_json)
    try:
        result = module._t_6d61696e(host, [], "")
        return {
            "stdout": host.stdout,
            "result": type(result).__name__,
            "value": getattr(result, "value", None),
        }
    except module.TpzFault as fault:
        return {
            "stdout": host.stdout,
            "fault": fault.to_json(),
        }

sys.stdout.write(json.dumps(
    {
        "ok": call(module._TPZ_EXTERN_REPLAY_JSONL, module._TPZ_EXTERN_SANDBOX_POLICIES_JSON),
        "fuel": call(module._TPZ_EXTERN_REPLAY_JSONL, policy(2, 65536)),
        "memory": call(module._TPZ_EXTERN_REPLAY_JSONL, policy(1000, 16)),
        "missing": call("", module._TPZ_EXTERN_SANDBOX_POLICIES_JSON),
    },
    ensure_ascii=False,
    sort_keys=True,
    separators=(",", ":"),
))
sys.stdout.write("\n")
"#,
    )
    .expect("write extern runner");

    let py = Command::new(python)
        .arg(&runner)
        .arg(&out_dir)
        .output()
        .expect("CPython runs generated extern replay artifact");
    assert!(py.status.success(), "{py:?}");
    let stdout = String::from_utf8_lossy(&py.stdout);
    assert!(stdout.contains("\"ok\":{\"result\":\"Ok\""), "{stdout}");
    assert!(stdout.contains("\"stdout\":[\"twice=42\"]"), "{stdout}");
    assert!(stdout.contains("\"value\":0"), "{stdout}");
    assert!(
        stdout.contains("\"fuel\":{\"fault\":{\"code\":\"TPZ5032\""),
        "{stdout}"
    );
    assert!(
        stdout
            .contains("extern replay fuel limit exceeded for `host.math.twice`: used 3, budget 2"),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"memory\":{\"fault\":{\"code\":\"TPZ5032\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("extern replay memory_bytes limit exceeded for `host.math.twice`"),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"missing\":{\"fault\":{\"code\":\"TPZ5032\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("extern replay has no row for `host.math.twice`"),
        "{stdout}"
    );
    assert!(String::from_utf8_lossy(&py.stderr).is_empty(), "{py:?}");

    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn build_python_target_accepts_extern_package_replay_abi_breadth() {
    let root = std::env::temp_dir().join(format!(
        "topaz_python_extern_abi_breadth_test_{}",
        std::process::id()
    ));
    let out_dir = root.join("py-out");
    let replay_dir = root.join("replay");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&replay_dir).expect("replay dir");
    std::fs::write(
        root.join("topaz.toml"),
        r#"[package]
name = "extern_abi_breadth"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"

[extern.host.abi]
hash = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
abi_hash = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[[extern.host.abi.functions]]
name = "negate"
params = ["bool"]
result = "bool"

[[extern.host.abi.functions]]
name = "greet"
params = ["string"]
result = "string"

[[extern.host.abi.functions]]
name = "sumValues"
params = ["Array<int>"]
result = "int"

[[extern.host.abi.functions]]
name = "maybePlus"
params = ["Option<int>"]
result = "Option<int>"

[[extern.host.abi.functions]]
name = "resultLabel"
params = ["Result<int,string>"]
result = "Result<string,string>"

[[extern.host.abi.functions]]
name = "lengthBytes"
params = ["Bytes"]
result = "int"

[extern.host.abi.replay]
fixture = "replay/host-abi.jsonl"

[extern.host.abi.sandbox]
kind = "replay"
fuel = 1000
memory_bytes = 65536
"#,
    )
    .expect("manifest");
    std::fs::write(
        root.join("main.tpz"),
        r#"import host.abi { negate, greet, sumValues, maybePlus, resultLabel, lengthBytes }

export function main(args: Array<string>, stdin: string) -> Result<int, string> {
  let b = negate(true)
  let msg = greet("Topaz")
  let total = sumValues([1, 2, 3])
  let opt = maybePlus(Some(7))
  let res = resultLabel(Ok(5))
  let len = lengthBytes(Bytes.encodeUtf8("abc"))
  let optValue = match opt {
    case Some(v) => v
    case None => -1
  }
  let resValue = match res {
    case Ok(text) => text
    case Err(e) => e
  }
  if b != false { return Err("bad bool") }
  if msg != "hi Topaz" { return Err("bad string") }
  if total != 6 { return Err("bad array") }
  if optValue != 8 { return Err("bad option") }
  if resValue != "ok=5" { return Err("bad result") }
  if len != 3 { return Err("bad bytes") }
  print("abi={b}:{msg}:{total}:{optValue}:{resValue}:{len}")
  Ok(0)
}
"#,
    )
    .expect("entry");
    std::fs::write(
        replay_dir.join("host-abi.jsonl"),
        concat!(
            "{\"module\":\"host.abi\",\"function\":\"negate\",\"args\":[{\"$\":\"bool\",\"value\":true}],\"result\":{\"$\":\"bool\",\"value\":false}}\n",
            "{\"module\":\"host.abi\",\"function\":\"greet\",\"args\":[{\"$\":\"string\",\"value\":\"Topaz\"}],\"result\":{\"$\":\"string\",\"value\":\"hi Topaz\"}}\n",
            "{\"module\":\"host.abi\",\"function\":\"sumValues\",\"args\":[{\"$\":\"array\",\"items\":[{\"$\":\"int\",\"value\":\"1\"},{\"$\":\"int\",\"value\":\"2\"},{\"$\":\"int\",\"value\":\"3\"}]}],\"result\":{\"$\":\"int\",\"value\":\"6\"}}\n",
            "{\"module\":\"host.abi\",\"function\":\"maybePlus\",\"args\":[{\"$\":\"some\",\"value\":{\"$\":\"int\",\"value\":\"7\"}}],\"result\":{\"$\":\"some\",\"value\":{\"$\":\"int\",\"value\":\"8\"}}}\n",
            "{\"module\":\"host.abi\",\"function\":\"resultLabel\",\"args\":[{\"$\":\"ok\",\"value\":{\"$\":\"int\",\"value\":\"5\"}}],\"result\":{\"$\":\"ok\",\"value\":{\"$\":\"string\",\"value\":\"ok=5\"}}}\n",
            "{\"module\":\"host.abi\",\"function\":\"lengthBytes\",\"args\":[{\"$\":\"bytes\",\"hex\":\"616263\"}],\"result\":{\"$\":\"int\",\"value\":\"3\"}}\n",
        ),
    )
    .expect("replay fixture");

    let out = topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg("--root")
        .arg(&root)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let program = std::fs::read_to_string(out_dir.join("program.py")).expect("program.py written");
    assert!(program.contains("host.abi"), "{program}");
    assert!(
        program.contains("\\\"module\\\":\\\"host.abi\\\""),
        "{program}"
    );
    assert!(program.contains("\\\"kind\\\":\\\"replay\\\""), "{program}");
    assert!(program.contains("\\\"artifact_path\\\":null"), "{program}");

    let Some(python) = cpython_31314() else {
        eprintln!("skipping generated extern ABI replay execution: CPython 3.13.14 was not found");
        let _ = std::fs::remove_dir_all(&root);
        return;
    };
    let runner = out_dir.join("run_extern_abi.py");
    std::fs::write(
        &runner,
        r#"
from __future__ import annotations

import importlib.util
import json
import sys

root = sys.argv[1]
spec = importlib.util.spec_from_file_location("topaz_generated_extern_abi", root + "/program.py")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

module.run("")
host = module.Host("", None, module._TPZ_EXTERN_REPLAY_JSONL, module._TPZ_EXTERN_SANDBOX_POLICIES_JSON)
result = module._t_6d61696e(host, [], "")
sys.stdout.write(json.dumps(
    {
        "stdout": host.stdout,
        "result": type(result).__name__,
        "value": getattr(result, "value", None),
    },
    ensure_ascii=False,
    sort_keys=True,
    separators=(",", ":"),
))
sys.stdout.write("\n")
"#,
    )
    .expect("write ABI runner");

    let py = Command::new(python)
        .arg(&runner)
        .arg(&out_dir)
        .output()
        .expect("CPython runs generated extern ABI replay artifact");
    assert!(py.status.success(), "{py:?}");
    let stdout = String::from_utf8_lossy(&py.stdout);
    assert!(stdout.contains("\"result\":\"Ok\""), "{stdout}");
    assert!(stdout.contains("\"value\":0"), "{stdout}");
    assert!(
        stdout.contains("\"stdout\":[\"abi=false:hi Topaz:6:8:ok=5:3\"]"),
        "{stdout}"
    );
    assert!(String::from_utf8_lossy(&py.stderr).is_empty(), "{py:?}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn build_python_target_emits_zero_timeout_noninstant_concurrent() {
    let src = std::env::temp_dir().join("topaz_python_decline_concurrent_timeout.tpz");
    let dir = std::env::temp_dir().join("topaz_python_decline_concurrent_timeout_out");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::write(
        &src,
        r#"
function f() -> int { 1 }
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        a: f()
    } else {
        { a: 0 }
    }
    r.a
}
main()
"#,
    )
    .expect("write source");

    let out = rust_topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg(&src)
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("wrote Python deployment bundle"),
        "{stderr}"
    );
    let program = std::fs::read_to_string(dir.join("program.py")).expect("read Python artifact");
    assert!(
        program.contains("tpz_concurrent_join_timeout("),
        "{program}"
    );
    assert!(dir.join("topaz_py_rt.py").exists(), "{out:?}");

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_python_target_rejects_concrete_non_callable_local_calls_before_emission() {
    let cases = [
        (
            "zero_arg",
            r#"
function main() -> int {
    let f = 1
    f()
}
"#,
        ),
        (
            "positional",
            r#"
function main() -> int {
    let f = 1
    f(1)
}
"#,
        ),
    ];

    for (name, source) in cases {
        let src = std::env::temp_dir().join(format!(
            "topaz_python_not_callable_local_{name}_{}.tpz",
            std::process::id()
        ));
        let dir = std::env::temp_dir().join(format!(
            "topaz_python_not_callable_local_{name}_out_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::write(&src, source).expect("write source");

        let out = rust_topaz()
            .arg("build")
            .arg("--target")
            .arg("python")
            .arg(&src)
            .arg("--out-dir")
            .arg(&dir)
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{name}: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("TPZ5005"), "{name}: {stderr}");
        assert!(stderr.contains("not callable"), "{name}: {stderr}");
        assert!(!stderr.contains("TPZ6PY"), "{name}: {stderr}");
        assert!(!dir.join("program.py").exists(), "{name}: {out:?}");
        assert!(!dir.join("topaz_py_rt.py").exists(), "{name}: {out:?}");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn build_python_target_rejects_non_final_variadic_before_emission() {
    let src = std::env::temp_dir().join(format!(
        "topaz_python_non_final_variadic_{}.tpz",
        std::process::id()
    ));
    let dir = std::env::temp_dir().join(format!(
        "topaz_python_non_final_variadic_out_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::write(
        &src,
        r#"
function bad(...xs: int, last: int) -> int {
    last
}
bad(1, 2)
"#,
    )
    .expect("write source");

    let out = rust_topaz()
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg(&src)
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("TPZ5024"), "{stderr}");
    assert!(
        stderr.contains("variadic parameter must be final"),
        "{stderr}"
    );
    assert!(!stderr.contains("TPZ6PY"), "{stderr}");
    assert!(!dir.join("program.py").exists(), "{out:?}");
    assert!(!dir.join("topaz_py_rt.py").exists(), "{out:?}");

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_requires_out_dir() {
    // `build` needs somewhere to put the crate + target; without it, fail cleanly
    // (before any cargo invocation).
    let out = topaz()
        .arg("build")
        .arg(emit_fixture())
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("requires `--out-dir"),
        "{out:?}"
    );
    // `--release` and `--run` do not bypass the `--out-dir` requirement.
    for flag in ["--release", "--run"] {
        let out = topaz()
            .arg("build")
            .arg(flag)
            .arg(emit_fixture())
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{flag}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("requires `--out-dir"),
            "{flag}: {out:?}"
        );
    }
}
