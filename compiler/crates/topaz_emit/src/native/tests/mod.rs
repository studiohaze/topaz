//! Native-emitter coverage grouped around hybrid, scalar, and boxed boundaries.
//! Shared lowering helpers stay local so the production native facade remains
//! free of test-only construction paths.

use std::collections::{BTreeMap, BTreeSet};

use topaz_resolve::{FileProvider, InMemoryProvider, resolve};

use super::*;
use crate::EmitErrorKind;

fn lowered(unit: &topaz_resolve::ResolveOutput, typed: Option<TypedUnit>) -> LoweredUnit {
    let mut lowered =
        topaz_lower::lower_resolved_compat(unit).expect("test unit lowers to emission IR");
    lowered.typed = typed;
    lowered
}

fn ast_top_level_function(
    statement: &topaz_syntax::ast::Stmt,
) -> Option<&topaz_syntax::ast::FunctionDecl> {
    use topaz_syntax::ast::StmtKind;
    match &statement.kind {
        StmtKind::Function(declaration) => Some(declaration),
        StmtKind::Export(inner) => match &inner.kind {
            StmtKind::Function(declaration) => Some(declaration),
            _ => None,
        },
        _ => None,
    }
}

struct ExternTestProvider {
    inner: InMemoryProvider,
    extern_files: BTreeMap<String, String>,
    extern_namespaces: BTreeSet<String>,
}

impl ExternTestProvider {
    fn new() -> Self {
        Self {
            inner: InMemoryProvider::new(),
            extern_files: BTreeMap::new(),
            extern_namespaces: BTreeSet::new(),
        }
    }

    fn add_file(&mut self, path: &'static str, source: &'static str) {
        self.inner.add_file(path, source);
    }

    fn add_extern_file(
        &mut self,
        identity: &'static str,
        path: &'static str,
        source: &'static str,
    ) {
        self.inner.add_file(path, source);
        self.extern_files
            .insert(path.to_string(), identity.to_string());
        if let Some((root, _)) = identity.split_once('.') {
            self.extern_namespaces.insert(root.to_string());
        }
    }
}

impl FileProvider for ExternTestProvider {
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

    fn read_directory(&self, dir: &str) -> topaz_resolve::DirectoryRead {
        self.inner.read_directory(dir)
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        self.inner.physical_id(path)
    }
}

mod boxed;
mod hybrid;
mod scalar;
