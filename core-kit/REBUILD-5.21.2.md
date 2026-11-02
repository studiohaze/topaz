# Rebuild the 5.21.2 compiler from the 5.21.1 seed

This recipe starts with the public 5.21.1 compiler seed and the public 5.21.2
Topaz source. It builds the old compiler, compiles the new source, then builds
that output and compiles the same source again. A final build and emission
check that the generated Rust has reached a fixed point.

Use Linux x86-64, Rust/Cargo 1.96.0, a C linker, GNU time, curl, Git and tar.
Use a fresh directory and keep the old compiler with the runtime from its own
seed. The later generations use the 5.21.2 public runtime. Cargo reads local
workspace packages with network access disabled.

## Inputs and expected output

| File | SHA-256 |
| --- | --- |
| `topaz-bootstrap-seed-v5.21.1.tar.gz` | `cf6a09b66fa5197472808fc01388e799b394686fb705b8e977cc39962583e974` |
| 5.21.2 `compiler/generated/native/compiler.rs` | `53afe18d431ace25e2896dc4d1359f2976707ae9d7f1f04f85876874480ee703` |

The public source commit is
`6e8cb763f4c1be67e2ff79a1bf628182ceeab3db`. The expected generated compiler
has 13,118,454 bytes. The source is under
`compiler/seed/initial/topaz/`, its standard declarations are under
`compiler/seed/initial/std/`, and `request.json` supplies the bootstrap request.

## Build and compare

```sh
set -eu
mkdir topaz-5.21.2-rebuild
cd topaz-5.21.2-rebuild
recipe_root=$(pwd -P)

curl --fail --location --proto '=https' --proto-redir '=https' \
  -o topaz-bootstrap-seed-v5.21.1.tar.gz \
  https://topaz.ooo/dl/v5.21.1/topaz-bootstrap-seed-v5.21.1.tar.gz
printf '%s  %s\n' \
  cf6a09b66fa5197472808fc01388e799b394686fb705b8e977cc39962583e974 \
  topaz-bootstrap-seed-v5.21.1.tar.gz | sha256sum --check -
mkdir old
tar -xzf topaz-bootstrap-seed-v5.21.1.tar.gz -C old
(cd old/topaz-public && sha256sum --check SHA256SUMS)

git clone --no-checkout https://github.com/studiohaze/topaz.git source
git -C source checkout --detach 6e8cb763f4c1be67e2ff79a1bf628182ceeab3db

export RUSTUP_TOOLCHAIN=1.96.0
export RUSTUP_AUTO_INSTALL=0
export CARGO_BUILD_JOBS=1
export CARGO_NET_OFFLINE=true
export MALLOC_ARENA_MAX=2
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS

# C0: build the previous public seed in its own workspace.
(cd old/topaz-public && CARGO_HOME="$PWD/.cargo-home" \
  /usr/bin/time -v -o "$recipe_root/C0-build.time.txt" \
  cargo build --release --locked --offline --jobs 1)

# G1: compile the 5.21.2 Topaz source with that compiler.
/usr/bin/time -v -o G1.time.txt \
  old/topaz-public/target/release/topaz-seed \
  --bootstrap source/compiler/seed/initial G1.rs

# C1: build G1 against the 5.21.2 public runtime.
cp -a source transition
cp G1.rs transition/compiler/generated/native/compiler.rs
(cd transition/compiler && CARGO_HOME="$PWD/.cargo-home" \
  /usr/bin/time -v -o "$recipe_root/C1-build.time.txt" \
  cargo build -p topaz_seed --bin topaz-seed --release --locked --offline \
  --jobs 1 --config 'profile.release.package.topaz_seed_compiler.opt-level=1')

# G2: compile the same source with C1.
/usr/bin/time -v -o G2.time.txt \
  transition/compiler/target/release/topaz-seed \
  --bootstrap source/compiler/seed/initial G2.rs
cmp G2.rs source/compiler/generated/native/compiler.rs

# C2/G3: independently build the published compiler and regenerate it.
(cd source/compiler && CARGO_HOME="$PWD/.cargo-home" \
  /usr/bin/time -v -o "$recipe_root/C2-build.time.txt" \
  cargo build -p topaz_seed --bin topaz-seed --release --locked --offline \
  --jobs 1 --config 'profile.release.package.topaz_seed_compiler.opt-level=1')
/usr/bin/time -v -o G3.time.txt \
  source/compiler/target/release/topaz-seed \
  --bootstrap source/compiler/seed/initial G3.rs
cmp G2.rs G3.rs
wc -c G1.rs G2.rs G3.rs
sha256sum G1.rs G2.rs G3.rs
```

The required final comparison is `G2.rs == G3.rs`, with both equal to the
published `compiler.rs`. Emission also retains a `.response.json` file for
each generation. Keep the time files and responses with the generated Rust.
An unequal comparison or unsuccessful command stops the recipe.

## Recorded memory and time

Release preparation used Rust/Cargo 1.96.0 with one Cargo job. The following
sequential measurements record the tools used to produce and qualify 5.21.2.
The first two compilers in that preparation were full CLIs. The last compiler
was the standalone public seed.

| Process | Recorded elapsed seconds | Largest-process peak RSS bytes |
| --- | ---: | ---: |
| 5.21.1 full CLI emits G1 | 5,858.676 | 4,102,287,360 |
| Build the transitional full CLI | 1,449.331 | 6,526,291,968 |
| Transitional full CLI emits G2 | 715.865 | 4,126,371,840 |
| Build the 5.21.2 standalone seed | 1,095.540 | 6,380,748,800 |
| 5.21.2 standalone seed emits G3 | 778.090 | 3,974,152,192 |

That preparation used a 16 GB host, a 12 GiB process-group memory ceiling and
4 GiB swap allowance. Each heavy operation started with at least 3.5 GiB free
host memory. The standalone seed build and G3 emission took 1,873.630 seconds
together according to GNU time. These are observations for that environment.
The first three rows use their process recorder's elapsed clock. The public
reader commands above have a separate execution record below.

## Public-input execution

A fresh Linux x86-64 run downloaded the 5.21.1 seed from the versioned public
download and checked out the 5.21.2 public source commit listed above. It built
C0 with Cargo offline, then ran C0 to produce G1. The generated Rust matched
the published compiler byte for byte.

The run used Rust/Cargo 1.96.0 and one Cargo job, with a container memory limit
of 8 GiB and 4 GiB additional swap on a 16 GB host. GNU time recorded these
process measurements.

| Process | Elapsed time | Largest-process peak RSS bytes |
| --- | --- | --- |
| Build the public 5.21.1 seed | 18:56.17 | 8,569,565,184 |
| Emit the 5.21.2 compiler with C0 | 1:54:52 | 3,987,042,304 |

G1 contains 13,118,454 bytes with SHA-256
`53afe18d431ace25e2896dc4d1359f2976707ae9d7f1f04f85876874480ee703`. The C1, G2, C2 and G3 commands above continue
the public recipe from that output. The complete release-preparation chain
has its separate measurements in the preceding table.
