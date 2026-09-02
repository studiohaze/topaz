use topaz_check::check_program_with_version;
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

#[test]
fn compatible_v518_exactly_inherits_v517_and_is_selectable() {
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
            language_version: LangVersion::V5_18,
        },
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let predecessor = check_program_with_version(source, &parsed.program, LangVersion::V5_17);
    let dormant = check_program_with_version(source, &parsed.program, LangVersion::V5_18);
    assert_eq!(predecessor.diagnostics, dormant.diagnostics);
    assert_eq!(LangVersion::parse_exact("5.18"), Some(LangVersion::V5_18));
    assert_eq!(
        LangVersion::parse_selectable("5.18"),
        Some(LangVersion::V5_18)
    );
    assert!(LangVersion::V5_18.is_selectable());
    assert!(LangVersion::V5_18 < LangVersion::CURRENT);
    assert_eq!(LangVersion::UNMARKED_SOURCE, LangVersion::V5_16);
    assert!(LangVersion::V5_18.uses_self_hosted_product_default());
}
