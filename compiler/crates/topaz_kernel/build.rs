//! Build-time provenance collection stays dependency-free so the bootstrap
//! kernel can hash its source inventory without extending its build graph.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../topaz_stage1_runtime/build_support/repository_file_identity.rs"]
mod repository_file_identity;

use repository_file_identity::git_stored_bytes;

const SOURCE_ROOTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "NOTICE",
    "licenses",
    "contracts/compiler",
    "crates",
    "tools",
];

fn main() {
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let compiler = crate_dir.join("../..");
    let compiler = compiler.canonicalize().expect("compiler root");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));

    let mut sources = Vec::new();
    for relative in SOURCE_ROOTS {
        let path = compiler.join(relative);
        println!("cargo:rerun-if-changed={}", path.display());
        collect_source_files(&compiler, &path, &mut sources);
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));

    let source_set_id = set_digest(
        sources
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    let vendor_root = compiler.join("vendor/stage0-recovery");
    println!("cargo:rerun-if-changed={}", vendor_root.display());
    let mut vendor_packages = fs::read_dir(&vendor_root)
        .expect("vendored dependencies")
        .map(|entry| entry.expect("vendor entry"))
        .filter(|entry| entry.file_type().expect("vendor type").is_dir())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let mut files = Vec::new();
            collect_all_files(&entry.path(), &entry.path(), &mut files);
            files.sort_by(|left, right| left.0.cmp(&right.0));
            let digest = set_digest(
                files
                    .iter()
                    .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
            );
            (name, digest, files.len() as u64)
        })
        .collect::<Vec<_>>();
    vendor_packages.sort_by(|left, right| left.0.cmp(&right.0));
    let vendor_set_id =
        set_digest(vendor_packages.iter().map(|(name, digest, count)| {
            (name.as_str(), format!("{digest}\0{count}").into_bytes())
        }));

    let mut generated = String::new();
    writeln!(
        generated,
        "pub const COMPILER_SOURCE_SET_ID: &str = \"sha256:{source_set_id}\";"
    )
    .unwrap();
    generated.push_str("pub const COMPILER_SOURCE_FILES: &[(&str, &str, u64)] = &[\n");
    for (path, bytes) in &sources {
        writeln!(
            generated,
            "    ({path:?}, \"sha256:{}\", {}),",
            sha256_hex(bytes),
            bytes.len()
        )
        .unwrap();
    }
    generated.push_str("];\n");
    writeln!(
        generated,
        "pub const VENDOR_SET_ID: &str = \"sha256:{vendor_set_id}\";"
    )
    .unwrap();
    generated.push_str("pub const VENDOR_PACKAGES: &[(&str, &str, u64)] = &[\n");
    for (name, digest, count) in &vendor_packages {
        writeln!(generated, "    ({name:?}, \"sha256:{digest}\", {count}),").unwrap();
    }
    generated.push_str("];\n");
    for (name, relative) in [
        ("CARGO_LOCK_SHA256", "Cargo.lock"),
        ("RUST_TOOLCHAIN_SHA256", "rust-toolchain.toml"),
        (
            "SCHEMA_REGISTRY_SHA256",
            "contracts/compiler/v1/schemas.json",
        ),
        (
            "BOOTSTRAP_PROFILE_SHA256",
            "contracts/compiler/v1/bootstrap-profile.json",
        ),
    ] {
        let bytes = git_stored_bytes(
            fs::read(compiler.join(relative)).expect("provenance input"),
            true,
        );
        writeln!(
            generated,
            "pub const {name}: &str = \"sha256:{}\";",
            sha256_hex(&bytes)
        )
        .unwrap();
    }
    writeln!(
        generated,
        "pub const BUILD_TARGET: &str = {:?};",
        std::env::var("TARGET").expect("TARGET")
    )
    .unwrap();
    writeln!(
        generated,
        "pub const BUILD_PROFILE: &str = {:?};",
        std::env::var("PROFILE").expect("PROFILE")
    )
    .unwrap();
    fs::write(out.join("bootstrap_build_provenance.rs"), generated)
        .expect("write build provenance");
}

fn collect_source_files(root: &Path, path: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    if path.is_dir() {
        let mut entries = fs::read_dir(path)
            .expect("source directory")
            .map(|entry| entry.expect("source entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            if entry
                .file_name()
                .is_some_and(|name| name == "target" || name == ".git")
            {
                continue;
            }
            collect_source_files(root, &entry, files);
        }
    } else if path.is_file() {
        let relative = path
            .strip_prefix(root)
            .expect("compiler-relative source")
            .to_string_lossy()
            .replace('\\', "/");
        files.push((
            relative,
            git_stored_bytes(fs::read(path).expect("source bytes"), true),
        ));
    }
}

fn collect_all_files(root: &Path, path: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    if path.is_dir() {
        let mut entries = fs::read_dir(path)
            .expect("vendor directory")
            .map(|entry| entry.expect("vendor entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            collect_all_files(root, &entry, files);
        }
    } else if path.is_file() {
        let relative = path
            .strip_prefix(root)
            .expect("vendor-relative source")
            .to_string_lossy()
            .replace('\\', "/");
        files.push((
            relative,
            git_stored_bytes(fs::read(path).expect("vendor bytes"), false),
        ));
    }
}

fn set_digest<'a, I, T>(entries: I) -> String
where
    I: IntoIterator<Item = (&'a str, T)>,
    T: AsRef<[u8]>,
{
    let mut framed = Vec::new();
    for (path, bytes) in entries {
        let bytes = bytes.as_ref();
        framed.extend_from_slice(path.as_bytes());
        framed.push(0);
        framed.extend_from_slice(bytes.len().to_string().as_bytes());
        framed.push(0);
        framed.extend_from_slice(sha256_hex(bytes).as_bytes());
        framed.push(b'\n');
    }
    sha256_hex(&framed)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut message = bytes.to_vec();
    let bit_len = (message.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    let mut state = INITIAL;
    for chunk in message.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}
