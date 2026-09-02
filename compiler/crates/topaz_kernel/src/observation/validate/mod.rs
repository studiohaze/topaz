//! Admission checks for a completed observation bundle.
//! Row schemas are validated before cross-reference rules, keeping structural
//! decoding separate from relationships among observation files.

use super::bundle::*;
use super::*;

mod cross_reference;
pub(super) mod schema;

use cross_reference::validate_cross_references;
use schema::{array_field, exact_fields, unsigned_field, validate_schema_rows};
use schema::{string_field, validate_schema_registry};

impl ObservationBundle {
    /// Admits schemas and cross-file references for every member of the bundle.
    pub fn validate(&self) -> Result<(), String> {
        validate_schema_registry()?;
        let files = self
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect::<BTreeMap<_, _>>();
        if files.len() != self.files.len() {
            return Err("observation bundle contains a duplicate path".to_string());
        }
        let manifest = files
            .get("topaz-observation.json")
            .ok_or_else(|| "observation manifest is missing".to_string())?;
        if manifest.schema != BUNDLE_SCHEMA {
            return Err("observation manifest member has the wrong schema".to_string());
        }
        let manifest_values = crate::canonical::validate(&manifest.bytes, false)?;
        let manifest_value = &manifest_values[0];
        exact_fields(manifest_value, &["files", "rootDigest", "schema"])?;
        if string_field(manifest_value, "schema")? != BUNDLE_SCHEMA {
            return Err("observation manifest schema is invalid".to_string());
        }
        let listed = array_field(manifest_value, "files")?;
        let mut listed_paths = BTreeSet::new();
        let mut previous = None::<String>;
        for entry in listed {
            exact_fields(entry, &["byteLength", "path", "schema", "sha256"])?;
            let path = string_field(entry, "path")?.to_string();
            if previous.as_ref().is_some_and(|value| value >= &path) {
                return Err("observation manifest paths are not strictly sorted".to_string());
            }
            previous = Some(path.clone());
            if !listed_paths.insert(path.clone()) {
                return Err("observation manifest repeats a path".to_string());
            }
            let file = files
                .get(path.as_str())
                .ok_or_else(|| format!("observation member `{path}` is missing"))?;
            if file.schema != string_field(entry, "schema")? {
                return Err(format!("observation member `{path}` schema drifted"));
            }
            if unsigned_field(entry, "byteLength")? != file.bytes.len() as u64 {
                return Err(format!("observation member `{path}` size drifted"));
            }
            if string_field(entry, "sha256")? != sha256(&file.bytes) {
                return Err(format!("observation member `{path}` digest drifted"));
            }
        }
        let actual_paths = files
            .keys()
            .filter(|path| **path != "topaz-observation.json")
            .map(|path| (*path).to_string())
            .collect::<BTreeSet<_>>();
        if actual_paths != listed_paths {
            return Err("observation manifest file set is not exact".to_string());
        }
        for required in [
            "request.json",
            "response.json",
            "provenance.json",
            "source-set.jsonl",
            "tokens.jsonl",
            "ast.jsonl",
            "resolved.jsonl",
            "diagnostics.jsonl",
        ] {
            if !files.contains_key(required) {
                return Err(format!("observation bundle is missing `{required}`"));
            }
        }
        if !files.keys().any(|path| path.starts_with("sources/")) {
            return Err("observation bundle contains no exact source member".to_string());
        }
        if string_field(manifest_value, "rootDigest")? != observation_files_root_digest(&files) {
            return Err("observation root digest drifted".to_string());
        }

        for file in self
            .files
            .iter()
            .filter(|file| file.path != "topaz-observation.json")
        {
            if file.path.starts_with("sources/") {
                if file.schema != "topaz.source/utf8" {
                    return Err(format!(
                        "source member `{}` has the wrong schema",
                        file.path
                    ));
                }
                std::str::from_utf8(&file.bytes)
                    .map_err(|_| format!("source member `{}` is not UTF-8", file.path))?;
                continue;
            }
            let jsonl = file.path.ends_with(".jsonl");
            if jsonl && file.bytes.is_empty() {
                if !matches!(
                    file.path.as_str(),
                    "ast.jsonl"
                        | "resolved.jsonl"
                        | "diagnostics.jsonl"
                        | "typed.jsonl"
                        | "lowered.jsonl"
                        | "rust-source.jsonl"
                ) {
                    return Err(format!("projection `{}` is unexpectedly empty", file.path));
                }
                continue;
            }
            let values = crate::canonical::validate(&file.bytes, jsonl)?;
            validate_schema_rows(&file.path, &file.schema, &values)?;
        }
        let response = crate::canonical::validate(&files["response.json"].bytes, false)?;
        let response = response
            .first()
            .ok_or_else(|| "response.json is empty".to_string())?;
        match string_field(response, "highestCompletedPhase")? {
            "tokens" => {
                if !files["ast.jsonl"].bytes.is_empty() || !files["resolved.jsonl"].bytes.is_empty()
                {
                    return Err(
                        "token-terminal observation contains post-token projection rows"
                            .to_string(),
                    );
                }
                for forbidden in ["typed.jsonl", "lowered.jsonl", "rust-source.jsonl"] {
                    if files.contains_key(forbidden) {
                        return Err(format!("token-terminal observation contains `{forbidden}`"));
                    }
                }
            }
            "ast" => {
                if files["ast.jsonl"].bytes.is_empty() {
                    return Err("AST-terminal observation has an empty AST projection".to_string());
                }
                if !files["resolved.jsonl"].bytes.is_empty() {
                    return Err(
                        "AST-terminal observation contains resolved projection rows".to_string()
                    );
                }
                for forbidden in ["typed.jsonl", "lowered.jsonl", "rust-source.jsonl"] {
                    if files.contains_key(forbidden) {
                        return Err(format!("AST-terminal observation contains `{forbidden}`"));
                    }
                }
            }
            _ if files["ast.jsonl"].bytes.is_empty()
                || files["resolved.jsonl"].bytes.is_empty() =>
            {
                return Err(
                    "post-AST observation has an empty AST or resolved projection".to_string(),
                );
            }
            _ => {}
        }
        validate_cross_references(&files)
    }
}
