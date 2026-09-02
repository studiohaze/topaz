use super::support::*;

#[test]
fn default_lsp_recomputes_changed_target_source_without_switching_engine() {
    let valid = "let answer: int = 1\nfunction main() -> int { answer }\n";
    let invalid = "let answer: int = \"wrong\"\nfunction main() -> int { answer }\n";
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///document.tpz","languageId":"topaz","version":1,"text":{valid:?}}}}}}}"#
    )));
    input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"file:///document.tpz","version":2}},"contentChanges":[{{"text":{invalid:?}}}]}}}}"#
    )));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut command = topaz();
    command.arg("lsp");
    let output = output_with_stdin(command, input.as_bytes());
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let bodies = lsp_bodies(&output.stdout);
    let diagnostic_bodies = bodies
        .iter()
        .filter(|body| body.contains("publishDiagnostics"))
        .collect::<Vec<_>>();
    assert_eq!(diagnostic_bodies.len(), 2, "{bodies:#?}");
    assert!(
        diagnostic_bodies[0].contains(r#""diagnostics":[]"#),
        "{}",
        diagnostic_bodies[0]
    );
    assert!(
        diagnostic_bodies[1].contains("TPZ5001"),
        "{}",
        diagnostic_bodies[1]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn lsp_preserves_unicode_overlay_paths_for_non_unicode_physical_targets() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_cli_lsp_non_unicode_physical_targets_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let source = root.join("src");
    let first_physical = root.join(std::ffi::OsString::from_vec(vec![b'd', 0xfe]));
    let second_physical = root.join(std::ffi::OsString::from_vec(vec![b'd', 0xff]));
    std::fs::create_dir_all(&source).expect("create package source directory");
    std::fs::create_dir_all(&first_physical).expect("create first physical directory");
    std::fs::create_dir_all(&second_physical).expect("create second physical directory");
    std::fs::write(
        first_physical.join("module.tpz"),
        "export let firstValue: string = \"disk\"\n",
    )
    .expect("write first physical module");
    std::fs::write(
        second_physical.join("module.tpz"),
        "export let secondValue: string = \"disk\"\n",
    )
    .expect("write second physical module");
    std::os::unix::fs::symlink(first_physical.join("module.tpz"), source.join("first.tpz"))
        .expect("link first logical module");
    std::os::unix::fs::symlink(
        second_physical.join("module.tpz"),
        source.join("second.tpz"),
    )
    .expect("link second logical module");
    let main_source = "import src.first { firstValue }\nimport src.second { secondValue }\nlet answer: int = firstValue + secondValue\n";
    std::fs::write(source.join("main.tpz"), main_source).expect("write package entry");
    let manifest = package_manifest().replace("language = \"5.4\"", "language = \"5.19\"");
    std::fs::write(root.join("topaz.toml"), &manifest).expect("write package manifest");
    std::fs::write(root.join("topaz.lock"), package_lock(&manifest)).expect("write package lock");

    let root_uri = file_uri(&root);
    let first_uri = file_uri(&source.join("first.tpz"));
    let second_uri = file_uri(&source.join("second.tpz"));
    let main_uri = file_uri(&source.join("main.tpz"));
    for compiler in ["rust", "self"] {
        let mut input = String::new();
        input.push_str(&lsp_frame(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"rootUri":"{root_uri}"}}}}"#
        )));
        for (uri, text) in [
            (&first_uri, "export let firstValue: int = 20\n"),
            (&second_uri, "export let secondValue: int = 22\n"),
            (&main_uri, main_source),
        ] {
            input.push_str(&lsp_frame(&format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","languageId":"topaz","version":1,"text":{}}}}}}}"#,
                json_string(text)
            )));
        }
        input.push_str(&lsp_frame(
            r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
        ));
        input.push_str(&lsp_frame(
            r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
        ));

        let mut command = topaz();
        command.args(["lsp", "--compiler", compiler]);
        let output = output_with_stdin(command, input.as_bytes());
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "{compiler}: {output:?}"
        );
        let final_diagnostics = lsp_bodies(&output.stdout)
            .into_iter()
            .filter(|body| body.contains("publishDiagnostics"))
            .last()
            .expect("main document diagnostics");
        assert!(
            final_diagnostics.contains(&main_uri)
                && final_diagnostics.contains(r#""diagnostics":[]"#),
            "{compiler}: {final_diagnostics}"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn lsp_publishes_diagnostics_for_open_documents() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///demo.tpz","languageId":"topaz","version":1,"text":"let x ="}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///demo.tpz","version":2},"contentChanges":[{"text":"let answer: int = 1\nfunction main() -> int { answer }\nlet shown: () = print(\"x\")"}]}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///demo.tpz"},"position":{"line":0,"character":5}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file:///demo.tpz"}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///demo.tpz"},"position":{"line":1,"character":28}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":5,"method":"textDocument/references","params":{"textDocument":{"uri":"file:///demo.tpz"},"position":{"line":1,"character":28},"context":{"includeDeclaration":true}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":6,"method":"textDocument/rename","params":{"textDocument":{"uri":"file:///demo.tpz"},"position":{"line":1,"character":28},"newName":"total"}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":7,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///demo.tpz"},"position":{"line":1,"character":28}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/signatureHelp","params":{"textDocument":{"uri":"file:///demo.tpz"},"position":{"line":2,"character":22}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///typo.tpz","languageId":"topaz","version":1,"text":"let answer: int = 1\nfunction main() -> int { answr }"}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/codeAction","params":{"textDocument":{"uri":"file:///typo.tpz"},"range":{"start":{"line":1,"character":28},"end":{"line":1,"character":33}},"context":{"diagnostics":[]}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":10,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = rust_topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":1"#), "{stdout}");
    assert!(
        stdout.contains(r#""method":"textDocument/publishDiagnostics""#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""uri":"file:///demo.tpz""#), "{stdout}");
    assert!(stdout.contains(r#""code":"TPZ2001""#), "{stdout}");
    assert!(stdout.contains(r#""hoverProvider":true"#), "{stdout}");
    assert!(
        stdout.contains(r#""documentSymbolProvider":true"#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""definitionProvider":true"#), "{stdout}");
    assert!(stdout.contains(r#""referencesProvider":true"#), "{stdout}");
    assert!(stdout.contains(r#""renameProvider":true"#), "{stdout}");
    assert!(stdout.contains(r#""completionProvider""#), "{stdout}");
    assert!(stdout.contains(r#""signatureHelpProvider""#), "{stdout}");
    assert!(stdout.contains(r#""codeActionProvider""#), "{stdout}");
    assert!(stdout.contains(r#""line":0"#), "{stdout}");
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(stdout.contains(r#"answer: int"#), "{stdout}");
    assert!(stdout.contains(r#""id":3"#), "{stdout}");
    assert!(stdout.contains(r#""name":"answer""#), "{stdout}");
    assert!(stdout.contains(r#""name":"main""#), "{stdout}");
    assert!(stdout.contains(r#""kind":13"#), "{stdout}");
    assert!(stdout.contains(r#""kind":12"#), "{stdout}");
    assert!(stdout.contains(r#""id":4"#), "{stdout}");
    assert!(
        stdout.contains(r#""range":{"start":{"line":0,"character":4}"#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""id":5"#), "{stdout}");
    assert!(
        stdout.contains(r#""start":{"line":1,"character":25}"#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""id":6"#), "{stdout}");
    assert!(stdout.contains(r#""newText":"total""#), "{stdout}");
    assert!(
        stdout.contains(r#""changes":{"file:///demo.tpz""#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""id":7"#), "{stdout}");
    assert!(stdout.contains(r#""label":"answer""#), "{stdout}");
    assert!(stdout.contains(r#""label":"main""#), "{stdout}");
    assert!(stdout.contains(r#""label":"std.math""#), "{stdout}");
    assert!(stdout.contains(r#""id":8"#), "{stdout}");
    assert!(
        stdout.contains(r#""label":"print(value: string) -> ()""#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""id":9"#), "{stdout}");
    assert!(stdout.contains(r#""kind":"quickfix""#), "{stdout}");
    assert!(
        stdout.contains(r#""title":"Replace with `answer`""#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""newText":"answer""#), "{stdout}");
    assert!(stdout.contains(r#""id":10,"result":null"#), "{stdout}");
}

#[test]
fn self_formatter_docs_and_lsp_use_one_selected_engine() {
    let root =
        std::env::temp_dir().join(format!("topaz_cli_selected_engine_{}", std::process::id()));
    let rust_file = root.join("rust.tpz");
    let self_file = root.join("self.tpz");
    let fixture_package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/self-hosting/dual-toolchain-package");
    let package = root.join("package");
    let rust_docs = root.join("rust-docs");
    let self_docs = root.join("self-docs");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create DT-K6 tempdir");
    std::fs::create_dir_all(package.join("src")).expect("create DT-K6 package");
    for name in ["topaz.toml", "topaz.lock"] {
        std::fs::copy(fixture_package.join(name), package.join(name))
            .expect("copy DT-K6 package metadata");
    }
    std::fs::copy(
        fixture_package.join("src/answer.tpz"),
        package.join("src/answer.tpz"),
    )
    .expect("copy DT-K6 package module");
    std::fs::write(
        package.join("src/main.tpz"),
        "import src.answer { answer }\n\n\
         export type Count = int\n\
         export record Item { value: int, label: string = \"ready\" }\n\
         export enum Status { Ready, Failed(string) }\n\
         export newtype ItemId = int\n\
         export function selected(value: int, step: int = 1) -> int { value + step }\n\
         let checked = selected(answer())\n",
    )
    .expect("write DT-K6 documentation package");
    let unformatted = "import std.math { abs }   \n\nfunction main() -> int { abs(-1) }  \n\n";
    std::fs::write(&rust_file, unformatted).expect("Rust formatter input");
    std::fs::write(&self_file, unformatted).expect("self formatter input");

    for (path, compiler) in [(&rust_file, "rust"), (&self_file, "self")] {
        let formatted = topaz()
            .arg("fmt")
            .arg(path)
            .args(["--compiler", compiler])
            .output()
            .expect("formatter runs");
        assert!(formatted.status.success(), "{formatted:?}");
    }
    assert_eq!(
        std::fs::read(&rust_file).expect("Rust formatted bytes"),
        std::fs::read(&self_file).expect("self formatted bytes")
    );
    let self_package_format = topaz()
        .arg("fmt")
        .arg("--check")
        .arg("--root")
        .arg(&package)
        .args(["--compiler", "self"])
        .output()
        .expect("self package formatter runs");
    assert!(
        self_package_format.status.success(),
        "{self_package_format:?}"
    );

    for (out_dir, compiler) in [(&rust_docs, "rust"), (&self_docs, "self")] {
        let documented = topaz()
            .arg("doc")
            .arg("--root")
            .arg(&package)
            .arg("--locked")
            .arg("--out-dir")
            .arg(out_dir)
            .args(["--compiler", compiler])
            .output()
            .expect("documentation runs");
        assert!(documented.status.success(), "{documented:?}");
    }
    let self_index = std::fs::read_to_string(self_docs.join("index.md")).expect("self docs");
    assert!(self_index.contains("## Modules"), "{self_index}");
    assert!(self_index.contains("#### Values"), "{self_index}");
    assert!(self_index.contains("#### Type Aliases"), "{self_index}");
    assert!(self_index.contains("#### Records"), "{self_index}");
    assert!(self_index.contains("#### Enums"), "{self_index}");
    assert!(self_index.contains("#### Newtypes"), "{self_index}");
    let self_exports =
        std::fs::read_to_string(self_docs.join("exports.json")).expect("self exports");
    for expected in [
        "\"names\":[\"value\",\"step\"]",
        "\"defaulted\":[false,true]",
        "\"aliases\":[{\"name\":\"Count\"",
        "\"records\":[{\"name\":\"Item\"",
        "\"hasDefault\":true",
        "\"enums\":[{\"name\":\"Status\"",
        "\"payloads\":[\"string\"]",
        "\"newtypes\":[{\"name\":\"ItemId\"",
    ] {
        assert!(
            self_exports.contains(expected),
            "{expected}: {self_exports}"
        );
    }

    let source = "let answer: int = 1\nfunction show(value: int) -> int { value + answer }\nfunction main() -> int { show(answer) }\n";
    let typo = "let answer: int = \"no\"\n";
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///document.tpz","languageId":"topaz","version":1,"text":{source:?}}}}}}}"#
    )));
    for request in [
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///document.tpz"},"position":{"line":0,"character":5}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file:///document.tpz"}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///document.tpz"},"position":{"line":2,"character":30}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"textDocument/references","params":{"textDocument":{"uri":"file:///document.tpz"},"position":{"line":2,"character":30},"context":{"includeDeclaration":true}}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"textDocument/rename","params":{"textDocument":{"uri":"file:///document.tpz"},"position":{"line":2,"character":30},"newName":"total"}}"#,
        r#"{"jsonrpc":"2.0","id":7,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///document.tpz"},"position":{"line":2,"character":30}}}"#,
        r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/signatureHelp","params":{"textDocument":{"uri":"file:///document.tpz"},"position":{"line":2,"character":34}}}"#,
    ] {
        input.push_str(&lsp_frame(request));
    }
    input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"file:///document.tpz","version":2}},"contentChanges":[{{"text":{typo:?}}}]}}}}"#
    )));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/codeAction","params":{"textDocument":{"uri":"file:///document.tpz"},"range":{"start":{"line":0,"character":18},"end":{"line":0,"character":22}},"context":{"diagnostics":[]}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":10,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut rust = topaz();
    rust.args(["lsp", "--compiler", "rust"]);
    let rust = output_with_stdin(rust, input.as_bytes());
    let mut self_hosted = topaz();
    self_hosted.args(["lsp", "--compiler", "self"]);
    let self_hosted = output_with_stdin(self_hosted, input.as_bytes());
    assert!(rust.status.success(), "{rust:?}");
    assert!(self_hosted.status.success(), "{self_hosted:?}");
    let rust_bodies = lsp_bodies(&rust.stdout);
    let self_bodies = lsp_bodies(&self_hosted.stdout);
    assert_eq!(rust_bodies.len(), self_bodies.len(), "LSP response count");
    for (index, (rust, self_hosted)) in rust_bodies.iter().zip(&self_bodies).enumerate() {
        assert_eq!(rust, self_hosted, "bounded LSP response drift at {index}");
    }
    assert!(self_hosted.stderr.is_empty(), "{self_hosted:?}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn lsp_agent_pack_profile_preserves_semantics_and_rule_identity() {
    let source = "let answer: int = 1\nlet checked = assert(true)\nlet result: int = answer\n";
    let mut profiled_input = String::new();
    profiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"experimental":{"checkProfile":"nonsense"}},"initializationOptions":{"topaz":{"checkProfile":"agent-pack"}}}}"#,
    ));
    profiled_input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///agent-pack.tpz","languageId":"topaz","version":1,"text":{source:?}}}}}}}"#
    )));
    profiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///agent-pack.tpz"},"position":{"line":2,"character":20}}}"#,
    ));
    profiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///agent-pack.tpz"},"position":{"line":2,"character":20}}}"#,
    ));
    profiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}"#,
    ));
    profiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    for compiler in ["self", "rust"] {
        let mut command = topaz();
        command.args(["lsp", "--compiler", compiler]);
        let output = output_with_stdin(command, profiled_input.as_bytes());
        assert!(output.status.success(), "{compiler}: {output:?}");
        assert!(output.stderr.is_empty(), "{compiler}: {output:?}");
        let bodies = lsp_bodies(&output.stdout);
        let diagnostics = bodies
            .iter()
            .find(|body| body.contains("publishDiagnostics"))
            .expect("profiled diagnostics notification");
        assert!(diagnostics.contains(r#""code":"TPZ5801""#), "{diagnostics}");
        assert!(
            diagnostics.contains(r#""data":{"profileRule":"agent-pack/no-assert"}"#),
            "{diagnostics}"
        );
        if compiler == "self" {
            let hover = bodies
                .iter()
                .find(|body| body.contains(r#""id":2"#))
                .expect("profiled hover response");
            assert!(!hover.contains(r#""result":null"#), "{compiler}: {hover}");
            let definition = bodies
                .iter()
                .find(|body| body.contains(r#""id":3"#))
                .expect("profiled definition response");
            assert!(
                definition.contains(r#""start":{"line":0,"character":4}"#),
                "{compiler}: {definition}"
            );
        }
    }

    let mut unprofiled_input = String::new();
    unprofiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    unprofiled_input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///agent-pack.tpz","languageId":"topaz","version":1,"text":{source:?}}}}}}}"#
    )));
    unprofiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
    ));
    unprofiled_input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));
    let mut command = topaz();
    command.args(["lsp", "--compiler", "self"]);
    let output = output_with_stdin(command, unprofiled_input.as_bytes());
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let diagnostics = lsp_bodies(&output.stdout)
        .into_iter()
        .find(|body| body.contains("publishDiagnostics"))
        .expect("unprofiled diagnostics notification");
    assert!(diagnostics.contains(r#""diagnostics":[]"#), "{diagnostics}");
    assert!(!diagnostics.contains("profileRule"), "{diagnostics}");
}

#[test]
fn lsp_rejects_invalid_scoped_check_profiles() {
    for (value, expected) in [
        (r#""bootstrap""#, "applies to a locked package"),
        (r#""nonsense""#, "must be `agent-pack`"),
        ("42", "must be the string `agent-pack`"),
    ] {
        let mut input = String::new();
        let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"initializationOptions":{"topaz":{"checkProfile":VALUE}}}}"#
            .replace("VALUE", value);
        input.push_str(&lsp_frame(&initialize));
        input.push_str(&lsp_frame(
            r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
        ));
        let mut command = topaz();
        command.args(["lsp", "--compiler", "self"]);
        let output = output_with_stdin(command, input.as_bytes());
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let response = lsp_bodies(&output.stdout)
            .into_iter()
            .find(|body| body.contains(r#""id":1"#))
            .expect("initialize response");
        assert!(response.contains(r#""code":-32602"#), "{response}");
        assert!(response.contains(expected), "{response}");
    }
}

#[test]
fn lsp_uses_initialized_locked_package_with_open_document_overlay() {
    let base = std::env::temp_dir().join(format!("topaz lsp package test {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("app");
    let dep = root.join("vendor/greeter/1.0.0");
    std::fs::create_dir_all(root.join("src/support")).expect("app sources");
    std::fs::create_dir_all(dep.join("src")).expect("vendored dependency");
    std::fs::write(
        dep.join("topaz.toml"),
        r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.6"
entry = "src/lib.tpz"

[exports]
module = "src/lib.tpz"
"#,
    )
    .expect("dependency manifest");
    std::fs::write(
        dep.join("src/lib.tpz"),
        "export function greeting() -> string { \"hello\" }\n",
    )
    .expect("dependency source");
    std::fs::write(
        root.join("topaz.toml"),
        r#"[package]
name = "lsp_app"
version = "0.1.0"
language = "5.6"
entry = "src/main.tpz"

[dependencies]
std = "5.6"
greeter = "1.0.0"
"#,
    )
    .expect("app manifest");
    std::fs::write(
        root.join("src/support/message.tpz"),
        "export function suffix() -> string { \"world\" }\n",
    )
    .expect("local module");
    let clean_source = "import greeter { greeting }\nimport src.support.message { suffix }\nprint(\"{greeting()} {suffix()}\")\n";
    std::fs::write(root.join("src/main.tpz"), clean_source).expect("entry");

    let lock = topaz()
        .arg("lock")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("lock runs");
    assert!(lock.status.success(), "{lock:?}");

    let root_uri = file_uri(&root);
    let document_uri = file_uri(&root.join("src/main.tpz"));
    let mut input = String::new();
    input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"rootUri":"{root_uri}"}}}}"#
    )));
    input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{document_uri}","languageId":"topaz","version":1,"text":{}}}}}}}"#,
        json_string(clean_source)
    )));
    input.push_str(&lsp_frame(&format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{document_uri}","version":2}},"contentChanges":[{{"text":"let value ="}}]}}}}"#
    )));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""diagnostics":[]"#), "{stdout}");
    assert!(stdout.contains(r#""code":"TPZ2001""#), "{stdout}");
    assert!(!stdout.contains("TPZ3001"), "{stdout}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn lsp_completion_resolves_std_namespace_members() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///gen-complete.tpz","languageId":"topaz","version":1,"text":"import std.gen\nlet xs = gen."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///gen-complete.tpz"},"position":{"line":1,"character":13}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = rust_topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(stdout.contains(r#""label":"bools""#), "{stdout}");
    assert!(stdout.contains(r#""label":"intRange""#), "{stdout}");
    assert!(!stdout.contains(r#""label":"random""#), "{stdout}");
    assert!(!stdout.contains(r#""label":"seed""#), "{stdout}");
    assert!(!stdout.contains(r#""label":"shrink""#), "{stdout}");
    assert!(!stdout.contains(r#""label":"std.math""#), "{stdout}");
}

#[test]
fn lsp_completion_resolves_static_namespace_members() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///map-complete.tpz","languageId":"topaz","version":1,"text":"let m = Map."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///map-complete.tpz"},"position":{"line":0,"character":12}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(stdout.contains(r#""label":"new""#), "{stdout}");
    assert!(stdout.contains(r#""label":"ofEntries""#), "{stdout}");
    assert!(!stdout.contains(r#""label":"std.math""#), "{stdout}");
}

#[test]
fn lsp_completion_includes_rounding_mode_values() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///rounding-complete.tpz","languageId":"topaz","version":1,"text":"let mode = RoundingMode."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///rounding-complete.tpz"},"position":{"line":0,"character":24}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///protocol-complete.tpz","languageId":"topaz","version":1,"text":"let render = Show."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///protocol-complete.tpz"},"position":{"line":0,"character":18}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    for member in [
        "Down",
        "Up",
        "TowardZero",
        "AwayFromZero",
        "HalfUp",
        "HalfEven",
    ] {
        assert!(
            stdout.contains(&format!(r#""label":"{member}","kind":20"#)),
            "{stdout}"
        );
    }
    assert!(stdout.contains(r#""label":"show","kind":3"#), "{stdout}");
    assert!(!stdout.contains(r#""label":"std.math""#), "{stdout}");
}

#[test]
fn lsp_completion_includes_checker_builtin_inventory() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///builtin-complete.tpz","languageId":"topaz","version":1,"text":"let value = "}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///builtin-complete.tpz"},"position":{"line":0,"character":12}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = rust_topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    for expected in [
        r#""label":"toIntRadix","kind":3"#,
        r#""label":"ByteBuffer","kind":7"#,
        r#""label":"template","kind":7"#,
        r#""label":"Math","kind":9"#,
        r#""label":"JSON","kind":9"#,
        r#""label":"Test","kind":9"#,
        r#""label":"Show","kind":8"#,
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
}

#[test]
fn lsp_completion_includes_the_complete_json_namespace() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///json-complete.tpz","languageId":"topaz","version":1,"text":"let value = JSON."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///json-complete.tpz"},"position":{"line":0,"character":17}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    for member in ["parse", "parseAs", "decode", "stringify"] {
        assert!(
            stdout.contains(&format!(r#""label":"{member}""#)),
            "{stdout}"
        );
    }
    assert!(!stdout.contains(r#""label":"std.math""#), "{stdout}");
}

#[test]
fn lsp_completion_resolves_checked_receiver_members() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///receiver-complete.tpz","languageId":"topaz","version":1,"text":"let xs: Array<int> = [1, 2]\nlet y = xs."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///receiver-complete.tpz"},"position":{"line":1,"character":11}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = rust_topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(stdout.contains(r#""label":"push","kind":2"#), "{stdout}");
    assert!(stdout.contains(r#""label":"length","kind":10"#), "{stdout}");
    assert!(!stdout.contains(r#""label":"std.math""#), "{stdout}");
}

#[test]
fn lsp_completion_respects_source_binding_over_static_namespace() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///shadowed-map.tpz","languageId":"topaz","version":1,"text":"let Map: Array<int> = [1, 2]\nlet value = Map."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///shadowed-map.tpz"},"position":{"line":1,"character":16}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///shadowed-map-import.tpz","languageId":"topaz","version":1,"text":"import std.gen as Map\nlet value = Map."}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///shadowed-map-import.tpz"},"position":{"line":1,"character":16}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    for mut cmd in [rust_topaz(), topaz()] {
        cmd.arg("lsp");
        let out = output_with_stdin(cmd, input.as_bytes());
        assert!(out.status.success(), "{out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains(r#""label":"push","kind":2"#), "{stdout}");
        assert!(
            stdout.contains(r#""label":"intRange","kind":3"#),
            "{stdout}"
        );
        assert!(!stdout.contains(r#""label":"ofEntries""#), "{stdout}");
    }
}

#[test]
fn lsp_signature_help_includes_map_of_entries() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///map.tpz","languageId":"topaz","version":1,"text":"let m = Map.ofEntries("}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/signatureHelp","params":{"textDocument":{"uri":"file:///map.tpz"},"position":{"line":0,"character":22}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(
        stdout.contains(
            r#""label":"Map.ofEntries(entries: Array<{ key: K, value: V }>) -> Map<K, V>""#
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#""label":"entries: Array<{ key: K, value: V }>""#),
        "{stdout}"
    );
}

#[test]
fn lsp_signature_help_resolves_std_namespace_imports() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///gen.tpz","languageId":"topaz","version":1,"text":"import std.gen\nlet xs = gen.intRange("}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/signatureHelp","params":{"textDocument":{"uri":"file:///gen.tpz"},"position":{"line":1,"character":22}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = rust_topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(
        stdout.contains(r#""label":"gen.intRange(lo: int, hi: int) -> Array<int>""#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""label":"lo: int""#), "{stdout}");
    assert!(stdout.contains(r#""label":"hi: int""#), "{stdout}");
}

#[test]
fn lsp_signature_help_resolves_std_selected_imports() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.tpz","languageId":"topaz","version":1,"text":"import std.test { forAllInt }\nforAllInt(\"range\", "}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/signatureHelp","params":{"textDocument":{"uri":"file:///test.tpz"},"position":{"line":1,"character":19}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = rust_topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(
        stdout.contains(
            r#""label":"forAllInt(name: string, values: Array<int>, f: (int) -> ()) -> ()""#
        ),
        "{stdout}"
    );
    assert!(stdout.contains(r#""activeParameter":1"#), "{stdout}");
}

#[test]
fn lsp_signature_help_resolves_checked_receiver_methods() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///receiver-sig.tpz","languageId":"topaz","version":1,"text":"let mut xs: Array<int> = [1]\nxs.push("}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/signatureHelp","params":{"textDocument":{"uri":"file:///receiver-sig.tpz"},"position":{"line":1,"character":8}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    let mut cmd = rust_topaz();
    cmd.arg("lsp");
    let out = output_with_stdin(cmd, input.as_bytes());
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#""id":2"#), "{stdout}");
    assert!(
        stdout.contains(r#""label":"xs.push(x: int) -> ()""#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""label":"x: int""#), "{stdout}");
    assert!(stdout.contains(r#""activeParameter":0"#), "{stdout}");
}

#[test]
fn lsp_signature_help_respects_source_function_over_builtin() {
    let mut input = String::new();
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///shadowed-print.tpz","languageId":"topaz","version":1,"text":"function print(left: int, right: string) -> int { left }\nlet output = print("}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/signatureHelp","params":{"textDocument":{"uri":"file:///shadowed-print.tpz"},"position":{"line":1,"character":19}}}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
    ));
    input.push_str(&lsp_frame(
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    ));

    for mut cmd in [rust_topaz(), topaz()] {
        cmd.arg("lsp");
        let out = output_with_stdin(cmd, input.as_bytes());
        assert!(out.status.success(), "{out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(r#""label":"print(left: int, right: string) -> int""#),
            "{stdout}"
        );
        assert!(stdout.contains(r#""activeParameter":0"#), "{stdout}");
    }
}
