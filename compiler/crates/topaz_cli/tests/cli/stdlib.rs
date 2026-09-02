use super::support::*;

#[test]
fn std_virtual_modules_are_importable_in_v5_4() {
    // v5.4 opens `std` as a virtual module root: no physical `std/*.tpz` files are
    // needed, and the imported wrappers lower through the same shared stdlib leaves
    // as the legacy global namespaces.
    let dir = std::env::temp_dir().join(format!("topaz_std_import_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        r#"import std.math
import std.bytes
import std.hash
import std.path

let b = bytes.encodeUtf8("abc")
let digest = hash.sha256(b).toHex()
let p = match path.from("src//./main.tpz") {
  case Ok(pathValue) => pathValue.toString()
  case Err(e) => e
}
match math.sqrt(9.0) {
  case Ok(x) => print("{x}/{digest}/{p}")
  case Err(e) => print(e)
}
"#,
    )
    .expect("std entry");

    let checked = rust_topaz()
        .arg("check")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stdout).contains("types-ok (5 modules)"),
        "{checked:?}"
    );

    let ran = rust_topaz()
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains(
            "3.0/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad/src/main.tpz"
        ),
        "{ran:?}"
    );

    let emitted = rust_topaz()
        .arg("emit")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(emitted.status.success(), "{emitted:?}");
    assert!(
        String::from_utf8_lossy(&emitted.stdout).contains("pub fn run_with_host"),
        "{emitted:?}"
    );

    let old = rust_topaz()
        .arg("check")
        .arg("--language-version")
        .arg("5.3")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(!old.status.success(), "{old:?}");
    assert!(
        String::from_utf8_lossy(&old.stderr).contains("TPZ3016"),
        "{old:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_virtual_modules_support_selected_imports() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_std_selected_import_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        r#"import std.math { sqrt, cos }
match sqrt(16.0) {
  case Ok(x) => print("{x}/{cos(3.141592653589793)}")
  case Err(e) => print(e)
}
"#,
    )
    .expect("std selected entry");

    let ran = rust_topaz()
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "4.0/-1.0\n");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_parser_virtual_module_scans_scalar_indices_in_run_and_build() {
    let dir = std::env::temp_dir().join(format!("topaz_std_parser_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        r#"import std.parser { isAsciiAlpha, isAsciiDigit, takeWhileAsciiAlnum, takeWhileAsciiWhitespace }

let text = "  abc123-한"
let tokenStart = takeWhileAsciiWhitespace(text, -4)
let tokenEnd = takeWhileAsciiAlnum(text, tokenStart)
print("{tokenStart}/{tokenEnd}/{isAsciiAlpha(text, 9)}/{isAsciiDigit(text, 5)}")
"#,
    )
    .expect("std.parser entry");

    let checked = topaz()
        .arg("check")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stdout).contains("types-ok (2 modules)"),
        "{checked:?}"
    );

    let ran = topaz()
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "2/8/false/true\n");

    let build_dir = dir.join("build-out");
    let built = topaz()
        .arg("build")
        .arg(&entry)
        .arg("--out-dir")
        .arg(&build_dir)
        .arg("--run")
        .output()
        .expect("binary runs");
    assert!(built.status.success(), "{built:?}");
    assert!(
        String::from_utf8_lossy(&built.stdout).contains("2/8/false/true"),
        "{built:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_test_virtual_module_assertions_and_golden_run_and_build() {
    let dir = std::env::temp_dir().join(format!("topaz_std_test_import_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("golden.txt"), "snap").expect("golden");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        r#"import std.test { assertEq, assertNe, assertContains, assertOk, assertErr, assertSome, assertNone, assertGolden }

let ok = assertOk(Ok(41))
let err = assertErr(Err("bad"))
let some = assertSome(Some(1))
assertEq(ok + some, 42)
assertNe(err, "good")
assertContains("topaz", "opa")
assertNone(None)
assertGolden("golden.txt", "snap")
print("std.test ok")
"#,
    )
    .expect("std.test entry");

    let checked = rust_topaz()
        .current_dir(&dir)
        .arg("check")
        .arg("main.tpz")
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stdout).contains("types-ok (2 modules)"),
        "{checked:?}"
    );

    let ran = rust_topaz()
        .current_dir(&dir)
        .arg("run")
        .arg("main.tpz")
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "std.test ok\n");

    let build_dir = dir.join("build-out");
    let built = rust_topaz()
        .current_dir(&dir)
        .arg("build")
        .arg("main.tpz")
        .arg("--out-dir")
        .arg(&build_dir)
        .arg("--run")
        .output()
        .expect("binary runs");
    assert!(built.status.success(), "{built:?}");
    assert!(
        String::from_utf8_lossy(&built.stdout).contains("std.test ok"),
        "{built:?}"
    );

    let failing = dir.join("failing.tpz");
    std::fs::write(
        &failing,
        r#"import std.test { assertEq }
assertEq(1, 2)
"#,
    )
    .expect("failing entry");
    let failed = rust_topaz()
        .current_dir(&dir)
        .arg("run")
        .arg("failing.tpz")
        .output()
        .expect("binary runs");
    assert!(!failed.status.success(), "{failed:?}");
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("TPZ4007"),
        "{failed:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_gen_and_test_property_helpers_run_and_build() {
    let dir = std::env::temp_dir().join(format!("topaz_std_gen_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        r#"import std.gen
import std.test { assertEq, forAllBool, forAllInt }

let values = gen.intRange(-3, 3)
let mut total = 0
let mut seen = ""
forAllInt("square non-negative", values, (x) => {
  assertEq(x * x >= 0, true)
  total += x
  seen = "{seen},{x}"
})
forAllInt("empty range", gen.intRange(3, 1), (x) => {
  total += x
})
let mut trueCount = 0
let mut boolSeen = ""
forAllBool("bool domain", gen.bools(), (flag) => {
  boolSeen = "{boolSeen},{flag}"
  if flag { trueCount += 1 }
  assertEq(flag == true || flag == false, true)
})
print("{values.length}/{total}/{gen.intRange(3, 1).length}/{trueCount}/{seen}/{boolSeen}")
"#,
    )
    .expect("std.gen entry");

    let checked = rust_topaz()
        .current_dir(&dir)
        .arg("check")
        .arg("main.tpz")
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");
    assert!(
        String::from_utf8_lossy(&checked.stdout).contains("types-ok (3 modules)"),
        "{checked:?}"
    );

    let ran = rust_topaz()
        .current_dir(&dir)
        .arg("run")
        .arg("main.tpz")
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "7/0/0/1/,-3,-2,-1,0,1,2,3/,false,true\n"
    );

    let build_dir = dir.join("build-out");
    let built = rust_topaz()
        .current_dir(&dir)
        .arg("build")
        .arg("main.tpz")
        .arg("--out-dir")
        .arg(&build_dir)
        .arg("--run")
        .output()
        .expect("binary runs");
    assert!(built.status.success(), "{built:?}");
    assert!(
        String::from_utf8_lossy(&built.stdout).contains("7/0/0/1/,-3,-2,-1,0,1,2,3/,false,true"),
        "{built:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_gen_seed_random_and_shrink_surface_stay_absent() {
    let dir = std::env::temp_dir().join(format!("topaz_std_gen_absent_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    for member in ["random", "seed", "shrink"] {
        let entry = dir.join(format!("{member}.tpz"));
        std::fs::write(&entry, format!("import std.gen\nlet bad = gen.{member}\n"))
            .expect("std.gen absent entry");
        let checked = topaz()
            .current_dir(&dir)
            .arg("check")
            .arg("--format")
            .arg("json")
            .arg(&entry)
            .output()
            .expect("binary runs");
        assert!(!checked.status.success(), "{checked:?}");
        let stderr = String::from_utf8_lossy(&checked.stderr);
        assert!(stderr.contains("\"code\":\"TPZ3009\""), "{stderr}");
        assert!(stderr.contains(member), "{stderr}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn using_resource_block_closes_file_in_run_and_build() {
    let dir =
        std::env::temp_dir().join(format!("topaz_using_resource_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let data = dir.join("data.txt");
    std::fs::write(&data, "old").expect("seed data");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        r#"function scenario(path: string) -> Result<string, string> {
  let mut later: () -> string = () => "unset"
  using file = open(path)? {
    later = () => match file.read() {
      case Ok(text) => text
      case Err(e) => e
    }
    match file.write("new") {
      case Ok(_) => ()
      case Err(e) => return Err(e)
    }
  }
  return Ok(later())
}

let result = match scenario("data.txt") {
  case Ok(text) => text
  case Err(e) => e
}
print(result)
"#,
    )
    .expect("using entry");

    let checked = rust_topaz()
        .current_dir(&dir)
        .arg("check")
        .arg("main.tpz")
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");

    let ran = rust_topaz()
        .current_dir(&dir)
        .arg("run")
        .arg("main.tpz")
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "file is closed\n");
    assert_eq!(std::fs::read_to_string(&data).expect("updated data"), "new");

    let build_dir = dir.join("build-out");
    let built = rust_topaz()
        .current_dir(&dir)
        .arg("build")
        .arg("main.tpz")
        .arg("--out-dir")
        .arg(&build_dir)
        .arg("--run")
        .output()
        .expect("binary runs");
    assert!(built.status.success(), "{built:?}");
    assert!(
        String::from_utf8_lossy(&built.stdout).contains("file is closed"),
        "{built:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_fs_and_io_wrap_the_host_boundary() {
    use std::io::Write;

    let dir = std::env::temp_dir().join(format!("topaz_std_fs_io_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let data = dir.join("data.txt");
    std::fs::write(&data, "old").expect("seed data");
    let entry = dir.join("main.tpz");
    let path = data.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        &entry,
        format!(
            r#"import std.fs
import std.io

let before = match fs.readText("{path}") {{
  case Ok(text) => text
  case Err(e) => e
}}
let wrote = match fs.writeText("{path}", io.readStdin()) {{
  case Ok(_) => "ok"
  case Err(e) => e
}}
let after = match fs.readText("{path}") {{
  case Ok(text) => text
  case Err(e) => e
}}
io.writeLine("{{before}}/{{wrote}}/{{after}}")
"#
        ),
    )
    .expect("std fs/io entry");

    let mut child = rust_topaz()
        .arg("run")
        .arg(&entry)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"new")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "old/ok/new\n");
    assert_eq!(std::fs::read_to_string(&data).expect("data"), "new");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The Markdown live editor's renderer (`examples/markdown-live/render.tpz`) is
/// pure Topaz: it reads the markdown via `input()` (here, piped stdin) and prints
/// HTML. This pins the rendered output for a representative document AND the
/// default-deny link security (a `javascript:` URL renders inert, no `href`).
#[test]
fn markdown_live_renderer_produces_safe_html() {
    use std::io::Write;
    use std::process::Stdio;
    let render = repo_root().join("../examples/markdown-live/render.tpz");
    // includes CRLF (`\r\n`), a protocol-relative link, an image, strikethrough, and a
    // backslash escape to pin the inline-feature hardening.
    let md = "# Title\r\n\r\nA **bold** word, *em*, `code`, ~~struck~~, \\*literal\\*, and a [link](https://topaz.dev).\n\n\
              ![logo](https://topaz.dev/logo.png) and ![evil](javascript:x) here.\n\n\
              - one\n- two\n\n1. a\n2. b\n\n> quote\n\n```\nx & <y>\n```\n\n\
              | Feature | State |\n|---------|:-----:|\n| links | **safe** |\n\n---\n\
              [xss](javascript:alert(1)) and [ext](//evil.com) and [bs](\\\\evil.com) stay inert";
    let mut child = topaz()
        .arg("run")
        .arg(&render)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(md.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("renderer completes");
    assert!(out.status.success(), "{out:?}");
    let html = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "<h1>Title</h1>",
        "<strong>bold</strong>",
        "<em>em</em>",
        "<code>code</code>",
        "<a href=\"https://topaz.dev\">link</a>",
        "<ul>",
        "<li>one</li>",
        "<ol>",
        "<blockquote>quote</blockquote>",
        "<pre><code>x &amp; &lt;y&gt;\n</code></pre>",
        "<hr>",
        "<del>struck</del>",
        "*literal*",
        "<img src=\"https://topaz.dev/logo.png\" alt=\"logo\">",
        "<table>",
        "<th>Feature</th>",
        "<td><strong>safe</strong></td>",
    ] {
        assert!(html.contains(needle), "missing {needle:?} in:\n{html}");
    }
    // §security: an unsafe image src must NOT render (no `src="javascript:`).
    assert!(
        !html.contains("src=\"javascript:"),
        "unsafe image src leaked:\n{html}"
    );
    // §security: javascript: AND protocol-relative URLs must NOT become an href
    // (default-deny allowlist); the link text survives, the href does not.
    assert!(
        !html.contains("href=\"javascript:")
            && !html.contains("href=\"//")
            && !html.contains("href=\"\\\\"),
        "unsafe URL leaked into an href:\n{html}"
    );
    assert!(
        html.contains("xss and ext and bs stay inert"),
        "link text dropped:\n{html}"
    );
    // CRLF input still splits + renders cleanly (the title carried a trailing \r).
    assert!(!html.contains('\r'), "CR leaked into output:\n{html}");
}

/// GFM table edge cases that must not produce false positives: a heading with
/// pipes stays a heading; a header/delimiter cell-count mismatch is not a table; a
/// mid-cell colon in the delimiter is not a table. And a valid table with an escaped
/// pipe (`\|`) keeps it as literal cell content.
#[test]
fn markdown_live_tables_do_not_false_positive() {
    use std::io::Write;
    use std::process::Stdio;
    let render = repo_root().join("../examples/markdown-live/render.tpz");
    let run = |md: &str| -> String {
        let mut child = topaz()
            .arg("run")
            .arg(&render)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("binary runs");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(md.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("renderer completes");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // a heading with pipes is a heading, not a stolen table header.
    let h = run("# a | b\n--- | ---");
    assert!(
        h.contains("<h1>a | b</h1>") && !h.contains("<table>"),
        "{h}"
    );
    // header has 2 cells, delimiter has 1 → not a table.
    let m = run("a | b\n| --- |");
    assert!(!m.contains("<table>"), "{m}");
    // a colon in the middle of a delimiter cell → not a table.
    let c = run("a | b\n| :--:-- | --- |");
    assert!(!c.contains("<table>"), "{c}");
    // a valid table; an escaped `\|` stays literal inside one cell.
    let t = run("| X | Y |\n|:--|--:|\n| 1 \\| 2 | z |");
    assert!(t.contains("<table>") && t.contains("<td>1 | 2</td>"), "{t}");
    // a table BODY ends at the next block start even if that line has a pipe — the
    // heading must not be swallowed as a table row.
    let b = run("| A | B |\n|---|---|\n| x | y |\n# h | i");
    assert!(
        b.contains("<td>x</td>") && b.contains("<h1>h | i</h1>"),
        "{b}"
    );
}

/// Living Docs: the renderer turns a ```` ```topaz ```` fence into an ordinal compute
/// placeholder (`<div data-topaz-block="N">`) for the JS shell to execute and fill, while a
/// plain fence stays a static, HTML-escaped `<pre><code>`. Block sources are NOT embedded in
/// the HTML (the shell extracts them from the source, in the same order).
#[test]
fn living_docs_renderer_emits_compute_placeholders() {
    use std::io::Write;
    use std::process::Stdio;
    let render = repo_root().join("../examples/living-docs/render.tpz");
    let md = "# Doc\n\n```topaz\n1 + 2\n```\n\n```\nstatic & <code>\n```\n\n```topaz\n\"hi\"\n```";
    let mut child = topaz()
        .arg("run")
        .arg(&render)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(md.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("renderer completes");
    assert!(out.status.success(), "{out:?}");
    let html = String::from_utf8_lossy(&out.stdout);
    // two topaz fences → ordinal placeholders 0 and 1
    assert!(
        html.contains("data-topaz-block=\"0\""),
        "missing block 0:\n{html}"
    );
    assert!(
        html.contains("data-topaz-block=\"1\""),
        "missing block 1:\n{html}"
    );
    // the source is NOT leaked into the placeholder HTML
    assert!(
        !html.contains("1 + 2") && !html.contains("&quot;hi&quot;"),
        "source leaked:\n{html}"
    );
    // a plain fence stays a static, escaped code block
    assert!(
        html.contains("<pre><code>static &amp; &lt;code&gt;\n</code></pre>"),
        "plain fence not static:\n{html}"
    );
}

/// The Korean-identifier renderer (`examples/living-docs/render-ko.tpz`) is a mechanical port
/// of `render.tpz` with all user identifiers renamed to Hangul — it must produce BYTE-IDENTICAL
/// HTML, so this catches drift between the two and proves Hangul identifiers work end-to-end.
#[test]
fn living_docs_korean_renderer_matches_english() {
    use std::io::Write;
    use std::process::Stdio;
    let md = "# 제목\n\nA **bold** word, `code`, ~~struck~~, [link](https://x.dev), ![i](/a.png).\n\n\
              - 하나\n  - 중첩\n- 둘\n\n1. a\n2. b\n\n> quote\n\n```topaz\n1 + 2\n```\n\n```\nx & <y>\n```\n\n---\n\
              | 항목 | 값 |\n|------|----|\n| 식비 | **3** |\n\n[xss](javascript:alert(1)) inert";
    let run = |renderer: &str| -> String {
        let mut child = rust_topaz()
            .arg("run")
            .arg(repo_root().join(renderer))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("binary runs");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(md.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("renderer completes");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let english = run("../examples/living-docs/render.tpz");
    let korean = run("../examples/living-docs/render-ko.tpz");
    assert_eq!(english, korean, "Korean renderer drifted from English");
    assert!(
        english.contains("<h1>제목</h1>") && english.contains("<th>항목</th>"),
        "{english}"
    );
    // A 2-space-indented item nests inside the previous `<li>`.
    assert!(
        english.contains("<li>하나\n<ul>\n<li>중첩</li>\n</ul>\n</li>"),
        "nested list missing:\n{english}"
    );
}

/// The 글자수 세기 (char-counter) example app exercises `str.byteLength()`: a Hangul scalar is
/// 3 UTF-8 bytes, so "가" → 1 scalar but 3 bytes. Pins the counter's stat output end-to-end.
#[test]
fn char_counter_counts_scalars_and_utf8_bytes() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = rust_topaz()
        .arg("run")
        .arg(repo_root().join("../examples/char-counter/count.tpz"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    // "가 b" → 3 scalars; 2 non-space; 2 words; 1 line; 가(3)+space(1)+b(1) = 5 bytes.
    child
        .stdin
        .take()
        .unwrap()
        .write_all("가 b".as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("counter completes");
    assert!(out.status.success(), "{out:?}");
    let html = String::from_utf8_lossy(&out.stdout);
    assert!(
        html.contains("<b>3</b><span>글자 수</span>"),
        "scalars: {html}"
    );
    assert!(
        html.contains("<b>5</b><span>바이트 (UTF-8)</span>"),
        "bytes: {html}"
    );
    assert!(
        html.contains("<b>2</b><span>단어 수</span>"),
        "words: {html}"
    );
}

/// The 목록 정리기 (list-organizer) example exercises `arr.sorted()` + dedup + HTML-escaping of
/// user lines. Pins sort order, adjacent-dedup, and that `<`/`&` in a line are escaped.
#[test]
fn list_organizer_sorts_dedups_and_escapes() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = topaz()
        .arg("run")
        .arg(repo_root().join("../examples/list-organizer/clean.tpz"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all("banana\n<b>\napple\nbanana\n".as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("organizer completes");
    assert!(out.status.success(), "{out:?}");
    let html = String::from_utf8_lossy(&out.stdout);
    // sorted + deduped (banana once), and the "<b>" line HTML-escaped, sorting before letters.
    assert!(
        html.contains("<li>&lt;b&gt;</li><li>apple</li><li>banana</li></ol>"),
        "sort/dedup/escape: {html}"
    );
    assert!(html.contains("고유 3줄"), "count: {html}");
    assert!(!html.contains("<b>"), "raw tag leaked: {html}");
}

/// The 초성 추출기 (choseong extractor) example exercises `str.codePointAt()`: a Hangul syllable
/// U+AC00..U+D7A3 maps to its leading consonant via (codepoint-0xAC00)/588; non-Hangul passes
/// through. Pins the extraction + HTML-escaping of user text.
#[test]
fn choseong_extracts_leading_consonants() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = topaz()
        .arg("run")
        .arg(repo_root().join("../examples/choseong/choseong.tpz"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all("안녕 <b> 초성검색!".as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("choseong completes");
    assert!(out.status.success(), "{out:?}");
    let html = String::from_utf8_lossy(&out.stdout);
    // 안→ㅇ 녕→ㄴ, space + escaped <b> pass through, 초성검색→ㅊㅅㄱㅅ, ! passes.
    assert!(
        html.contains("<div class=\"out\">ㅇㄴ &lt;b&gt; ㅊㅅㄱㅅ!</div>"),
        "choseong/escape: {html}"
    );
}

/// The 유니코드 변환기 (unicode-convert) example exercises the free `fromCodePoint(n)`: decimal
/// codepoints → text, with invalid tokens (non-number / surrogate / out-of-range) listed, not
/// dropped. Pins the conversion + invalid handling + HTML-escaping.
#[test]
fn unicode_convert_builds_text_from_codepoints() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = topaz()
        .arg("run")
        .arg(repo_root().join("../examples/unicode-convert/convert.tpz"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    // 44032=가 110=n, then a non-number and a surrogate are invalid.
    child
        .stdin
        .take()
        .unwrap()
        .write_all("44032 110 xyz 55296".as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("convert completes");
    assert!(out.status.success(), "{out:?}");
    let html = String::from_utf8_lossy(&out.stdout);
    assert!(html.contains("<div class=\"out\">가n</div>"), "out: {html}");
    assert!(
        html.contains("무효 2개: xyz 55296 "),
        "invalids listed: {html}"
    );
}

/// The JSON 포매터/검증기 example is a full recursive-descent JSON parser in Topaz: it
/// pretty-prints valid JSON (2-space indent, token text preserved) and reports Korean
/// errors with line:column. Pins both paths.
#[test]
fn json_format_pretty_prints_and_reports_errors() {
    use std::io::Write;
    use std::process::Stdio;
    let run = |input: &str| -> String {
        let mut child = rust_topaz()
            .arg("run")
            .arg(repo_root().join("../examples/json-format/format.tpz"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("binary runs");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("formatter completes");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    // valid: pretty-printed with 2-space indent, number text + Korean string preserved.
    let ok = run("{\"a\":[1,2.5],\"b\":\"가\"}");
    assert!(
        ok.contains(
            "<pre class=\"ok\">{\n  \"a\": [\n    1,\n    2.5\n  ],\n  \"b\": \"가\"\n}</pre>"
        ),
        "pretty-print: {ok}"
    );
    // invalid: Korean error with line:column.
    let err = run("{\"a\" 1}");
    assert!(
        err.contains("오류 1:6: 콜론(:)이 필요합니다"),
        "error: {err}"
    );
    // \uXXXX decode: a safe printable escape becomes the actual char (toInt(hex,16) +
    // fromCodePoint); the escaped quote " stays escaped.
    let dec = run("{\"k\":\"\\uAC00\\u0022\"}");
    assert!(dec.contains("\"k\": \"가\\u0022\""), "decode: {dec}");
}

/// v5.6 dogfood: the six concrete "Rust replacement" examples are executable, not
/// just plan text. Interpreter, boxed Rust, and generated Python outputs pin the
/// data-format, stdlib, runtime-module, and explicit-main paths.
#[test]
fn dogfood_v56_examples_run_and_build_across_backends() {
    let manifest = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n[dependencies]\nstd = \"5.4\"\ncodec = \"1\"\n";
    let corpus = r#"{"fixtures":[{"dir":"control","result":"ok"},{"dir":"values","result":"error"},{"dir":"control","result":"ok"}]}"#;
    let csv = "name,kind,path\nParser,function,/parser\nValue,type,/value\n";
    let dir = std::env::temp_dir().join(format!("topaz_dogfood_examples_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let input = dir.join("input.bin");
    std::fs::write(&input, b"abc").expect("seed hash input");
    let cases = [
        (
            "manifest-audit",
            manifest,
            "manifest demo@0.1.0 deps=codec,std",
        ),
        (
            "corpus-report",
            corpus,
            "fixtures=3 ok=2 error=1 dirs=control,values",
        ),
        (
            "signature-site",
            csv,
            "# API\n\n## Parser\n\n- kind: function\n- path: /parser\n- anchor: #Parser\n\n## Value\n\n- kind: type\n- path: /value\n- anchor: #Value\n",
        ),
        ("http-handler", "", "200:ok:abc"),
        ("mini-expr", "2 + 3 * 4", "value=20"),
        (
            "hash-tool",
            "",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
    ];

    for (name, stdin, expected) in cases {
        let source = repo_root().join(format!("../examples/dogfood/{name}.tpz"));
        let add_hash_args = |command: &mut Command| {
            if name == "hash-tool" {
                command.arg("--").arg("--input").arg(&input);
            }
        };

        let mut command = rust_topaz();
        command.arg("run").arg(&source);
        add_hash_args(&mut command);
        let out = output_with_stdin(command, stdin.as_bytes());
        assert!(out.status.success(), "interpreter {name}: {out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim_end(),
            expected.trim_end(),
            "interpreter {name}"
        );

        let mut command = rust_topaz();
        command
            .arg("build")
            .arg(&source)
            .arg("--out-dir")
            .arg(dir.join(format!("{name}-rust")))
            .arg("--run");
        add_hash_args(&mut command);
        let out = output_with_stdin(command, stdin.as_bytes());
        assert!(out.status.success(), "boxed Rust {name}: {out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim_end(),
            expected.trim_end(),
            "boxed Rust {name}"
        );

        if let Some(python) = cpython_31314() {
            let out_dir = dir.join(format!("{name}-python"));
            let out = rust_topaz()
                .arg("build")
                .arg(&source)
                .arg("--target")
                .arg("python")
                .arg("--out-dir")
                .arg(&out_dir)
                .output()
                .expect("Python dogfood artifact builds");
            assert!(out.status.success(), "Python build {name}: {out:?}");
            let mut command = Command::new(python);
            if name == "hash-tool" {
                command
                    .current_dir(&out_dir)
                    .arg("-c")
                    .arg("import sys; import program; sys.stdout.write(program.run('', files={'input.bin': 'abc'}, args=['--input', 'input.bin']) + '\\n')");
            } else {
                command.arg(out_dir.join("program.py"));
            }
            let out = output_with_stdin(command, stdin.as_bytes());
            assert!(out.status.success(), "Python run {name}: {out:?}");
            let quoted_stdout = expected
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            let expected_files = if name == "hash-tool" {
                "[{\"path\":\"input.bin\",\"content\":{\"str\":\"abc\"}}]"
            } else {
                "[]"
            };
            let expected_trace = format!(
                "{{\"v\":1,\"status\":\"ok\",\"stdout\":[\"{quoted_stdout}\"],\"files\":{expected_files},\"defer_errors\":[],\"fault\":null}}"
            );
            assert_eq!(
                String::from_utf8_lossy(&out.stdout).trim_end(),
                expected_trace,
                "Python {name}"
            );
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lispex_readiness_probes_match_across_backends() {
    let dir = std::env::temp_dir().join(format!("topaz_lispex_readiness_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    for (name, expected) in [
        ("lispex-recursive-values", "recursive-size=9"),
        ("lispex-cell-state", "cell=10,15,12"),
        ("lispex-trampoline", "trampoline=50005000"),
        (
            "lispex-deterministic-surface",
            "json:1:2:expected a string key in object",
        ),
        ("lispex-portable-data", "tags=data,portable"),
    ] {
        let source = repo_root().join(format!("../examples/readiness/{name}.tpz"));
        let out = rust_topaz()
            .arg("run")
            .arg(&source)
            .output()
            .expect("interpreter probe runs");
        assert!(out.status.success(), "{name}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(expected),
            "{name}: {out:?}"
        );

        let out = rust_topaz()
            .arg("build")
            .arg(&source)
            .arg("--out-dir")
            .arg(dir.join(format!("{name}-rust")))
            .arg("--run")
            .output()
            .expect("boxed Rust probe builds and runs");
        assert!(out.status.success(), "{name}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(expected),
            "{name}: {out:?}"
        );

        if let Some(python) = cpython_31314() {
            let out_dir = dir.join(format!("{name}-python"));
            let out = rust_topaz()
                .arg("build")
                .arg(&source)
                .arg("--target")
                .arg("python")
                .arg("--out-dir")
                .arg(&out_dir)
                .output()
                .expect("Python probe artifact builds");
            assert!(out.status.success(), "{name}: {out:?}");
            let out = Command::new(&python)
                .arg(out_dir.join("program.py"))
                .output()
                .expect("Python probe runs");
            assert!(out.status.success(), "{name}: {out:?}");
            assert!(
                String::from_utf8_lossy(&out.stdout).contains(expected),
                "{name}: {out:?}"
            );
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_math_boundary_audit_matches_across_backends() {
    let source = repo_root().join("corpus/v5_4/stdlib/math-boundaries.tpz");
    let expected =
        std::fs::read_to_string(repo_root().join("corpus/v5_4/stdlib/math-boundaries.stdout"))
            .expect("math boundary golden")
            .trim()
            .to_string();
    let dir = std::env::temp_dir().join(format!("topaz_math_audit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let out = rust_topaz()
        .arg("run")
        .arg(&source)
        .output()
        .expect("interpreter math audit runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&expected),
        "{out:?}"
    );

    let out = rust_topaz()
        .arg("build")
        .arg(&source)
        .arg("--out-dir")
        .arg(dir.join("rust"))
        .arg("--run")
        .output()
        .expect("boxed Rust math audit builds and runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&expected),
        "{out:?}"
    );

    if let Some(python) = cpython_31314() {
        let out_dir = dir.join("python");
        let out = rust_topaz()
            .arg("build")
            .arg(&source)
            .arg("--target")
            .arg("python")
            .arg("--out-dir")
            .arg(&out_dir)
            .output()
            .expect("Python math audit artifact builds");
        assert!(out.status.success(), "{out:?}");
        let out = Command::new(python)
            .arg(out_dir.join("program.py"))
            .output()
            .expect("Python math audit runs");
        assert!(out.status.success(), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(&expected),
            "{out:?}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
