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
        "topaz-5.19.2-numeric",
        "lispex-r7rs-small-pinned/v1.4",
        "topaz-5.19.2/numeric.lspx",
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
fn completed_numeric_capabilities_match_reference_vectors() {
    let source = r#"(% -13 4)
(modulo -13 4)
(floor-remainder -13 4)
(remainder -13 4)
(truncate-remainder -13 4)
(floor-quotient -13 4)
(quotient -13 4)
(truncate-quotient -13 4)
(floor/ -13 4)
(truncate/ -13 4)
(abs -9/4)
(square -9/4)
(floor -7/2)
(ceiling -7/2)
(truncate -7/2)
(round 5/2)
(round 7/2)
(zero? 0/3)
(positive? 1/3)
(negative? -0.5)
(rational? 1)
(real? 1/2)
(complex? 1.0)
(even? -8)
(odd? -7)
(exact 0.5)
(inexact->exact 0.25)
(inexact 1/4)
(exact->inexact 3)
(min 8 -2 3/2)
(max 8 -2 3/2)
(gcd 24 -18 30)
(lcm 4 -6 10)
(expt 2 10)
(expt 2 -3)
(expt -1 4294967297)
(exact-integer-sqrt 10)
(modulo 13 -4)
(modulo -13 -4)
(remainder 13 -4)
(remainder -13 -4)
(floor/ 13.0 -4)
(truncate/ -13 4.0)
(round -5/2)
(round -7/2)
(round 2.5)
(round 3.5)
(min 1 2.0)
(max 2 1.0)
(gcd)
(lcm)
(gcd 12.0 8)
(lcm 4 6.0)
(expt 0 0)
(expt 0.0 0)
(expt 0 4)
(expt 1 -999999999999999999999999)
(expt -1 -999999999999999999999999)
(exact-integer-sqrt 0)
(exact-integer-sqrt 999999999999999999999999999999999999)
(rational? (quote x))
(real? #f)
(complex? "x")
(guard (e (else (error-object-message e))) (modulo 1 0))
(guard (e (else (error-object-message e))) (quotient 1/2 2))
(guard (e (else (error-object-message e))) (even? 1/2))
(guard (e (else (error-object-message e))) (expt 2 3.0))
(guard (e (else (error-object-message e))) (exact-integer-sqrt -1))
(guard (e (else (error-object-message e))) (exact-integer-sqrt 1/2))
(guard (e (else (error-object-message e))) (abs (quote x)))
(guard (e (else (error-object-message e))) (min))
(guard (e (else (error-object-message e))) (floor 1 2))
(guard (e (else (error-object-message e))) (expt 0 -1))
(guard (e (else (error-object-message e))) (expt 0.0 -1))
"#;
    let result = run_lit_source(source);
    let row = result.as_array().expect("source result is an array");
    assert_eq!(row[2], "ok");
    assert_eq!(row[5], 0);
    assert_eq!(byte_channel(&row[4]), "");
    assert_eq!(
        byte_channel(&row[3]),
        r#"3
3
3
-1
-1
-4
-3
-3
-4
3
-3
-1
9/4
81/16
-4
-3
-3
2
4
#t
#t
#t
#t
#t
#t
#t
#t
1/2
1/4
0.25
3.0
-2
8
6
60
1024
1/8
-1
3
1
-3
-1
1
-1
-4.0
-3.0
-3.0
-1.0
-2
-4
2.0
4.0
1.0
2.0
0
1
4.0
12.0
1
1.0
0
1
-1
0
0
999999999999999999
1999999999999999998
#f
#f
#f
"modulo: division by zero"
"quotient: integer required"
"even?: integer required"
"expt: exact integer required"
"exact-integer-sqrt: non-negative integer required"
"exact-integer-sqrt: exact integer required"
"abs: number required"
"min: expected at least 1 argument, got 0"
"floor: expected 1 arguments, got 2"
"expt: division by zero"
"expt: inexact result is not finite"
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
