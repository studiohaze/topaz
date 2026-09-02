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
        "topaz-5.19.4-character-string-io",
        "lispex-r7rs-small-pinned/v1.4",
        "topaz-5.19.4/character-string-io.lspx",
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
fn completed_character_string_io_capabilities_match_reference_vectors() {
    let source = r#"(!= (list 1 2) (list 1 2))
(!= 1 2)
(boolean=? #t #t #t)
(boolean=? #t #f #f)
(symbol=? (string->symbol "alpha") (string->symbol "alpha"))
(symbol=? (string->symbol "alpha") (string->symbol "beta"))
(char<? #\A #\B #\C)
(char>=? #\C #\C #\A)
(char-ci=? #\A #\a)
(char-ci<? #\a #\B)
(char-upper-case? #\Ω)
(char-lower-case? #\ω)
(char-whitespace? #\space)
(char-whitespace? #\A)
(char-upcase #\ß)
(char-downcase #\İ)
(char-foldcase #\A)
(string<? "alpha" "beta" "gamma")
(string>=? "γ" "β" "α")
(string-ci=? "ΟΣ" "ος")
(string-ci<? "A" "b")
(string-upcase "straße")
(string-downcase "ΟΣ")
(string-downcase "ΟΣΑ")
(string-downcase "İ")
(string-foldcase "ΟΣ")
(make-string 3 #\λ)
(make-string 2)
(string->vector "aλz")
(string->vector "aλz" 1 2)
(vector->string (vector #\a #\λ #\z))
(vector->string (vector 7 #\λ 9) 1 2)
(begin (display "d") 11)
(begin (write "w") 12)
(begin (newline) 13)
(begin (println #\x) 14)
(guard (e (else (error-object-message e))) (boolean=? #t 1 #t))
(guard (e (else (error-object-message e))) (string<? "a" 2 "c"))
(guard (e (else (error-object-message e))) (vector->string (vector 1 #\a) 1 2))
(guard (e (else (error-object-message e))) (vector->string (vector #\a 1)))
(guard (e (else (error-object-message e))) (string->vector "abc" 2 1))
"#;
    let result = run_lit_source(source);
    let row = result.as_array().expect("source result is an array");
    assert_eq!(row[2], "ok");
    assert_eq!(row[5], 0);
    assert_eq!(byte_channel(&row[4]), "");
    assert_eq!(
        byte_channel(&row[3]),
        r#"#f
#t
#t
#f
#t
#f
#t
#t
#t
#t
#t
#t
#t
#f
#\ß
#\i
#\a
#t
#t
#t
#t
"STRASSE"
"ος"
"οσα"
"i̇"
"ος"
"λλλ"
"  "
#(#\a #\λ #\z)
#(#\λ)
"aλz"
"λ"
d11
"w"12

13
x
14
"boolean=?: expected a boolean, got 1"
"string<?: expected a string, got 2"
"a"
"vector->string: expected a vector of characters, got 1"
"string->vector: start 2 is past end 1"
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
