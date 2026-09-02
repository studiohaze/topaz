use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn compiler_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn lit_source() -> PathBuf {
    compiler_root().join("lit/lit.tpz")
}

fn sha256(bytes: &[u8]) -> String {
    topaz_package::manifest_sha256(std::str::from_utf8(bytes).expect("test input is UTF-8"))
        .strip_prefix("sha256:")
        .expect("manifest digest has the sha256 prefix")
        .to_owned()
}

fn run_lit_source(source: &str) -> Value {
    let lit_bytes = std::fs::read(lit_source()).expect("read canonical LIT source");
    let request = json!([
        "lispex.hosted-source-backend-request/v0",
        "topaz-5.19.3-list-search",
        "lispex-r7rs-small-pinned/v1.4",
        "topaz-5.19.3/list-search.lspx",
        sha256(source.as_bytes()),
        source.as_bytes(),
        [2_000_000, 100_000, 2_000_000, 5_000_000, 1_048_576],
        [
            sha256(&lit_bytes),
            "7252cc91ce3685d1085e127777977ee9938f092e015bd37d7e56b4fc556df1ee",
            "72bc588742e499cd47dc8f9b01751743535e0ed97bb2969527b00a1291695840"
        ],
        [1, false]
    ]);
    let mut input = serde_json::to_vec(&request).expect("encode source request");
    input.push(b'\n');

    let mut child = Command::new(env!("CARGO_BIN_EXE_topaz"))
        .args(["--compiler", "rust", "run"])
        .arg(lit_source())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the Topaz interpreter");
    child
        .stdin
        .as_mut()
        .expect("LIT stdin is piped")
        .write_all(&input)
        .expect("write source request");
    let output = child.wait_with_output().expect("LIT completes");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("LIT result is JSON")
}

fn byte_channel(value: &Value) -> String {
    String::from_utf8(
        value
            .as_array()
            .expect("byte channel is an array")
            .iter()
            .map(|value| {
                u8::try_from(value.as_u64().expect("byte channel member is an integer"))
                    .expect("byte channel member is in range")
            })
            .collect(),
    )
    .expect("byte channel is UTF-8")
}

#[test]
fn completed_list_search_capabilities_match_reference_vectors() {
    let source = r#"(define tree '((((a . b) . (c . d)) . ((e . f) . (g . h))) . (((i . j) . (k . l)) . ((m . n) . (o . p)))))
(caar tree)
(cadr tree)
(cdar tree)
(cddr tree)
(caaar tree)
(caadr tree)
(cadar tree)
(caddr tree)
(cdaar tree)
(cdadr tree)
(cddar tree)
(cdddr tree)
(caaaar tree)
(caaadr tree)
(caadar tree)
(caaddr tree)
(cadaar tree)
(cadadr tree)
(caddar tree)
(cadddr tree)
(cdaaar tree)
(cdaadr tree)
(cdadar tree)
(cdaddr tree)
(cddaar tree)
(cddadr tree)
(cdddar tree)
(cddddr tree)
(list-ref '(10 20 30) 1)
(nth '(10 20 30) 2)
(list-tail '(10 20 30) 1)
(list-tail 7 0)
(make-list 3 'x)
(make-list 2)
(list-copy '(1 2 . 3))
(memv 'b '(a b c))
(assoc '(a) '(((a) . 1) ((b) . 2)))
(assv 'b '((a . 1) (b . 2)))
(assq 'b '((a . 1) (b . 2)))
(list-first '(8 9))
(list-rest '(8 9))
(let ((xs '(1 2 3))) (eq? (list-tail xs 1) (cdr xs)))
(let ((xs '(1 2 3))) (eq? (list-copy xs) xs))
(guard (e (else (error-object-message e))) (list-ref '(1) 2))
(guard (e (else (error-object-message e))) (list-tail '(1) 2))
(guard (e (else (error-object-message e))) (memv 'z '(a . b)))
(guard (e (else (error-object-message e))) (assoc 'z '((a . 1) bad)))
(guard (e (else (error-object-message e))) (caaaar (cons 1 2)))
"#;
    let result = run_lit_source(source);
    let row = result.as_array().expect("source result is an array");
    assert_eq!(row[2], "ok");
    assert_eq!(row[5], 0);
    assert_eq!(byte_channel(&row[4]), "");
    assert_eq!(
        byte_channel(&row[3]),
        r#"((a . b) c . d)
((i . j) k . l)
((e . f) g . h)
((m . n) o . p)
(a . b)
(i . j)
(e . f)
(m . n)
(c . d)
(k . l)
(g . h)
(o . p)
a
i
e
m
c
k
g
o
b
j
f
n
d
l
h
p
20
30
(20 30)
7
(x x x)
(0 0)
(1 2 . 3)
(b c)
((a) . 1)
(b . 2)
(b . 2)
8
(9)
#t
#f
"list-ref: index 2 out of range (length 1)"
"list-tail: index 2 out of range"
"memv: expected a proper list, got b"
"assoc: expected an association list (each entry a pair), got bad"
"caaaar: expected a pair, got 1"
"#
    );

    let machine_metrics = serde_json::to_string(&row[9]).expect("encode machine metrics");
    assert!(
        machine_metrics.contains("[\"hosted-implemented-count\",174]"),
        "{machine_metrics}"
    );
    assert!(
        machine_metrics.contains("[\"unsupported-loud-count\",0]"),
        "{machine_metrics}"
    );
}
