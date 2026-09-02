use topaz_check::check_program_with_version;
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

#[test]
fn current_v513_inherits_the_complete_v512_checked_surface() {
    let source = r#"
let mut bytes = ByteBuffer.allocate(4)
bytes.set(0, 84)
bytes.set(1, 111)
bytes.set(2, 112)
bytes.set(3, 97)
let snapshot = bytes.toBytes()
print("{Hash.crc32(snapshot)}")
"#;
    let parsed = parse_with_options(
        FileId(0),
        source,
        ParseOptions {
            language_version: LangVersion::V5_13,
        },
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let predecessor = check_program_with_version(source, &parsed.program, LangVersion::V5_12);
    let current = check_program_with_version(source, &parsed.program, LangVersion::V5_13);
    assert!(
        predecessor.diagnostics.is_empty(),
        "{:?}",
        predecessor.diagnostics
    );
    assert!(current.diagnostics.is_empty(), "{:?}", current.diagnostics);
    assert_eq!(LangVersion::parse_exact("5.13"), Some(LangVersion::V5_13));
    assert_eq!(
        LangVersion::parse_selectable("5.13"),
        Some(LangVersion::V5_13)
    );
}
