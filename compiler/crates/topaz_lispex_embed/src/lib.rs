//! Exact installed product adapter for the bounded Lispex evaluator.
//!
//! The component and every admitted identity are compile-time constants. This
//! crate exposes no discovery or selection surface and never falls back to a
//! different evaluator.

mod application;
mod application_host;
mod artifact;
#[cfg(feature = "full-profile-contract")]
mod full_artifact;
mod limits;
mod protocol;
mod report;
mod runtime;
mod value_codec;
pub use application_host::{AdmittedApplicationRule, LispexApplicationHost};
pub use artifact::{
    ArtifactCategory, ArtifactError, ArtifactKind, ConsumerArtifact, ConsumerArtifactInspection,
    decode_artifact, encode_artifact, inspect_artifact, portable_core_for_artifact,
    verify_artifact, wrap_evaluate_artifact, wrap_prepare_artifact,
};
#[cfg(feature = "full-profile-contract")]
pub use full_artifact::{
    FULL_COMPONENT_ID, FULL_EVALUATOR_SHA256, FULL_FEATURE_SET_SHA256, FULL_LANGUAGE_PROFILE_ID,
    FULL_MODEL_ID, FULL_PROFILE_DENOMINATOR, FULL_PROFILE_ID, FullArtifactCategory,
    FullArtifactError, FullArtifactKind, FullConsumerArtifact, FullProfileDenominator,
    FullProfileRuleHandle, decode_full_artifact, inspect_full_artifact,
    load_full_profile_rule_handle, verify_full_artifact, wrap_full_evaluate_artifact,
    wrap_full_prepare_artifact,
};
pub use topaz_value::LispexApplicationRuleIdentity;

use num_bigint::BigInt;
use num_integer::Integer;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, OnceLock,
    atomic::{AtomicU8, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use topaz_value::value::{JsonNumber, JsonValue, json_parse, write_json_node};
use wasmtime::{Config, Engine, ExternType, Instance, Module, Store, UpdateDeadline};

pub const ADAPTER_ID: &str = "topaz.lispex-embed-adapter/5.17";
pub const COMPONENT_ID: &str = "lispex-embed-evaluator/1.20.0";
pub const EVALUATOR_SHA256: &str =
    "8ecc89c1c0b6e83e75f2be23951e99ad9d3405a179b8f062a5cf76360fb16190";
pub const COMPONENT_MANIFEST_SHA256: &str =
    "b2669402bc376ada4e9aa4c24daaf4bc687ef43aeea80c3959a81d55d8946ff8";
pub const CAPABILITY_MANIFEST_V1_SHA256: &str =
    "32f2aa5906a09a2358526b00a51264ef9325c138edcdb40dc01d846d9a0ed66d";
pub const CAPABILITY_MANIFEST_V2_SHA256: &str =
    "22debb0569cabc51c8848d5a089e1b3c0b88e6c606525b92b5ce060e40f1d900";
pub const METER_MANIFEST_V2_SHA256: &str =
    "0e7bfe9be2ea155d7f9d8b5f83f49b0df4451710891769b77423e1c7cecbf657";
pub const METER_MANIFEST_V3_SHA256: &str =
    "f0368f5f0b93876575bbff016d5ae7ba384c6b4bafef6c9a872fc396c71cc16a";
pub const RESOURCE_PROFILES_SHA256: &str =
    "3dcd098d67ace9d9a01e38eb52b469a6924a05d9fe88eb93130415b9df041501";
pub const OBSERVATION_CONTRACT_SHA256: &str =
    "efb521008ae1077163b55bcb0f77e03fa586bd047ead2ee9b7f5fa25e4a6c7f8";
pub const OBSERVATION_RESULT_SHA256: &str =
    "36433446ca7b0ab99b222764c9a3c148b1fe29117e99244a8c0168bbc2935dee";
pub const DIAGNOSTIC_PHASE_ALIAS_SHA256: &str =
    "187b32e78dd37af083b6394644f77aa5bd48700b4d503db9d045119268350c9d";
pub const GUEST_FAULT_BOUNDARY_SHA256: &str =
    "5586ceb6417bfcf3dcc97ded5bcd926ebaac8692cbde95ca24edf8abc57e3a80";
pub const TOPLEVEL_OUTPUT_V2_SHA256: &str =
    "5711035b5ed4fe89dbbf6406686781b02ee678eb48ffa00404db59571b2aa9fe";
pub const INTERACTIVE_OUTPUT_SHA256: &str =
    "82b08d1be54ce7db3bc673daadefd18192249df6f1117d3d02347dd2bb097dcc";
pub const PROFILE_TOMBSTONES_SHA256: &str =
    "8d35394b1529a39f9693826077ede980905ee3e36182eec583e3fe3e025f560e";
pub const PROFILE_ID: &str = "lispex/r7rs-rule-embedded-core/1";
pub const BOUNDED_APPLICATION_COMPATIBILITY_PROFILE_ID: &str = PROFILE_ID;
pub const MODEL_ID: &str = "lispex-vm-meter/1";
pub const ABI_ID: &str = "lispex.embed-wasm-abi/v1";
pub const VALUE_CODEC_ID: &str = "lispex.embed-value/v1";
pub const CONTRACT_ID: &str = "topaz-lispex-embedding-contract/v1.2";
pub const CONTRACT_MANIFEST_SHA256: &str =
    "b6dd1d97951f9e7165c6a56c9a678be1fea27793ba55e0c7487f42d26be23b83";
pub const RUNTIME_ID: &str = "wasmtime/38.0.4";
pub const RUNTIME_POLICY_ID: &str = "topaz.lispex-embedding-runtime/v0";
pub const RUNTIME_POLICY_SHA256: &str =
    "9f326e79b2d166ec3dbf454bf758799a7e7a1625dbacb9e9cd8c83154e0d1ffb";
/// SHA-256 of the provider delivery manifest admitted with the embedded component.
pub const PROVIDER_INPUT_SHA256: &str =
    "298c60e1a75025853be9efec0617c5d0482a69f2ffacb9f5482863cc51495e36";
pub const PROVIDER_VERIFICATION_SHA256: &str =
    "316dd584bc65ae2018dc3209430ed8e495eb60ffb4d02482b07b5356ca040747";
pub const INTAKE_DISPOSITION_SHA256: &str =
    "e3f0716289e18a62b1c4587643a3858c035a5554f1fe3298c4f9c337aeae755e";
pub const CONTRACT_PINS_SHA256: &str =
    "6261c186f8ba082c289d49ffba331ef742ac2cd8d918efa9f394759bd15e57e1";
pub const LIMITS_SCHEMA: &str = "topaz.lispex-embed-limits/v1";
pub const APPLICATION_QUOTAS_SCHEMA: &str = "topaz.lispex-application-quotas/v1";
pub const REPORT_SCHEMA: &str = "topaz.lispex-embed-run-report/v4";
pub const INFO_SCHEMA: &str = "topaz.lispex-embed-info/v4";
pub const RELEASE_AUTHORITY: bool = true;
pub const ABI_VERSION: u32 = 0x0001_0000;
// This ceiling exceeds admitted worst-case evaluation cost while bounding malformed workloads.
pub const SAFETY_FUEL: u64 = 1_000_000_000;
pub const LIMITS_FILE_MAX_BYTES: u64 = 65_536;
pub const MAX_CANONICAL_VALUE_BYTES: usize = 1_000_000;
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub const TARGET: &str = "aarch64-apple-darwin";
#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
pub const TARGET: &str = "x86_64-apple-darwin";
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub const TARGET: &str = "aarch64-unknown-linux-gnu";
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub const TARGET: &str = "x86_64-unknown-linux-gnu";
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
pub const TARGET: &str = "x86_64-pc-windows-msvc";
#[cfg(not(any(
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "macos"),
    all(target_arch = "aarch64", target_os = "linux"),
    all(target_arch = "x86_64", target_os = "linux"),
    all(target_arch = "x86_64", target_os = "windows")
)))]
pub const TARGET: &str = "unsupported-target";

#[cfg(all(feature = "workspace-component", feature = "managed-product-component"))]
compile_error!("select only one Lispex evaluator component source");
#[cfg(feature = "workspace-component")]
const EVALUATOR_BYTES: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.20.0/payload/lispex-embed-evaluator.wasm"
);
#[cfg(all(
    not(feature = "workspace-component"),
    feature = "managed-product-component",
    not(feature = "full-profile-contract")
))]
const EVALUATOR_BYTES: &[u8] =
    include_bytes!("../../../../lispex/component/lispex-embed-evaluator.wasm");
#[cfg(all(
    not(feature = "workspace-component"),
    feature = "managed-product-component",
    feature = "full-profile-contract"
))]
const EVALUATOR_BYTES: &[u8] = &[];
#[cfg(all(
    not(feature = "workspace-component"),
    not(feature = "managed-product-component")
))]
compile_error!("select exactly one Lispex evaluator component source");
pub use application::*;
pub use limits::*;
pub use report::info_json;
pub use runtime::*;
pub use value_codec::{LispexValue, LispexValueError, sha256_hex, validate_value};

#[cfg(test)]
mod tests;
