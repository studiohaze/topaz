use topaz_check::check_program_with_version;
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

#[test]
fn current_v59_inherits_the_complete_v58_checked_surface() {
    let source = r#"
let mut bytes = ByteBuffer.allocate(4)
bytes.set(0, 84)
bytes.set(1, 111)
bytes.set(2, 112)
bytes.set(3, 97)
let snapshot = bytes.toBytes()
print("{snapshot.length}")
"#;
    let parsed = parse_with_options(
        FileId(0),
        source,
        ParseOptions {
            language_version: LangVersion::V5_9,
        },
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let predecessor = check_program_with_version(source, &parsed.program, LangVersion::V5_8);
    let current = check_program_with_version(source, &parsed.program, LangVersion::V5_9);
    assert!(
        predecessor.diagnostics.is_empty(),
        "{:?}",
        predecessor.diagnostics
    );
    assert!(current.diagnostics.is_empty(), "{:?}", current.diagnostics);
}
