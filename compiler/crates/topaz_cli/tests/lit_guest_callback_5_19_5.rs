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
        "topaz-5.19.5-guest-callback",
        "lispex-r7rs-small-pinned/v1.4",
        "topaz-5.19.5/guest-callback.lspx",
        sha256(source.as_bytes()),
        source.as_bytes(),
        [4_000_000, 200_000, 4_000_000, 10_000_000, 1_048_576],
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
fn completed_guest_callback_capabilities_match_reference_vectors() {
    let source = r#"(map (lambda (x) (* x 2)) (list 1 2 3))
(filter (lambda (x) (odd? x)) (list 1 2 3 4))
(all? (lambda (x) (positive? x)) (list 1 2 3))
(any? (lambda (x) (zero? x)) (list 1 0 2))
(let ((n 0)) (list (any? (lambda (x) (begin (set! n (+ n 1)) (= x 2))) (list 1 2 3)) n))
(let ((n 0)) (list (all? (lambda (x) (begin (set! n (+ n 1)) (< x 2))) (list 1 2 3)) n))
(begin (for-each display (list "a" "b" "c")) 7)
(reduce (lambda (acc x) (- acc x)) 10 (list 1 2 3))
(fold-left (lambda (acc x) (- acc x)) 10 (list 1 2 3))
(fold-right (lambda (x acc) (- x acc)) 0 (list 1 2 3))
(string-map char-upcase "aßω")
(begin (string-for-each display "aλ") 8)
(vector-map square (vector 2 3 4))
(begin (vector-for-each display (vector 1 "x" 2)) 9)
(map 1 (list))
(vector-map 1 (vector))
(string-map 1 "")
(reduce 1 9 (list))
(call/cc (lambda (exit) (map (lambda (x) (if (= x 2) (exit x) x)) (list 1 2 3))))
(guard (e (else (error-object-message e))) (any? (lambda (x) x) (list 1)))
(guard (e (else (error-object-message e))) (map (lambda (x) (values)) (list 1)))
(guard (e (else (error-object-message e))) (string-map (lambda (x) 1) "a"))
(guard (e (else (error-object-message e))) (map (lambda (x) x) (cons 1 2)))
"#;
    let result = run_lit_source(source);
    let row = result.as_array().expect("source result is an array");
    assert_eq!(row[2], "ok");
    assert_eq!(row[5], 0);
    assert_eq!(byte_channel(&row[4]), "");
    assert_eq!(
        byte_channel(&row[3]),
        r#"(2 4 6)
(1 3)
#t
#t
(#t 2)
(#f 2)
abc7
4
4
2
"AßΩ"
aλ8
#(4 9 16)
1x29
()
#()
""
9
2
"any?: expected a boolean predicate result, got 1"
"a single value is required here, but the procedure produced zero or multiple values"
"string-map: expected a character from the procedure, got 1"
"map: expected a proper list, got 2"
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
    assert!(
        machine_metrics.contains("[\"supported-may-call-guest-count\",18]"),
        "{machine_metrics}"
    );
}
