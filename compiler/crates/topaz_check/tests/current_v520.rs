use topaz_check::check_program_with_version;
use topaz_diag::FileId;
use topaz_parser::{ParseOptions, parse_with_options};
use topaz_syntax::LangVersion;

#[test]
fn current_v520_is_selectable_and_checks_the_current_profile() {
    let source = r#"
record Item { value: int }
let item = Item { value: 20 }
print("{item.value}")
"#;
    let parsed = parse_with_options(
        FileId(0),
        source,
        ParseOptions {
            language_version: LangVersion::V5_20,
        },
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check_program_with_version(source, &parsed.program, LangVersion::V5_20);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    assert_eq!(LangVersion::parse_exact("5.20"), Some(LangVersion::V5_20));
    assert_eq!(
        LangVersion::parse_selectable("5.20"),
        Some(LangVersion::V5_20)
    );
    assert!(LangVersion::V5_20.is_selectable());
    assert_eq!(LangVersion::CURRENT, LangVersion::V5_20);
    assert_eq!(LangVersion::UNMARKED_SOURCE, LangVersion::V5_16);
    assert!(LangVersion::V5_20.uses_self_hosted_product_default());
}
