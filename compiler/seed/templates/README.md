# Topaz compiler seed

This source builds the Topaz {{VERSION}} compiler from its generated Rust and six native runtime crates. The compiler written in Topaz is in `topaz/`.

## Bootstrap

Install Rust {{RUST_VERSION}} with Cargo. On macOS or Linux, run this command from the extracted directory.

```sh
sh bootstrap.sh
```

On Windows, use PowerShell.

```powershell
./bootstrap.ps1
```

The script builds the seed with Cargo in offline mode, reads the compiler sources in `topaz/`, regenerates Rust and compares it with `generated/compiler.rs`. The result is saved as `target/bootstrap-regenerated.rs`. No private repository, reference compiler or downloaded executable is used.

To run the steps separately, use the following commands. On Windows, the executable has an `.exe` suffix.

```sh
cargo build --release --locked --offline --jobs 1
target/release/topaz-seed --bootstrap . target/bootstrap-regenerated.rs
cmp generated/compiler.rs target/bootstrap-regenerated.rs
```

The small runner handles files and the compiler exchange. The compiler written in Topaz performs parsing, checking, lowering and Rust generation. The exchange response is also saved beside the regenerated Rust to help diagnose a failure.

The Cargo lockfile contains only the eight local workspace packages. No registry
crates, vendor download or network access are needed after the Rust toolchain is
installed. `SEED-PROVENANCE.json` identifies the release source, generated Rust
and the separately selected runtime revision, with the input and packaged hash
of each runtime file. The release compiler sources and generated Rust are
unchanged. Packaging omits separate test modules and updates documentation.
The runtime includes a sorting repair: an unordered NaN key returns an ordering
error instead of causing the underlying Rust sort to panic.
Date construction also rejects out-of-range day values before calendar arithmetic.
Array assignment and optional Array, Bytes and JSON array access reject indices
that exceed the target pointer width instead of truncating them on 32-bit targets.

Self-compilation is a substantial build. Available memory and swap affect both
completion and elapsed time; allow several hours and preserve the generated
output when diagnosing an interrupted run. The scripts print success only after
the regenerated Rust matches byte for byte.

## License

The compiler source, generated Rust, six runtime crates and seed runner are available under Apache-2.0. See `LICENSE`, `LICENSE-COMPONENTS.txt`, `NOTICE` and the included third-party notices.
