use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

const HASH_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct TempPackage {
    root: PathBuf,
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn temp_package(name: &str) -> TempPackage {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("topaz-cli-{name}-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temp package root");
    TempPackage { root }
}

fn write_file(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, text).expect("write temp package file");
}

fn extern_manifest(package: &str) -> String {
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"

[extern.host.image]
hash = "{HASH_A}"
abi_hash = "{HASH_B}"

[[extern.host.image.functions]]
name = "resizePng"
params = ["Bytes", "int", "int"]
result = "Bytes"

[extern.host.image.replay]
fixture = "replay/host-image.jsonl"

[extern.host.image.sandbox]
kind = "replay"
"#
    )
}

fn extern_math_manifest(package: &str) -> String {
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"

[extern.host.math]
hash = "{HASH_A}"
abi_hash = "{HASH_B}"

[[extern.host.math.functions]]
name = "twice"
params = ["int"]
result = "int"

[extern.host.math.replay]
fixture = "replay/host-math.jsonl"

[extern.host.math.sandbox]
kind = "replay"
"#
    )
}

fn extern_math_budget_manifest(package: &str, fuel: u64, memory_bytes: u64) -> String {
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"

[extern.host.math]
hash = "{HASH_A}"
abi_hash = "{HASH_B}"

[[extern.host.math.functions]]
name = "twice"
params = ["int"]
result = "int"

[extern.host.math.replay]
fixture = "replay/host-math.jsonl"

[extern.host.math.sandbox]
kind = "replay"
fuel = {fuel}
memory_bytes = {memory_bytes}
"#
    )
}

fn extern_math_wasm_manifest(package: &str) -> String {
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"

[extern.host.math]
hash = "{HASH_A}"
abi_hash = "{HASH_B}"

[[extern.host.math.functions]]
name = "twice"
params = ["int"]
result = "int"

[extern.host.math.artifact]
path = "artifacts/host-math.wasm"

[extern.host.math.replay]
fixture = "replay/host-math.jsonl"

[extern.host.math.sandbox]
kind = "wasm"
fuel = 1000
memory_bytes = 65536
"#
    )
}

fn basic_manifest(package: &str) -> String {
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
language = "5.4"
entry = "main.tpz"
"#
    )
}

fn package_target_for(root: &Path) -> PackageTarget {
    package_target_for_locked(root, false)
}

fn package_target_for_locked(root: &Path, locked: bool) -> PackageTarget {
    let root_arg = root.to_string_lossy().into_owned();
    package_target(Some(&root_arg), Some(LangVersion::V5_4), locked)
        .unwrap_or_else(|_| panic!("package target loads for `{root_arg}`"))
}

#[test]
fn locked_lispex_application_is_rejected_on_v517_before_target_assembly() {
    let package = temp_package("lispex-application-v517-denied");
    write_file(
        &package.root,
        "topaz.toml",
        r#"[package]
name = "lispex_application_denied"
version = "0.1.0"
language = "5.17"
entry = "main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.17"

[lispex]
profile = "lispex/r7rs-rule-embedded-core/1"
application = "topaz/lispex-decision-application/1"
application_quotas = "rules/application.quotas.json"

[[lispex.rule]]
name = "환불"
source = "rules/refund.lspx"
limits = "rules/refund.limits.json"
"#,
    );
    write_file(&package.root, "main.tpz", "print(\"unreachable\")\n");
    let root_arg = package.root.to_string_lossy().into_owned();
    assert!(matches!(
        package_target(Some(&root_arg), Some(LangVersion::V5_17), true),
        Err(code) if code == ExitCode::FAILURE
    ));
    assert!(!package.root.join("topaz.lock").exists());
    assert!(!package.root.join(".topaz").exists());
}

#[test]
fn v51_run_keeps_the_frozen_single_file_route() {
    let package = temp_package("v51-single-file-run");
    write_file(&package.root, "main.tpz", "let value = 1\n");
    let entry = package.root.join("main.tpz");
    let entry = entry.to_string_lossy();
    assert_eq!(
        run_entry(&entry, None, LangVersion::V5_1, false, &[]),
        ExitCode::SUCCESS
    );
}

#[test]
fn current_v518_run_is_accepted_for_execution() {
    let package = temp_package("dormant-v518-run-denied");
    write_file(&package.root, "main.tpz", "let value = 18\n");
    let entry = package.root.join("main.tpz");
    let entry = entry.to_string_lossy();
    assert_eq!(
        run_entry(&entry, None, LangVersion::V5_17, false, &[]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        run_entry(&entry, None, LangVersion::V5_18, false, &[]),
        ExitCode::SUCCESS
    );
}

#[test]
fn current_v518_migration_selector_is_selectable() {
    assert_eq!(
        migrate_version_arg("--to", Some("5.18")),
        Some(LangVersion::V5_18)
    );
}

#[test]
fn current_v519_run_and_migration_selector_are_selectable() {
    let package = temp_package("current-v519-run");
    write_file(&package.root, "main.tpz", "let value = 19\n");
    let entry = package.root.join("main.tpz");
    let entry = entry.to_string_lossy();
    assert_eq!(
        run_entry(&entry, None, LangVersion::V5_19, false, &[]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        migrate_version_arg("--to", Some("5.19")),
        Some(LangVersion::V5_19)
    );
}

#[test]
fn current_v520_run_and_migration_selector_are_selectable() {
    let package = temp_package("current-v520-run");
    write_file(&package.root, "main.tpz", "let value = 20\n");
    let entry = package.root.join("main.tpz");
    let entry = entry.to_string_lossy();
    assert_eq!(
        run_entry(&entry, None, LangVersion::V5_20, false, &[]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        migrate_version_arg("--to", Some("5.20")),
        Some(LangVersion::V5_20)
    );
}

#[test]
fn candidate_v518_lispex_application_exposes_only_generated_modules_and_rule_facts() {
    let package = temp_package("lispex-application");
    write_file(
        &package.root,
        "topaz.toml",
        r#"[package]
name = "lispex_application"
version = "0.1.0"
language = "5.18"
entry = "main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.18"

[lispex]
profile = "lispex/r7rs-rule-embedded-core/1"
application = "topaz/lispex-decision-application/1"
application_quotas = "rules/application.quotas.json"

[[lispex.rule]]
name = "환불"
source = "rules/refund.lspx"
limits = "rules/refund.limits.json"
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import std.lispex {
    LispexConsumerArtifactInspection,
    LispexConsumerEvidence,
    LispexEvidenceOutcome,
    LispexLimits,
    LispexRuleIdentity,
    LispexSettlement,
    consumerArtifactBytes,
    consumerArtifactFromBytes,
    defaultLimits,
    evaluate,
    evaluateWithEvidence,
    freshReplay,
    inspectConsumerArtifact,
    inspectRule,
    portableCoreBytes,
    valueFromCanonical,
    verifyConsumerArtifact,
}
import std.lispex.rules as rules

export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    let rule = rules.환불()
    let identity: LispexRuleIdentity = inspectRule(rule)
    if identity.name != "환불" {
        return Err("prepared rule identity mismatch")
    }
    let bytes = match Bytes.fromArray([0]) {
        case Ok(value) => value
        case Err(error) => return Err(error)
    }
    let input = match valueFromCanonical(bytes) {
        case Ok(value) => value
        case Err(_) => return Err("canonical input refused")
    }
    let defaults = defaultLimits(rule)
    let limits = LispexLimits {
        canonicalInputBytes: defaults.canonicalInputBytes,
        evalWork: defaults.evalWork,
        logicalAllocation: defaults.logicalAllocation,
        semanticFrames: defaults.semanticFrames,
        traversalDepth: defaults.traversalDepth,
        outputBytes: defaults.outputBytes,
        diagnosticBytes: defaults.diagnosticBytes,
        transcriptBytes: defaults.transcriptBytes,
        transcriptEvents: defaults.transcriptEvents,
        resultBytes: defaults.resultBytes,
    }
    match evaluate(rule, input, limits) {
        case Ok(Complete(_)) => ()
        case Ok(_) => return Err("deterministic evaluation did not complete")
        case Err(_) => return Err("operational evaluation fault")
    }
    let evidence = match evaluateWithEvidence(rule, input, defaultLimits(rule)) {
        case Ok(Portable(found)) => found
        case Ok(Unrecorded(_)) => return Err("evaluation did not retain portable evidence")
        case Err(_) => return Err("evidence evaluation fault")
    }
    let serialized = consumerArtifactBytes(evidence.artifact)
    let restored = match consumerArtifactFromBytes(serialized) {
        case Ok(value) => value
        case Err(_) => return Err("consumer artifact did not round-trip")
    }
    let inspection = match verifyConsumerArtifact(restored) {
        case Ok(value) => value
        case Err(_) => return Err("consumer artifact verification failed")
    }
    if inspection.kind != "evaluate" ||
        inspection.category != "complete" ||
        inspection.authenticated ||
        inspection.issuer != None ||
        inspection.semanticProfileId != Some("lispex/r7rs-rule-embedded-core/1") ||
        inspection.portableCoreSha256 == None {
        return Err("consumer artifact inspection mismatch")
    }
    match inspectConsumerArtifact(restored) {
        case Ok(value) => if value.artifactSha256 != inspection.artifactSha256 {
            return Err("independent artifact inspection mismatch")
        }
        case Err(_) => return Err("consumer artifact inspection failed")
    }
    let core = match portableCoreBytes(restored) {
        case Ok(bytes) => bytes
        case Err(_) => return Err("portable core extraction failed")
    }
    if core.isEmpty() {
        return Err("portable core is empty")
    }
    match freshReplay(rule, input, restored) {
        case Ok(Complete(_)) => Ok(0)
        case Ok(_) => Err("fresh replay did not complete")
        case Err(_) => Err("fresh replay failed")
    }
}
"#,
    );
    write_file(
        &package.root,
        "rules/refund.lspx",
        "(if (< 10 15) \"allow\" \"deny\")\n",
    );
    write_file(
        &package.root,
        "rules/refund.limits.json",
        "{\n  \"schema\": \"topaz.lispex-embed-limits/v1\",\n  \"prepare\": {\n    \"raw_source_bytes\": 4096,\n    \"prepare_work\": 1000000,\n    \"logical_allocation\": 1000000,\n    \"syntax_depth\": 64\n  },\n  \"evaluate\": {\n    \"canonical_input_bytes\": 4096,\n    \"eval_work\": 1000000,\n    \"logical_allocation\": 1000000,\n    \"semantic_frames\": 1000,\n    \"traversal_depth\": 256,\n    \"output_bytes\": 1000000,\n    \"diagnostic_bytes\": 1000000,\n    \"transcript_bytes\": 1000000,\n    \"transcript_events\": 100,\n    \"result_bytes\": 1000000\n  }\n}\n",
    );
    write_file(
        &package.root,
        "rules/application.quotas.json",
        "{\n  \"schema\": \"topaz.lispex-application-quotas/v1\",\n  \"concurrent_evaluations\": 2,\n  \"queued_evaluations\": 2,\n  \"total_evaluations\": 16,\n  \"aggregate_input_bytes\": 65536,\n  \"aggregate_result_bytes\": 16000000,\n  \"aggregate_output_bytes\": 16000000,\n  \"aggregate_transcript_bytes\": 16000000,\n  \"aggregate_safety_fuel\": 16000000000,\n  \"prepared_bytes\": 1000000,\n  \"wall_millis\": 5000\n}\n",
    );
    let project = topaz_package::Project::load(&package.root).expect("project loads");
    topaz_lispex_product::write_locked_package(&project).expect("application package locks");
    let root_arg = package.root.to_string_lossy().into_owned();
    let target = package_target(Some(&root_arg), Some(LangVersion::V5_18), true)
        .expect("activated package target");
    assert_eq!(target.generated_std_modules.len(), 2);
    let resolved = resolve_package_target(&target);
    assert!(
        resolved.diagnostics.is_empty(),
        "{:?}",
        resolved.diagnostics
    );
    assert!(
        resolved
            .modules
            .iter()
            .filter(|module| module.is_generated_std)
            .map(|module| module.identity.as_str())
            .eq(["std.lispex", "std.lispex.rules"])
    );
    let units = unit_modules(&resolved);
    let checked = topaz_check::check_unit_typed_with_version(&units, target.version);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let targets = checked
        .typed_hir
        .as_ref()
        .expect("typed application")
        .calls
        .iter()
        .filter_map(|call| call.target_identity.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(targets, ["topaz.lispex-rule-handle/v1:환불"]);
    let self_product = compile_self_package_product(&target, None, "check")
        .expect("self compiler accepts generated application modules");
    assert_eq!(self_product.status(), "completed");
    let self_targets = self_product
        .typed()
        .calls
        .iter()
        .filter_map(|call| call.target_identity.as_deref())
        .filter(|identity| identity.starts_with("topaz.lispex-rule-handle/v1:"))
        .collect::<Vec<_>>();
    assert_eq!(self_targets, ["topaz.lispex-rule-handle/v1:환불"]);
    let self_plan = self_lispex_application_plan(&target, &self_product)
        .expect("self product derives the exact application plan")
        .expect("self product carries an application plan");
    assert_eq!(
        self_plan.reachable_rules,
        BTreeSet::from(["환불".to_string()])
    );
    assert!(matches!(
        package_harness_with_lispex(&target, Some(&self_plan)),
        HostHarness::PackageFsLispex { .. }
    ));
    let lowered = topaz_lower::lower_checked(&resolved, &checked).expect("application lowers");
    let lowered_targets = lowered
        .calls
        .iter()
        .filter_map(|call| call.target_identity.as_deref())
        .filter(|identity| identity.starts_with("topaz.lispex-rule-handle/v1:"))
        .collect::<Vec<_>>();
    assert_eq!(lowered_targets, ["topaz.lispex-rule-handle/v1:환불"]);
    assert!(
        lowered
            .runtime
            .leaves
            .iter()
            .any(|leaf| { leaf.identity == "topaz.lispex-rule-handle/v1:환불" })
    );
    assert_eq!(run_package_target(&target, false, &[]), ExitCode::SUCCESS);
    assert_eq!(run_self_package(&target, &[], false), ExitCode::SUCCESS);
    assert!(
        resolve_and_lower_package_for_service(&target).is_err(),
        "HTTP service must reject a reached Lispex application before output"
    );
    assert!(
        resolve_and_lower_package_for_web(&target, Backend::Boxed).is_err(),
        "Web targets must reject a reached Lispex application before output"
    );
    assert!(
        resolve_and_emit_python_package(&target, false).is_err(),
        "Python must reject a reached Lispex application before output"
    );

    let generated = resolve_and_lower_package_with_report(
        &target,
        false,
        Backend::Native,
        None,
        "emit",
        "rust",
    )
    .expect("checked application emits Rust");
    let plan = generated
        .lispex_application
        .as_ref()
        .expect("checked source carries one application plan");
    assert_eq!(plan.reachable_rules, BTreeSet::from(["환불".to_string()]));
    let output = package.root.join("generated-native");
    scaffold_crate(
        &output,
        &generated.text,
        package_harness_with_lispex(&target, Some(plan)),
    )
    .expect("selected application scaffolds its conditional native closure");
    let manifest = fs::read_to_string(output.join("Cargo.toml")).expect("generated manifest");
    assert!(manifest.contains("topaz_lispex_embed"), "{manifest}");
    assert!(
        output
            .join("lispex/component/lispex-embed-evaluator.wasm")
            .is_file()
    );
    assert!(
        output
            .join("lispex/RUNTIME-THIRD-PARTY-NOTICES.txt")
            .is_file()
    );
    let vendored_adapter =
        fs::read_to_string(output.join("vendor/crates/topaz_lispex_embed/src/lib.rs"))
            .expect("vendored bounded adapter");
    assert!(!vendored_adapter.contains("lispex-evaluator/rust-vm-current-profile"));
    assert!(
        !output
            .join("vendor/crates/topaz_lispex_embed/src/full_artifact.rs")
            .exists()
    );
}

#[test]
fn future_v519_complete_profile_scaffolds_an_offline_native_application() {
    const LIMITS: &str = "{\n  \"schema\": \"topaz.lispex-embed-limits/v1\",\n  \"prepare\": {\"raw_source_bytes\": 4096, \"prepare_work\": 1000000, \"logical_allocation\": 1000000, \"syntax_depth\": 64},\n  \"evaluate\": {\"canonical_input_bytes\": 4096, \"eval_work\": 1000000, \"logical_allocation\": 1000000, \"semantic_frames\": 1000, \"traversal_depth\": 256, \"output_bytes\": 1000000, \"diagnostic_bytes\": 1000000, \"transcript_bytes\": 1000000, \"transcript_events\": 100, \"result_bytes\": 1000000}\n}\n";
    const QUOTAS: &str = "{\"schema\":\"topaz.lispex-application-quotas/v1\",\"concurrent_evaluations\":2,\"queued_evaluations\":2,\"total_evaluations\":4,\"aggregate_input_bytes\":16,\"aggregate_result_bytes\":4000000,\"aggregate_output_bytes\":4000000,\"aggregate_transcript_bytes\":4000000,\"aggregate_safety_fuel\":4000000000,\"prepared_bytes\":1000000,\"wall_millis\":60000}\n";
    const SOURCE: &str = r#"import std.lispex { LispexSettlement, defaultLimits, evaluate, valueFromCanonical }
import std.lispex.rules as rules

export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    let bytes = match Bytes.fromArray([0]) {
        case Ok(value) => value
        case Err(error) => return Err(error)
    }
    let input = match valueFromCanonical(bytes) {
        case Ok(value) => value
        case Err(_) => return Err("canonical input refused")
    }
    match evaluate(rules.refund(), input, defaultLimits(rules.refund())) {
        case Ok(Complete(_)) => Ok(0)
        case Ok(_) => Err("complete-profile evaluation did not complete")
        case Err(_) => Err("complete-profile evaluation faulted")
    }
}
"#;
    let bounded_manifest = r#"[package]
name = "native_complete_profile"
version = "0.1.0"
language = "5.18"
entry = "main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.18"

[lispex]
profile = "lispex/r7rs-rule-embedded-core/1"
application = "topaz/lispex-decision-application/1"
application_quotas = "rules/application.quotas.json"

[[lispex.rule]]
name = "refund"
source = "rules/refund.lspx"
limits = "rules/refund.limits.json"
"#;

    let bounded = temp_package("native-complete-profile-source");
    write_file(&bounded.root, "topaz.toml", bounded_manifest);
    write_file(&bounded.root, "main.tpz", SOURCE);
    write_file(
        &bounded.root,
        "rules/refund.lspx",
        "(if (< 10 15) \"allow\" \"deny\")\n",
    );
    write_file(&bounded.root, "rules/refund.limits.json", LIMITS);
    write_file(&bounded.root, "rules/application.quotas.json", QUOTAS);
    let bounded_project =
        topaz_package::Project::load(&bounded.root).expect("bounded source project");
    topaz_lispex_product::write_locked_package(&bounded_project)
        .expect("bounded source package locks");
    let bounded_root = bounded.root.to_string_lossy().into_owned();
    let bounded_target = package_target(Some(&bounded_root), Some(LangVersion::V5_18), true)
        .expect("bounded source package target");
    let generated = resolve_and_lower_package_with_report(
        &bounded_target,
        false,
        Backend::Native,
        None,
        "emit",
        "rust",
    )
    .expect("shared 5.18 semantics emit the application call graph");

    let complete = temp_package("native-complete-profile-product");
    let future_manifest = bounded_manifest
        .replace("5.18", "5.20")
        .replace(
            topaz_package::LISPEX_BOUNDED_PROFILE_ID,
            topaz_package::LISPEX_COMPLETE_PROFILE_ID,
        )
        .replace(
            topaz_package::LISPEX_APPLICATION_PROFILE_ID,
            topaz_package::LISPEX_COMPLETE_APPLICATION_PROFILE_ID,
        );
    write_file(&complete.root, "topaz.toml", &future_manifest);
    write_file(&complete.root, "main.tpz", SOURCE);
    write_file(
        &complete.root,
        "rules/refund.lspx",
        "(if (< 10 15) \"allow\" \"deny\")\n",
    );
    write_file(&complete.root, "rules/refund.limits.json", LIMITS);
    write_file(&complete.root, "rules/application.quotas.json", QUOTAS);
    let mut manifest =
        topaz_package::parse_manifest(bounded_manifest).expect("bounded future-profile template");
    manifest.package.language = topaz_package::LISPEX_COMPLETE_APPLICATION_LANGUAGE;
    manifest
        .dependencies
        .get_mut("std")
        .expect("std dependency")
        .version = Some(topaz_package::LISPEX_COMPLETE_APPLICATION_STD_VERSION.into());
    let lispex = manifest.lispex.as_mut().expect("Lispex declaration");
    lispex.profile = topaz_package::LISPEX_COMPLETE_PROFILE_ID.into();
    lispex.application = Some(topaz_package::LISPEX_COMPLETE_APPLICATION_PROFILE_ID.into());
    let complete_project = topaz_package::Project {
        root: complete.root.clone(),
        manifest_text: future_manifest,
        manifest,
    };
    topaz_lispex_product::write_locked_package(&complete_project)
        .expect("complete-profile package locks privately");
    let plan = topaz_lispex_product::checked_application_plan(
        &complete_project,
        ["topaz.lispex-rule-handle/v1:refund"],
    )
    .expect("complete-profile checked plan");
    let read_roots = Vec::new();
    let write_roots = Vec::new();
    let policies = Vec::new();
    let harness = HostHarness::PackageFsLispex {
        read_roots: &read_roots,
        write_roots: &write_roots,
        extern_replay_jsonl: "",
        extern_sandbox_policies: &policies,
        plan: &plan,
    };
    let output = complete.root.join("generated-native");
    scaffold_crate(&output, &generated.text, harness).expect("complete-profile native scaffold");
    let manifest = fs::read_to_string(output.join("Cargo.toml")).expect("native manifest");
    assert!(manifest.contains("\"full-profile-contract\""), "{manifest}");
    assert!(
        output
            .join("vendor/crates/topaz_lispex_embed/src/full_artifact.rs")
            .is_file()
    );
    assert!(
        output
            .join("lispex/component/lispex-full-embed-evaluator.wasm")
            .is_file()
    );
    assert!(
        !output
            .join("lispex/component/lispex-embed-evaluator.wasm")
            .exists()
    );
}

#[test]
fn extern_manifest_module_resolves_to_virtual_surface_then_replay_gate() {
    let package = temp_package("extern-virtual");
    write_file(&package.root, "topaz.toml", &extern_manifest("extern_demo"));
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.image { resizePng }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    Ok(0)
}
"#,
    );

    let target = package_target_for(&package.root);
    let resolved = resolve_package_target(&target);
    assert!(
        resolved.diagnostics.is_empty(),
        "extern package resolves before replay gate: {:?}",
        resolved.diagnostics
    );
    let extern_module = resolved
        .modules
        .iter()
        .find(|m| m.identity == "host.image")
        .expect("declared extern module is in the resolved unit");
    assert!(extern_module.is_extern, "extern module is tagged");

    let units = unit_modules(&resolved);
    let checked = topaz_check::check_unit_with_version(&units, target.version);
    let codes: Vec<_> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(
        codes,
        [topaz_diag::extern_codes::REPLAY],
        "extern import publishes a typed surface, then stops at the replay gate: {:?}",
        checked.diagnostics
    );
}

#[test]
fn extern_replay_fixture_runs_and_builds_package() {
    let package = temp_package("extern-replay");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_manifest("extern_replay"),
    );
    write_file(
        &package.root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    let callable = match twice {
    case f: (int) -> int => true
    case _ => false
    }
    let answer = twice(21)
    if callable && answer == 42 { Ok(0) } else { Err("bad replay") }
}
"#,
    );

    let project = topaz_package::Project::load(&package.root).expect("project loads");
    let lock = project.render_lockfile().expect("extern lock renders");
    assert!(lock.contains("[[extern]]"), "{lock}");
    assert!(lock.contains("module = \"host.math\""), "{lock}");
    assert!(lock.contains("replay_hash = \"sha256:"), "{lock}");
    write_file(&package.root, "topaz.lock", &lock);
    topaz_package::Project::load(&package.root)
        .expect("project reloads")
        .verify_locked()
        .expect("extern replay lock verifies");

    let target = package_target_for_locked(&package.root, true);
    assert!(
        target.extern_replay_errors.is_empty(),
        "valid replay fixture loads: {:?}",
        target.extern_replay_errors
    );
    assert_eq!(
        run_package_target(&target, false, &[]),
        ExitCode::SUCCESS,
        "topaz run uses the extern replay fixture"
    );
    assert_eq!(
        test_package_target(&target, &[]),
        ExitCode::SUCCESS,
        "topaz test uses the extern replay fixture"
    );

    let out_dir = package.root.join("out");
    assert_eq!(
        build_package_target(
            &target,
            Some(&out_dir),
            false,
            true,
            false,
            Backend::Native,
            false,
            BuildTarget::Native,
            false,
            &[],
            None,
        ),
        ExitCode::SUCCESS,
        "topaz build --run falls back to boxed emit and uses the same replay fixture"
    );
}

#[test]
fn extern_replay_budget_limits_fail_run_and_build_run_deterministically() {
    let package = temp_package("extern-budget-tight");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_budget_manifest("extern_budget_tight", 2, 16),
    );
    write_file(
        &package.root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    let answer = twice(21)
    if answer == 42 { Ok(0) } else { Err("bad replay") }
}
"#,
    );

    let target = package_target_for(&package.root);
    assert!(
        target.extern_replay_errors.is_empty(),
        "budget-bound replay fixture loads: {:?}",
        target.extern_replay_errors
    );
    let err = target
        .extern_replay
        .call("host.math", "twice", &[topaz_value::Value::Int(21)])
        .unwrap_err();
    assert!(
        err.contains("extern replay fuel limit exceeded for `host.math.twice`"),
        "{err}"
    );
    assert_eq!(
        run_package_target(&target, false, &[]),
        ExitCode::FAILURE,
        "topaz run faults deterministically on an extern replay fuel budget"
    );

    let out_dir = package.root.join("out");
    assert_eq!(
        build_package_target(
            &target,
            Some(&out_dir),
            false,
            true,
            false,
            Backend::Native,
            false,
            BuildTarget::Native,
            false,
            &[],
            None,
        ),
        ExitCode::FAILURE,
        "topaz build --run faults through the same extern replay budget"
    );
}

#[test]
fn extern_replay_budget_limits_allow_run_and_build_run_at_large_budget() {
    let package = temp_package("extern-budget-large");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_budget_manifest("extern_budget_large", 1000, 65536),
    );
    write_file(
        &package.root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    let answer = twice(21)
    if answer == 42 { Ok(0) } else { Err("bad replay") }
}
"#,
    );

    let target = package_target_for(&package.root);
    assert!(
        target.extern_replay_errors.is_empty(),
        "large budget replay fixture loads: {:?}",
        target.extern_replay_errors
    );
    assert_eq!(
        run_package_target(&target, false, &[]),
        ExitCode::SUCCESS,
        "topaz run accepts replay within the declared budget"
    );

    let out_dir = package.root.join("out");
    assert_eq!(
        build_package_target(
            &target,
            Some(&out_dir),
            false,
            true,
            false,
            Backend::Native,
            false,
            BuildTarget::Native,
            false,
            &[],
            None,
        ),
        ExitCode::SUCCESS,
        "topaz build --run accepts replay within the declared budget"
    );
}

#[test]
fn load_extern_replay_bindings_attaches_sandbox_policy() {
    let package = temp_package("extern-policy");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_wasm_manifest("extern_policy"),
    );
    write_file(
        &package.root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    let answer = twice(21)
    if answer == 42 { Ok(0) } else { Err("bad replay") }
}
"#,
    );

    let target = package_target_for(&package.root);
    assert!(
        target.extern_replay_errors.is_empty(),
        "valid policy-bound replay fixture loads: {:?}",
        target.extern_replay_errors
    );
    assert_eq!(target.extern_sandbox_policies.len(), 1);
    let policy = target
        .extern_replay
        .sandbox_policy("host.math")
        .expect("manifest sandbox policy is visible in the replay store");
    assert_eq!(policy.kind, topaz_value::ExternSandboxKind::Wasm);
    assert_eq!(
        policy.artifact_path.as_deref(),
        Some("artifacts/host-math.wasm")
    );
    assert_eq!(policy.fuel, Some(1000));
    assert_eq!(policy.memory_bytes, Some(65536));
    let replayed = target
        .extern_replay
        .call("host.math", "twice", &[topaz_value::Value::Int(21)])
        .expect("wasm-with-artifact policy still runs through deterministic replay");
    assert_eq!(topaz_value::render(&replayed), "42");
}

#[cfg(unix)]
#[test]
fn package_target_rejects_linked_extern_replay_fixture() {
    use std::os::unix::fs::symlink;

    let package = temp_package("extern-replay-symlink");
    let outside = temp_package("extern-replay-symlink-outside");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_manifest("extern_replay_symlink"),
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    if twice(21) == 42 { Ok(0) } else { Err("bad replay") }
}
"#,
    );
    write_file(
        &outside.root,
        "host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    fs::create_dir_all(package.root.join("replay")).expect("replay directory");
    symlink(
        outside.root.join("host-math.jsonl"),
        package.root.join("replay/host-math.jsonl"),
    )
    .expect("linked replay fixture");

    let target = package_target_for(&package.root);
    let error = target
        .extern_replay_errors
        .get("host.math")
        .expect("linked replay fixture has an admission error");
    assert!(
            error.contains(
                "extern module `host.math` replay fixture `replay/host-math.jsonl` must not contain a symlink"
            ),
            "{error}"
        );
    assert!(
        target
            .extern_replay
            .call("host.math", "twice", &[topaz_value::Value::Int(21)])
            .is_err(),
        "bytes outside the package root must not become executable replay input"
    );
}

#[test]
fn emitted_host_init_embeds_extern_sandbox_policy() {
    let package = temp_package("extern-policy-init");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_wasm_manifest("extern_policy_init"),
    );
    write_file(
        &package.root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    if twice(21) == 42 { Ok(0) } else { Err("bad replay") }
}
"#,
    );

    let target = package_target_for(&package.root);
    let harness = main_harness(package_harness(&target));
    assert!(
        harness.contains("ExternReplayStore::parse_jsonl_with_policies"),
        "{harness}"
    );
    assert!(harness.contains("ExternSandboxPolicy::new"), "{harness}");
    assert!(harness.contains("ExternSandboxKind::Wasm"), "{harness}");
    assert!(harness.contains("\"host.math\""), "{harness}");
    assert!(
        harness.contains("\"artifacts/host-math.wasm\""),
        "{harness}"
    );
    assert!(harness.contains("Some(1000)"), "{harness}");
    assert!(harness.contains("Some(65536)"), "{harness}");
}

#[test]
fn emitted_host_init_keeps_v54_externs_replay_sandboxed() {
    let package = temp_package("extern-replay-sandbox-init");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_wasm_manifest("extern_replay_sandbox_init"),
    );
    write_file(
        &package.root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":[{"$":"int","value":"21"}],"result":{"$":"int","value":"42"}}
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    if twice(21) == 42 { Ok(0) } else { Err("bad replay") }
}
"#,
    );

    let target = package_target_for(&package.root);
    let harness = main_harness(package_harness(&target));
    assert!(
        harness.contains("ExternReplayStore::parse_jsonl_with_policies"),
        "{harness}"
    );
    assert!(
        !harness.contains("include_bytes!"),
        "v5.4 emitted harness must not embed a live artifact loader:\n{harness}"
    );
    assert!(
        !harness.contains("extern_live"),
        "v5.4 emitted harness must not mention an experimental live backend:\n{harness}"
    );
}

#[test]
fn compiler_workspace_lockfile_has_only_the_approved_runtime_registry_closure() {
    let lock = include_str!("../../../../Cargo.lock");
    let mut registry = lock
        .split("[[package]]")
        .filter(|package| package.contains("source = \"registry+"))
        .filter_map(|package| {
            let field = |prefix: &str| {
                package.lines().find_map(|line| {
                    line.trim()
                        .strip_prefix(prefix)
                        .and_then(|value| value.strip_suffix('"'))
                })
            };
            Some(format!(
                "{}@{}",
                field("name = \"")?,
                field("version = \"")?
            ))
        })
        .collect::<Vec<_>>();
    registry.sort();
    // This is the exact registry closure admitted by the HTTP service host,
    // the bounded Lispex evaluator adapter and suites, and the CLI's
    // no-replace filesystem primitive. Keep versions here so a lockfile
    // change cannot silently widen or replace an admitted external input.
    let mut expected = vec![
        "addr2line@0.25.1",
        "allocator-api2@0.2.21",
        "android_system_properties@0.1.5",
        "anyhow@1.0.104",
        "arbitrary@1.4.2",
        "async-trait@0.1.91",
        "atomic-waker@1.1.2",
        "autocfg@1.5.1",
        "bitflags@2.13.1",
        "block-buffer@0.10.4",
        "bumpalo@3.20.3",
        "bytes@1.12.1",
        "cc@1.4.0",
        "cfg-if@1.0.4",
        "chrono@0.4.45",
        "cobs@0.3.0",
        "core-foundation-sys@0.8.7",
        "cpufeatures@0.2.17",
        "cranelift-assembler-x64-meta@0.125.4",
        "cranelift-assembler-x64@0.125.4",
        "cranelift-bforest@0.125.4",
        "cranelift-bitset@0.125.4",
        "cranelift-codegen-meta@0.125.4",
        "cranelift-codegen-shared@0.125.4",
        "cranelift-codegen@0.125.4",
        "cranelift-control@0.125.4",
        "cranelift-entity@0.125.4",
        "cranelift-frontend@0.125.4",
        "cranelift-isle@0.125.4",
        "cranelift-native@0.125.4",
        "cranelift-srcgen@0.125.4",
        "crc32fast@1.5.0",
        "crypto-common@0.1.7",
        "digest@0.10.7",
        "dyn-clone@1.0.20",
        "either@1.17.0",
        "embedded-io@0.4.0",
        "embedded-io@0.6.1",
        "equivalent@1.0.2",
        "errno@0.3.14",
        "fallible-iterator@0.3.0",
        "find-msvc-tools@0.1.9",
        "foldhash@0.1.5",
        "futures-channel@0.3.33",
        "futures-core@0.3.33",
        "futures-executor@0.3.33",
        "futures-io@0.3.33",
        "futures-macro@0.3.33",
        "futures-sink@0.3.33",
        "futures-task@0.3.33",
        "futures-util@0.3.33",
        "futures@0.3.33",
        "generic-array@0.14.7",
        "gimli@0.32.3",
        "hashbrown@0.15.5",
        "hashbrown@0.17.1",
        "heck@0.5.0",
        "http-body-util@0.1.4",
        "http-body@1.1.0",
        "http@1.4.2",
        "httparse@1.10.1",
        "httpdate@1.0.3",
        "hyper-util@0.1.20",
        "hyper@1.11.0",
        "iana-time-zone-haiku@0.1.2",
        "iana-time-zone@0.1.65",
        "indexmap@2.14.0",
        "itertools@0.14.0",
        "itoa@1.0.18",
        "js-sys@0.3.103",
        "leb128fmt@0.1.0",
        "libc@0.2.189",
        "libm@0.2.16",
        "linux-raw-sys@0.12.1",
        "log@0.4.33",
        "mach2@0.4.3",
        "memchr@2.8.3",
        "memfd@0.6.5",
        "mio@1.2.2",
        "num-bigint@0.4.6",
        "num-integer@0.1.46",
        "num-traits@0.2.19",
        "object@0.37.3",
        "once_cell@1.21.4",
        "pastey@0.2.3",
        "pin-project-lite@0.2.17",
        "postcard@1.1.3",
        "proc-macro2@1.0.107",
        "pulley-interpreter@38.0.4",
        "pulley-macros@38.0.4",
        "quote@1.0.47",
        "ref-cast-impl@1.0.26",
        "ref-cast@1.0.26",
        "regalloc2@0.13.5",
        "rmcp@1.5.0",
        "rustc-hash@2.1.3",
        "rustix@1.1.4",
        "rustversion@1.0.23",
        "ryu@1.0.23",
        "schemars@1.2.2",
        "schemars_derive@1.2.2",
        "semver@1.0.28",
        "serde@1.0.229",
        "serde_core@1.0.229",
        "serde_derive@1.0.229",
        "serde_derive_internals@0.30.0",
        "serde_json@1.0.140",
        "sha2@0.10.9",
        "shlex@2.0.1",
        "signal-hook-registry@1.4.8",
        "slab@0.4.12",
        "smallvec@1.15.2",
        "socket2@0.6.5",
        "stable_deref_trait@1.2.1",
        "syn@2.0.119",
        "syn@3.0.3",
        "target-lexicon@0.13.5",
        "termcolor@1.4.1",
        "thiserror-impl@2.0.19",
        "thiserror@2.0.19",
        "tokio-macros@2.7.2",
        "tokio-util@0.7.18",
        "tokio@1.53.1",
        "tracing-attributes@0.1.31",
        "tracing-core@0.1.36",
        "tracing@0.1.44",
        "typenum@1.20.1",
        "unicode-ident@1.0.24",
        "version_check@0.9.5",
        "wasi@0.11.1+wasi-snapshot-preview1",
        "wasm-bindgen-macro-support@0.2.126",
        "wasm-bindgen-macro@0.2.126",
        "wasm-bindgen-shared@0.2.126",
        "wasm-bindgen@0.2.126",
        "wasm-encoder@0.239.0",
        "wasmparser@0.239.0",
        "wasmprinter@0.239.0",
        "wasmtime-environ@38.0.4",
        "wasmtime-internal-cranelift@38.0.4",
        "wasmtime-internal-fiber@38.0.4",
        "wasmtime-internal-jit-debug@38.0.4",
        "wasmtime-internal-jit-icache-coherence@38.0.4",
        "wasmtime-internal-math@38.0.4",
        "wasmtime-internal-slab@38.0.4",
        "wasmtime-internal-unwinder@38.0.4",
        "wasmtime-internal-versioned-export-macros@38.0.4",
        "wasmtime@38.0.4",
        "winapi-util@0.1.11",
        "windows-core@0.62.2",
        "windows-implement@0.60.2",
        "windows-interface@0.59.3",
        "windows-link@0.2.1",
        "windows-result@0.4.1",
        "windows-strings@0.5.1",
        "windows-sys@0.60.2",
        "windows-sys@0.61.2",
        "windows-targets@0.53.5",
        "windows_aarch64_gnullvm@0.53.1",
        "windows_aarch64_msvc@0.53.1",
        "windows_i686_gnu@0.53.1",
        "windows_i686_gnullvm@0.53.1",
        "windows_i686_msvc@0.53.1",
        "windows_x86_64_gnu@0.53.1",
        "windows_x86_64_gnullvm@0.53.1",
        "windows_x86_64_msvc@0.53.1",
    ];
    expected.sort();
    assert_eq!(registry, expected, "unexpected registry dependency closure");
}

#[test]
fn invalid_extern_replay_fixture_reports_replay_code() {
    let package = temp_package("extern-invalid-replay");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_math_manifest("extern_invalid_replay"),
    );
    write_file(
        &package.root,
        "replay/host-math.jsonl",
        r#"{"module":"host.math","function":"twice","args":"not-array","result":{"$":"int","value":"42"}}
"#,
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.math { twice }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    Ok(0)
}
"#,
    );

    let target = package_target_for(&package.root);
    assert!(
        target
            .extern_replay_errors
            .get("host.math")
            .is_some_and(|e| e.contains("is invalid")),
        "invalid replay fixture is recorded on the extern module: {:?}",
        target.extern_replay_errors
    );
    let resolved = resolve_package_target(&target);
    assert!(
        resolved.diagnostics.is_empty(),
        "invalid replay is a checker gate, not a resolver failure: {:?}",
        resolved.diagnostics
    );
    let units = unit_modules(&resolved);
    let checked = topaz_check::check_unit_with_version(&units, target.version);
    let codes: Vec<_> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(
        codes,
        [topaz_diag::extern_codes::REPLAY],
        "invalid replay fixture blocks the extern import at TPZ5032: {:?}",
        checked.diagnostics
    );
    assert!(
        checked.diagnostics[0]
            .message
            .contains("invalid deterministic replay binding"),
        "diagnostic names the replay binding problem: {:?}",
        checked.diagnostics
    );
}

#[test]
fn leaked_extern_call_names_are_not_public_builtins() {
    for (i, leaked_name) in ["__topaz_extern_call", "ExternCall"].iter().enumerate() {
        let leaked_name = *leaked_name;
        let package = temp_package("extern-leak");
        let package_name = format!("extern_leak_{i}");
        write_file(&package.root, "topaz.toml", &basic_manifest(&package_name));
        write_file(
            &package.root,
            "main.tpz",
            &format!(
                r#"export function main(args: Array<string>, stdin: string) -> Result<int, string> {{
    let value = {leaked_name}(1)
    Ok(0)
}}
"#
            ),
        );

        let target = package_target_for(&package.root);
        let resolved = resolve_package_target(&target);
        assert!(
            resolved.diagnostics.is_empty(),
            "leaked name source resolves before closed-unit checking: {:?}",
            resolved.diagnostics
        );
        let units = unit_modules(&resolved);
        let checked = topaz_check::check_unit_with_version(&units, target.version);
        let codes: Vec<_> = checked
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        assert_eq!(
            codes,
            [topaz_diag::guard_codes::UNBOUND],
            "{leaked_name} must remain an ordinary unbound source callee: {:?}",
            checked.diagnostics
        );
        assert!(
            checked.diagnostics[0].message.contains(leaked_name),
            "unbound diagnostic names the attempted internal callee: {:?}",
            checked.diagnostics
        );
    }
}

#[test]
fn undeclared_extern_sibling_reports_manifest_decl_code() {
    let package = temp_package("extern-missing");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_manifest("extern_missing"),
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"import host.video { resizePng }
export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    Ok(0)
}
"#,
    );

    let target = package_target_for(&package.root);
    let resolved = resolve_package_target(&target);
    let codes: Vec<_> = resolved
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(
        codes,
        [topaz_diag::extern_codes::DECL],
        "extern namespace misses are manifest declaration errors: {:?}",
        resolved.diagnostics
    );
}

#[test]
fn extern_root_conflicts_with_physical_root_module() {
    let package = temp_package("extern-conflict");
    write_file(
        &package.root,
        "topaz.toml",
        &extern_manifest("extern_conflict"),
    );
    write_file(
        &package.root,
        "main.tpz",
        r#"export function main(args: Array<string>, stdin: string) -> Result<int, string> {
    Ok(0)
}
"#,
    );
    write_file(&package.root, "host.tpz", "export let value = 1\n");

    let root_arg = package.root.to_string_lossy().into_owned();
    assert!(
        package_target(Some(&root_arg), Some(LangVersion::V5_4), false).is_err(),
        "extern roots may not overlap physical root modules"
    );
}
