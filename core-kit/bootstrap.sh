#!/bin/sh
# Build the Topaz CLI from the public generated core; core self-compilation is opt-in.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$root/out/core-kit-bootstrap"
out_set=false
verify_core=false

usage() {
  printf '%s\n' 'usage: bootstrap [--out DIRECTORY | DIRECTORY] [--verify-core]'
  printf '%s\n' 'Requires the already-installed Rust/Cargo 1.96.0 toolchain and native linker.'
  printf '%s\n' 'Builds the generator, generates the Topaz CLI, and links the native Core Kit.'
  printf '%s\n' '--verify-core additionally performs expensive canonical core regeneration.'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --verify-core)
      [ "$verify_core" = false ] || { usage >&2; exit 2; }
      verify_core=true ;;
    --out)
      [ "$#" -ge 2 ] && [ "$out_set" = false ] && [ -n "$2" ] || { usage >&2; exit 2; }
      out=$2
      out_set=true
      shift ;;
    -*) usage >&2; exit 2 ;;
    *)
      [ "$out_set" = false ] && [ -n "$1" ] || { usage >&2; exit 2; }
      out=$1
      out_set=true ;;
  esac
  shift
done

# These variables affect this process only. Missing tools must fail, never download.
RUSTUP_TOOLCHAIN=1.96.0
RUSTUP_AUTO_INSTALL=0
CARGO_NET_OFFLINE=true
TOPAZ_CORE_KIT_ROOT="$root"
export RUSTUP_TOOLCHAIN RUSTUP_AUTO_INSTALL CARGO_NET_OFFLINE TOPAZ_CORE_KIT_ROOT

command -v cargo >/dev/null 2>&1 || { printf '%s\n' 'Cargo 1.96.0 is required.' >&2; exit 1; }
command -v rustc >/dev/null 2>&1 || { printf '%s\n' 'Rust 1.96.0 is required.' >&2; exit 1; }
rust_info=$(rustc -vV) || { printf '%s\n' 'Rust 1.96.0 must already be installed; automatic installation is disabled.' >&2; exit 1; }
release=$(printf '%s\n' "$rust_info" | sed -n 's/^release: //p')
host=$(printf '%s\n' "$rust_info" | sed -n 's/^host: //p')
[ "$release" = '1.96.0' ] || { printf 'expected Rust 1.96.0, found %s\n' "$release" >&2; exit 1; }
case "$host" in ''|*[!a-zA-Z0-9_-]*) printf '%s\n' 'Cannot determine Rust host target.' >&2; exit 1 ;; esac
cargo_version=$(cargo --version)
case "$cargo_version" in 'cargo 1.96.0 '*) ;; *) printf 'expected Cargo 1.96.0, found %s\n' "$cargo_version" >&2; exit 1 ;; esac

# Resolve the user's output relative to the caller, before entering the workspace.
mkdir -p -- "$out"
out=$(CDPATH= cd -- "$out" && pwd)
target="$root/compiler/target"
artifacts="$target/$host/release"
case "$host" in *-windows-*) suffix=.exe ;; *) suffix= ;; esac

cd "$root/compiler"
if [ "$verify_core" = true ]; then
  printf '%s\n' 'Bootstrap mode: product build plus optional canonical core regeneration.'
else
  printf '%s\n' 'Bootstrap mode: product build. Canonical core regeneration is disabled.'
fi
stage='public core and CLI generator build'
trap 'code=$?; if [ "$code" -ne 0 ]; then printf "Bootstrap failed during %s (exit %s).\n" "$stage" "$code" >&2; fi' 0
printf '%s\n' '[1/3] Building the public core and CLI generator (offline). The first build can take a while.'
cargo build -p topaz-core-kit-native --bin topaz-core-kit-generate-cli \
  --release --frozen --target "$host" --target-dir "$target"
stage='Topaz CLI generation'
printf '%s\n' '[2/3] Compiling the checked-in Topaz CLI source.'
"$artifacts/topaz-core-kit-generate-cli$suffix" "$out"

TOPAZ_CORE_KIT_GENERATED_CLI="$out/generated_cli.rs"
export TOPAZ_CORE_KIT_GENERATED_CLI
stage='native Core Kit build'
printf '%s\n' '[3/3] Linking the native Core Kit.'
cargo build -p topaz-core-kit-native --bin topaz-core-kit --features native-cli --message-format=short \
  --release --frozen --target "$host" --target-dir "$target"
stage='copying the built Core Kit'
cp "$artifacts/topaz-core-kit$suffix" "$out/topaz-core-kit$suffix"

if [ "$verify_core" = true ]; then
  printf '%s\n' 'Regenerating the canonical core; this optional step can take a long time.'
  stage='optional seed build'
  cargo build -p topaz_seed --release --frozen --target "$host" --target-dir "$target"
  stage='optional canonical core regeneration'
  "$artifacts/topaz-seed$suffix" --bootstrap "$root/compiler/seed/initial" "$out/compiler-regenerated.rs"
  stage='optional canonical core comparison'
  cmp "$root/compiler/generated/native/compiler.rs" "$out/compiler-regenerated.rs"
  printf '%s\n' 'Canonical core regeneration matches byte for byte.'
fi

printf 'Core Kit output: %s\n' "$out/topaz-core-kit$suffix"
printf '%s\n' 'Keep the binary inside this checkout, or set TOPAZ_CORE_KIT_ROOT to its data checkout.'
printf '%s\n' 'No release payload or reproducibility claim is created by this local build.'
