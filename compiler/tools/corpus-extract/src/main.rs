//! corpus-extract — regenerates `corpus/v5.1/{examples,spec,site}`
//! and the fence-classification manifests deterministically from the
//! vendored sources (CDR-001 §7). CI re-runs it and fails on drift.
//!
//! Usage: `corpus-extract --check` (also the no-argument default)
//! verifies the committed corpus matches a fresh generation and
//! exits nonzero on drift; `corpus-extract --write` purges and
//! regenerates the corpus in place. Regeneration is destructive
//! (purge-then-write), so it never runs implicitly: any invocation
//! without the explicit `--write` flag is a read-only check.

use std::fs;
use std::process::ExitCode;

use corpus_extract::{drift, generate, owned_dirs, repo_root};

fn main() -> ExitCode {
    // Destructive regeneration requires the explicit flag; every
    // other invocation — including unknown arguments such as a test
    // runner's `--list` probing this binary — is a read-only check.
    let write = std::env::args().any(|a| a == "--write");
    let root = repo_root();

    if !write {
        match drift(&root) {
            Ok(problems) if problems.is_empty() => {
                println!("corpus-extract: no drift");
                ExitCode::SUCCESS
            }
            Ok(problems) => {
                for p in &problems {
                    eprintln!("corpus-extract: {p}");
                }
                eprintln!(
                    "corpus-extract: drift detected ({} problems)",
                    problems.len()
                );
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("corpus-extract: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        let generated = match generate(&root) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("corpus-extract: {e}");
                return ExitCode::FAILURE;
            }
        };
        for dir in owned_dirs() {
            let abs = root.join(&dir);
            if abs.exists()
                && let Err(e) = fs::remove_dir_all(&abs)
            {
                eprintln!("corpus-extract: purging {dir}: {e}");
                return ExitCode::FAILURE;
            }
        }
        for (rel, content) in &generated.files {
            let abs = root.join(rel);
            if let Some(parent) = abs.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                eprintln!("corpus-extract: creating {}: {e}", parent.display());
                return ExitCode::FAILURE;
            }
            if let Err(e) = fs::write(&abs, content) {
                eprintln!("corpus-extract: writing {rel}: {e}");
                return ExitCode::FAILURE;
            }
        }
        let topaz = generated
            .rows
            .iter()
            .filter(|r| r.fence.language == "topaz")
            .count();
        println!(
            "corpus-extract: wrote {} files ({} fences inventoried, {} topaz)",
            generated.files.len(),
            generated.rows.len(),
            topaz
        );
        ExitCode::SUCCESS
    }
}
