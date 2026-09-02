use super::support::*;

#[test]
fn fmt_formats_package_sources_and_skips_vendor() {
    let dir = std::env::temp_dir().join("topaz_fmt_package_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::create_dir_all(dir.join("vendor/dep/1.0.0")).expect("vendor");
    std::fs::write(
        dir.join("topaz.toml"),
        r#"[package]
name = "fmtpkg"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
"#,
    )
    .expect("manifest");
    std::fs::write(dir.join("src/main.tpz"), "print(\"fmt\")   \n\n\n").expect("source");
    std::fs::write(
        dir.join("vendor/dep/1.0.0/lib.tpz"),
        "print(\"do not touch\")   \n\n",
    )
    .expect("vendor source");

    let out = topaz()
        .arg("fmt")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(dir.join("src/main.tpz")).expect("source");
    assert_eq!(src, "print(\"fmt\")\n");
    let vendored = std::fs::read_to_string(dir.join("vendor/dep/1.0.0/lib.tpz")).expect("vendor");
    assert_eq!(vendored, "print(\"do not touch\")   \n\n");

    let out = topaz()
        .arg("run")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "fmt\n");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_sorts_leading_import_block() {
    let path = std::env::temp_dir().join("topaz_fmt_import_sort.tpz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        "import zeta.Tools   \nimport alpha.Core\n\nlet value = 1   \n",
    )
    .expect("source");

    let out = rust_topaz()
        .arg("fmt")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(
        src,
        "import alpha.Core\nimport zeta.Tools\n\nlet value = 1\n"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn fmt_check_reports_sorted_drift_without_writing() {
    let dir = std::env::temp_dir().join("topaz_fmt_check_package_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/nested")).expect("src");
    std::fs::create_dir_all(dir.join("vendor/dep/1.0.0")).expect("vendor");
    std::fs::create_dir_all(dir.join("target/generated")).expect("target");
    std::fs::write(dir.join("topaz.toml"), package_manifest()).expect("manifest");
    let a = dir.join("src").join("a.tpz");
    let b = dir.join("src").join("nested").join("b.tpz");
    let vendor = dir.join("vendor").join("dep").join("1.0.0").join("lib.tpz");
    let target = dir.join("target").join("generated").join("out.tpz");
    std::fs::write(&a, "let a: int = 1   \n\n").expect("a");
    std::fs::write(&b, "let b: int = 2\t\n\n").expect("b");
    std::fs::write(&vendor, "let vendor: int = 3   \n").expect("vendor");
    std::fs::write(&target, "let generated: int = 4   \n").expect("target");

    let a_before = std::fs::read(&a).expect("a bytes");
    let b_before = std::fs::read(&b).expect("b bytes");
    let a_meta = std::fs::metadata(&a).expect("a metadata");
    let b_meta = std::fs::metadata(&b).expect("b metadata");

    let out = topaz()
        .arg("fmt")
        .arg("--check")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let a_name = a.to_string_lossy();
    let b_name = b.to_string_lossy();
    let a_pos = stderr.find(a_name.as_ref()).expect("a drift");
    let b_pos = stderr.find(b_name.as_ref()).expect("b drift");
    assert!(a_pos < b_pos, "{stderr}");
    assert!(
        stderr.contains("topaz: checked 2 file(s), 2 differs"),
        "{stderr}"
    );
    assert!(
        !stderr.contains(vendor.to_string_lossy().as_ref()),
        "{stderr}"
    );
    assert!(
        !stderr.contains(target.to_string_lossy().as_ref()),
        "{stderr}"
    );

    assert_eq!(std::fs::read(&a).expect("a bytes"), a_before);
    assert_eq!(std::fs::read(&b).expect("b bytes"), b_before);
    let a_after = std::fs::metadata(&a).expect("a metadata");
    let b_after = std::fs::metadata(&b).expect("b metadata");
    assert_eq!(
        a_after.modified().expect("a mtime"),
        a_meta.modified().expect("a mtime")
    );
    assert_eq!(
        b_after.modified().expect("b mtime"),
        b_meta.modified().expect("b mtime")
    );
    assert_eq!(
        a_after.permissions().readonly(),
        a_meta.permissions().readonly()
    );
    assert_eq!(
        b_after.permissions().readonly(),
        b_meta.permissions().readonly()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(a_after.permissions().mode(), a_meta.permissions().mode());
        assert_eq!(b_after.permissions().mode(), b_meta.permissions().mode());
    }

    let fmt = topaz()
        .arg("fmt")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(fmt.status.success(), "{fmt:?}");
    let clean = topaz()
        .arg("fmt")
        .arg("--check")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(clean.status.success(), "{clean:?}");
    assert!(
        String::from_utf8_lossy(&clean.stderr).contains("topaz: checked 2 file(s), 0 differs"),
        "{clean:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_check_preserves_malformed_source_and_rejects_misuse() {
    let path = std::env::temp_dir().join("topaz_fmt_check_malformed.tpz");
    let _ = std::fs::remove_file(&path);
    let malformed = b"let value =\n";
    std::fs::write(&path, malformed).expect("source");
    let metadata = std::fs::metadata(&path).expect("metadata");

    let out = topaz()
        .arg("fmt")
        .arg("--check")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert_eq!(std::fs::read(&path).expect("source"), malformed);
    assert_eq!(
        std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        metadata.modified().expect("mtime")
    );

    for command in ["check", "run"] {
        let misuse = topaz()
            .arg(command)
            .arg("--check")
            .arg(&path)
            .output()
            .expect("binary runs");
        assert!(!misuse.status.success(), "{misuse:?}");
        assert!(
            String::from_utf8_lossy(&misuse.stderr).contains("`--check` applies to `fmt` only"),
            "{misuse:?}"
        );
    }
    let help = topaz().arg("help").output().expect("binary runs");
    assert!(help.status.success(), "{help:?}");
    assert!(
        String::from_utf8_lossy(&help.stdout).contains("`--check` reports drift without writing"),
        "{help:?}"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_organize_imports_sorts_single_file_import_block() {
    let path = std::env::temp_dir().join("topaz_refactor_organize_imports.tpz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        "import zeta.Tools   \nimport alpha.Core\n\nlet value = 1   \n",
    )
    .expect("source");

    let out = topaz()
        .arg("refactor")
        .arg("organize-imports")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(
        src,
        "import alpha.Core\nimport zeta.Tools   \n\nlet value = 1   \n"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_add_missing_match_cases_fills_enum_unit_arms() {
    let path = std::env::temp_dir().join("topaz_refactor_add_match_cases.tpz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        "enum Color { Red, Green, Rgb(int, int, int) }\n\
         let color: Color = Color.Red\n\
         match color {\n\
           case Red => print(\"red\")\n\
         }\n",
    )
    .expect("source");

    let out = topaz()
        .arg("refactor")
        .arg("add-missing-match-cases")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(
        src,
        "enum Color { Red, Green, Rgb(int, int, int) }\n\
         let color: Color = Color.Red\n\
         match color {\n\
           case Red => print(\"red\")\n\
           case Green => ()\n\
           case Rgb(_, _, _) => ()\n\
         }\n"
    );

    let out = topaz()
        .arg("check")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_add_missing_match_cases_refuses_ill_typed_placeholder() {
    let path = std::env::temp_dir().join("topaz_refactor_add_match_cases_refuse.tpz");
    let _ = std::fs::remove_file(&path);
    let original = "enum Color { Red, Green }\n\
                    let color: Color = Color.Red\n\
                    let n: int = match color {\n\
                      case Red => 1\n\
                    }\n";
    std::fs::write(&path, original).expect("source");

    let out = topaz()
        .arg("refactor")
        .arg("add-missing-match-cases")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("would not type-check"),
        "{out:?}"
    );
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(src, original);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_derive_json_adds_fresh_derives_clause() {
    let path = std::env::temp_dir().join("topaz_refactor_derive_json.tpz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        "record User { name: string }\nlet user = User { name: \"Ada\" }\n",
    )
    .expect("source");
    let location = format!("{}:1", path.to_string_lossy());

    let out = topaz()
        .arg("refactor")
        .arg("derive-json")
        .arg(&location)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(
        src,
        "record User derives JSON { name: string }\nlet user = User { name: \"Ada\" }\n"
    );

    let out = topaz()
        .arg("check")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_derive_json_extends_existing_derives_clause() {
    let path = std::env::temp_dir().join("topaz_refactor_derive_json_existing.tpz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        "enum Status derives Show { Pending, Done }\nlet status: Status = Status.Done\n",
    )
    .expect("source");
    let location = format!("{}:1", path.to_string_lossy());

    let out = topaz()
        .arg("refactor")
        .arg("derive-json")
        .arg(&location)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(
        src,
        "enum Status derives Show, JSON { Pending, Done }\nlet status: Status = Status.Done\n"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_derive_json_refuses_non_encodable_fields() {
    let path = std::env::temp_dir().join("topaz_refactor_derive_json_refuse.tpz");
    let _ = std::fs::remove_file(&path);
    let original = "record Bad { f: (int) -> int }\n";
    std::fs::write(&path, original).expect("source");
    let location = format!("{}:1", path.to_string_lossy());

    let out = topaz()
        .arg("refactor")
        .arg("derive-json")
        .arg(&location)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("would not type-check"),
        "{out:?}"
    );
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(src, original);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_rename_rewrites_single_file_binding() {
    let path = std::env::temp_dir().join("topaz_refactor_rename.tpz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        "let answer: int = 40\nfunction main() -> int { answer + answer }\n",
    )
    .expect("source");

    let out = topaz()
        .arg("refactor")
        .arg("rename")
        .arg("answer")
        .arg("total")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(
        src,
        "let total: int = 40\nfunction main() -> int { total + total }\n"
    );

    let out = topaz()
        .arg("check")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn refactor_rename_rejects_ambiguous_same_name_bindings() {
    let path = std::env::temp_dir().join("topaz_refactor_rename_ambiguous.tpz");
    let _ = std::fs::remove_file(&path);
    let original = "let value: int = 1\nfunction pick(value: int) -> int { value }\n";
    std::fs::write(&path, original).expect("source");

    let out = topaz()
        .arg("refactor")
        .arg("rename")
        .arg("value")
        .arg("renamed")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("found 2 lexical bindings named `value`"),
        "{out:?}"
    );
    let src = std::fs::read_to_string(&path).expect("source");
    assert_eq!(src, original);

    let _ = std::fs::remove_file(&path);
}
