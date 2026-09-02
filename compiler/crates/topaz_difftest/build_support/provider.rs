use std::collections::{BTreeMap, BTreeSet};

use topaz_resolve::{FileProvider, InMemoryProvider};

pub(super) struct ExternReplayProvider {
    inner: InMemoryProvider,
    extern_files: BTreeMap<String, String>,
    extern_namespaces: BTreeSet<String>,
    replay_errors: BTreeMap<String, String>,
}

impl ExternReplayProvider {
    pub(super) fn new() -> Self {
        Self {
            inner: InMemoryProvider::new(),
            extern_files: BTreeMap::new(),
            extern_namespaces: BTreeSet::new(),
            replay_errors: BTreeMap::new(),
        }
    }

    pub(super) fn add_file(&mut self, path: &'static str, source: &'static str) {
        self.inner.add_file(path, source);
    }

    pub(super) fn add_extern_file(
        &mut self,
        identity: &'static str,
        path: &'static str,
        source: &'static str,
        replay_error: Option<&'static str>,
    ) {
        self.inner.add_file(path, source);
        self.extern_files
            .insert(path.to_string(), identity.to_string());
        if let Some((root, _)) = identity.split_once('.') {
            self.extern_namespaces.insert(root.to_string());
        }
        if let Some(error) = replay_error {
            self.replay_errors
                .insert(identity.to_string(), error.to_string());
        }
    }
}

impl FileProvider for ExternReplayProvider {
    fn read(&self, path: &str) -> topaz_resolve::SourceRead {
        self.inner.read(path)
    }

    fn is_extern_file(&self, path: &str) -> bool {
        self.extern_files.contains_key(path)
    }

    fn is_extern_namespace(&self, identity: &str) -> bool {
        self.extern_namespaces
            .iter()
            .any(|ns| identity == ns || identity.starts_with(&format!("{ns}.")))
    }

    fn extern_replay_error(&self, identity: &str) -> Option<String> {
        self.replay_errors.get(identity).cloned()
    }

    fn read_directory(&self, dir: &str) -> topaz_resolve::DirectoryRead {
        self.inner.read_directory(dir)
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        self.inner.physical_id(path)
    }
}
