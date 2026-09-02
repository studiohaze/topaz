use super::support::*;

fn first_signature_hash(json: &str) -> String {
    let marker = "\"signatureHash\":\"";
    let start = json.find(marker).expect("signature hash present") + marker.len();
    let rest = &json[start..];
    let end = rest.find('"').expect("signature hash terminates");
    rest[..end].to_string()
}

#[test]
fn check_exports_json_emits_checked_public_surface() {
    let dir = std::env::temp_dir().join(format!("topaz_cli_exports_json_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create tempdir");
    let lib = dir.join("lib.tpz");
    let types = dir.join("types.tpz");
    let main = dir.join("main.tpz");
    std::fs::write(
        &lib,
        "export record User derives Show { name: string, age: int = 0 }\n\
         export record Box<T> { value: T }\n\
         export enum Msg derives Show { Noop, Inc(int), Pair(string, int) }\n\
         export enum Maybe<T> { Missing, Present(T) }\n\
         export newtype UserId = int\n\
         export newtype Id<T> = T\n\
         export type UserName = string\n\
         export function render<T: Show>(value: T, prefix: string = \"\") -> string {\n\
           prefix + Show.show(value)\n\
         }\n\
         export function tag(msg: Msg, id: UserId) -> string {\n\
           Show.show(msg) + \":\" + \"{id.value()}\"\n\
         }\n",
    )
    .expect("write lib");
    std::fs::write(
        &types,
        "export enum Msg { Noop, Inc(int) }\n\
         export newtype UserId = int\n\
         export function noop() -> Msg { Msg.Noop }\n\
         export function userId(n: int) -> UserId { UserId(n) }\n",
    )
    .expect("write types");
    std::fs::write(
        &main,
        "import types as T\n\
         import lib { render, User, Msg, UserId, tag }\n\
         let viaNs: T.Msg = T.noop()\n\
         let idNs: T.UserId = T.userId(8)\n\
         let s = render(User { name: \"Ada\" }) + tag(Msg.Inc(1), UserId(7))\n\
         print(s)\n",
    )
    .expect("write main");

    let out = rust_topaz()
        .arg("check")
        .arg("--root")
        .arg(&dir)
        .arg("--exports-json")
        .arg(&main)
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).trim().is_empty(),
        "{out:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.trim();
    assert!(line.starts_with("{\"modules\":["), "{stdout}");
    assert!(line.contains("\"identity\":\"lib\""), "{stdout}");
    assert!(line.contains("\"signatureHash\":\"sha256:"), "{stdout}");
    assert!(line.contains("\"name\":\"render\""), "{stdout}");
    assert!(
        line.contains("\"type\":\"(?0, string) -> string\""),
        "{stdout}"
    );
    assert!(line.contains("\"bounds\":[[\"Show\"]]"), "{stdout}");
    assert!(line.contains("\"defaulted\":[false,true]"), "{stdout}");
    assert!(line.contains("\"name\":\"UserName\""), "{stdout}");
    assert!(line.contains("\"name\":\"Box\",\"params\":1"), "{stdout}");
    assert!(
        line.contains("\"name\":\"User\",\"params\":0,\"fields\":[{\"name\":\"name\""),
        "{stdout}"
    );
    assert!(line.contains("\"name\":\"Maybe\",\"params\":1"), "{stdout}");
    assert!(
        line.contains(
            "\"name\":\"Msg\",\"params\":0,\"variants\":[{\"name\":\"Noop\",\"payloads\":[]},{\"name\":\"Inc\",\"payloads\":[\"int\"]},{\"name\":\"Pair\",\"payloads\":[\"string\",\"int\"]}]}"
        ),
        "{stdout}"
    );
    assert!(
        line.contains("\"name\":\"Id\",\"params\":1,\"base\":\"?0\""),
        "{stdout}"
    );
    assert!(
        line.contains("\"name\":\"UserId\",\"params\":0,\"base\":\"int\""),
        "{stdout}"
    );
    assert!(
        line.contains(
            "\"conformances\":[{\"protocol\":\"Show\",\"type\":\"Msg\"},{\"protocol\":\"Show\",\"type\":\"User\"}]"
        ),
        "{stdout}"
    );
    assert!(!line.contains("types-ok"), "{stdout}");
}

#[test]
fn exports_json_signature_hash_tracks_public_surface() {
    let dir = std::env::temp_dir().join(format!("topaz_cli_exports_hash_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create tempdir");
    let lib = dir.join("lib.tpz");

    std::fs::write(&lib, "export function answer(x: int) -> int { x + 1 }\n").expect("write lib");
    let hash_one = check_signature_hash(&dir, &lib);

    std::fs::write(&lib, "export function answer(x: int) -> int { x + 2 }\n")
        .expect("rewrite lib body");
    let hash_two = check_signature_hash(&dir, &lib);
    assert_eq!(hash_one, hash_two);

    std::fs::write(
        &lib,
        "export function answer(x: int) -> string { \"ok\" }\n",
    )
    .expect("rewrite lib signature");
    let hash_three = check_signature_hash(&dir, &lib);
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(hash_one, hash_three);
}

fn check_signature_hash(root: &Path, entry: &Path) -> String {
    let out = topaz()
        .arg("check")
        .arg("--root")
        .arg(root)
        .arg("--exports-json")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).trim().is_empty(),
        "{out:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let hash = first_signature_hash(stdout.trim());
    assert!(hash.starts_with("sha256:"), "{hash}");
    assert_eq!(hash.len(), "sha256:".len() + 64, "{hash}");
    hash
}

#[test]
fn exports_json_is_rejected_outside_check() {
    let out = rust_topaz()
        .arg("run")
        .arg("--exports-json")
        .arg("whatever.tpz")
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("applies to `check` only"),
        "{out:?}"
    );
}
