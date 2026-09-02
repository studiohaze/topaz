use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockPackage {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) hash: Option<String>,
    pub(crate) manifest_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LockExtern {
    pub(super) module: String,
    pub(super) hash: String,
    pub(super) abi_hash: String,
    pub(super) artifact_path: Option<String>,
    pub(super) sandbox: ExternSandboxKind,
    pub(super) fuel: Option<u64>,
    pub(super) memory_bytes: Option<u64>,
    pub(super) replay_hash: String,
}

pub(crate) struct ParsedLock {
    pub(super) packages: Vec<LockPackage>,
    pub(super) externs: Vec<LockExtern>,
    pub(super) lispex: Option<LispexLock>,
}
