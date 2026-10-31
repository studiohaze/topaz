#!/bin/sh
# Explicit local execution gate. This never rebuilds or self-compiles Core Kit.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary="$root/out/core-kit-bootstrap/topaz-core-kit"
binary_set=false
native_build=false
for argument in "$@"; do
  case "$argument" in
    --native-build) native_build=true ;;
    -*) printf '%s\n' 'usage: smoke.sh [core-kit-executable] [--native-build]' >&2; exit 2 ;;
    *)
      [ "$binary_set" = false ] || { printf '%s\n' 'only one executable may be supplied' >&2; exit 2; }
      binary=$argument
      binary_set=true ;;
  esac
done
[ -x "$binary" ] || { printf 'missing Core Kit executable: %s\n' "$binary" >&2; exit 1; }
binary=$(CDPATH= cd -- "$(dirname -- "$binary")" && pwd)/$(basename -- "$binary")
mkdir -p "$root/out"
work=$(mktemp -d "$root/out/core-kit smoke.XXXXXX")
TOPAZ_CORE_KIT_ROOT="$root"
RUSTUP_TOOLCHAIN=1.96.0
RUSTUP_AUTO_INSTALL=0
CARGO_NET_OFFLINE=true
export TOPAZ_CORE_KIT_ROOT RUSTUP_TOOLCHAIN RUSTUP_AUTO_INSTALL CARGO_NET_OFFLINE
stage='entering the smoke workspace'
trap 'code=$?; if [ "$code" -ne 0 ]; then printf "Smoke failed during %s (exit %s).\n" "$stage" "$code" >&2; fi; printf "Smoke logs retained: %s\n" "$work"' 0
cd "$work"

stage='version and help'
printf 'Smoke: %s\n' "$stage"
"$binary" --version > version.txt 2>&1
case "$(cat version.txt)" in 'topaz-core-kit '*) ;; *) printf '%s\n' 'invalid version output' >&2; exit 1 ;; esac
"$binary" help > help.txt 2>&1
grep -F 'Topaz Core Kit' help.txt >/dev/null
stage='file hashing'
printf 'Smoke: %s\n' "$stage"
"$binary" hash "$root/core-kit/cli/data/input.txt" > hash.txt 2>&1
# Keep the complete output contract, including its single LF terminator.
cmp "$root/core-kit/fixtures/hash.stdout" hash.txt
stage='linked CLI identity'
printf 'Smoke: %s\n' "$stage"
"$binary" kit build-id > linked-cli.sha256
source_file=$(dirname -- "$binary")/generated_cli.rs
if [ -f "$source_file" ]; then
  "$binary" hash "$source_file" > generated-cli.sha256
  cmp generated-cli.sha256 linked-cli.sha256
fi
stage='platform executable inspection'
printf 'Smoke: %s\n' "$stage"
"$binary" kit binary-info --file "$binary" > binary-info.json 2> binary-info.stderr.txt
[ -s binary-info.json ]
stage='check, request and Rust output'
printf 'Smoke: %s\n' "$stage"
"$binary" check --source "$root/examples/core-kit/hello.tpz" > check.txt 2>&1
"$binary" request --source "$root/examples/core-kit/hello.tpz" --out hello.request.json > request.txt 2>&1
"$binary" check --request hello.request.json > check-request.txt 2>&1
"$binary" emit-rust --source "$root/examples/core-kit/hello.tpz" --out hello.rs > emit.txt 2>&1
[ -s hello.rs ]

stage='project resolution and saved input replay'
printf 'Smoke: %s\n' "$stage"
"$binary" check --root "$root/examples/core-kit/project" > project-check.txt 2>&1
"$binary" request --root "$root/examples/core-kit/project" --out 'project inputs/snapshot.json' > project-request.txt 2>&1
"$binary" check --request 'project inputs/snapshot.json' > project-replay.txt 2>&1
"$binary" emit-rust --root "$root/examples/core-kit/project" --out 'project output/generated.rs' > project-emit.txt 2>&1
[ -s 'project output/generated.rs' ]

stage='negative type-check and output preservation'
printf 'Smoke: %s\n' "$stage"
if "$binary" check --source "$root/examples/core-kit/type-error.tpz" > rejected-check.txt 2>&1; then
  printf '%s\n' 'type-error fixture was incorrectly accepted' >&2; exit 1
else
  status=$?
  [ "$status" -eq 1 ] || { printf 'type-error check returned unexpected exit %s\n' "$status" >&2; exit "$status"; }
fi
grep -F 'TPZ5001:' rejected-check.txt >/dev/null
if "$binary" emit-rust --source "$root/examples/core-kit/type-error.tpz" --out rejected.rs > rejected-emit.txt 2>&1; then
  printf '%s\n' 'type-error fixture incorrectly emitted Rust' >&2; exit 1
else
  status=$?
  [ "$status" -eq 1 ] || { printf 'type-error emission returned unexpected exit %s\n' "$status" >&2; exit "$status"; }
fi
grep -F 'TPZ5001:' rejected-emit.txt >/dev/null
[ ! -e rejected.rs ]
if "$binary" check --root "$root/examples/core-kit" > rejected-root.txt 2>&1; then
  printf '%s\n' 'project without topaz.toml was silently accepted' >&2; exit 1
else
  status=$?
  [ "$status" -eq 1 ] || { printf 'missing project manifest returned unexpected exit %s\n' "$status" >&2; exit "$status"; }
fi
grep -F 'project topaz.toml must be a readable file inside the project root' rejected-root.txt >/dev/null
stage='recorded input verification'
printf 'Smoke: %s\n' "$stage"
"$binary" kit verify > verify.txt 2>&1

stage='CLI snapshot and callable contract check'
"$binary" kit refresh --check > refresh-check.txt 2>&1

stage='payload inventory and extra-file rejection'
printf 'Smoke: %s\n' "$stage"
case "$(uname -s):$(uname -m)" in
  Linux:x86_64) payload_target=x86_64-unknown-linux-gnu ;;
  Linux:aarch64|Linux:arm64) payload_target=aarch64-unknown-linux-gnu ;;
  Darwin:x86_64) payload_target=x86_64-apple-darwin ;;
  Darwin:arm64) payload_target=aarch64-apple-darwin ;;
  *) printf '%s\n' 'this smoke host has no repository payload target mapping' >&2; exit 1 ;;
esac
mkdir 'payload files'
cp "$binary" 'payload files/topaz-core-kit'
"$binary" kit payload-record --target "$payload_target" --payload-dir 'payload files' > payload-record.txt 2>&1
"$binary" kit payload-verify --payload-dir 'payload files' > payload-verify.txt 2>&1
printf '%s\n' 'unrecorded smoke fixture' > 'payload files/extra.txt'
if "$binary" kit payload-verify --payload-dir 'payload files' > rejected-payload.txt 2>&1; then
  printf '%s\n' 'an extra payload file was incorrectly accepted' >&2; exit 1
else
  status=$?
  [ "$status" -eq 1 ] || { printf 'payload rejection returned unexpected exit %s\n' "$status" >&2; exit "$status"; }
fi
grep -F 'payload file count differs from payload.json' rejected-payload.txt >/dev/null
rm 'payload files/extra.txt'

if [ "$native_build" = true ]; then
  stage='native sample build and execution'
  printf 'Smoke: %s\n' "$stage"
  "$binary" build --source "$root/examples/core-kit/hello.tpz" --out-dir 'native output' > native-build.txt 2>&1
  rust_info=$(rustc -vV)
  host=$(printf '%s\n' "$rust_info" | sed -n 's/^host: //p')
  case "$host" in ''|*[!a-zA-Z0-9_-]*) printf '%s\n' 'cannot identify native program target' >&2; exit 1 ;; esac
  case "$host" in *-windows-*) suffix=.exe ;; *) suffix= ;; esac
  program="$work/native output/target/$host/release/topaz-core-kit-program$suffix"
  "$program" < /dev/null > program.txt 2>&1
  cmp "$root/core-kit/fixtures/hello.stdout" program.txt

  stage='native project build and execution'
  printf 'Smoke: %s\n' "$stage"
  "$binary" build --request 'project inputs/snapshot.json' --out-dir 'project native' > project-native-build.txt 2>&1
  project_program="$work/project native/target/$host/release/topaz-core-kit-program$suffix"
  "$project_program" < /dev/null > project-program.txt 2>&1
  cmp "$root/core-kit/fixtures/project.stdout" project-program.txt
fi

printf '%s\n' 'Core Kit smoke checks passed.'
