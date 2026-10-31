#!/bin/sh
# Normalize source locations for a separately verified POSIX payload build.
# This is the ordinary bootstrap with fixed Rust path mappings, not a claim
# that this host/toolchain or its complete output has been reproduced.
set -eu

usage() {
  printf '%s\n' 'usage: core-kit/repro-build.sh [OUTPUT_DIRECTORY]'
  printf '%s\n' 'Builds Core Kit with stable Rust source paths; does not regenerate the core.'
  printf '%s\n' 'For a reproduction check, run in two clean checkouts and compare their actual executables.'
}

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
  -*) usage >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { usage >&2; exit 2; }

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
out=${1:-"$root/out/core-kit-repro"}

# An inherited flags string could override the mapping or change the recipe.
# Fail explicitly rather than silently discarding a caller's compiler flags.
if [ -n "${RUSTFLAGS:-}" ] || [ -n "${CARGO_ENCODED_RUSTFLAGS:-}" ]; then
  printf '%s\n' 'Unset RUSTFLAGS and CARGO_ENCODED_RUSTFLAGS before using this fixed path-mapping recipe.' >&2
  exit 1
fi

mkdir -p -- "$out"
out=$(CDPATH= cd -- "$out" && pwd -P)
if [ -L "$out/generated_cli.rs" ]; then
  printf '%s\n' 'The generated CLI output must be a regular file, not a symbolic link.' >&2
  exit 1
fi

# Cargo splits this variable only at U+001F. Spaces, Unicode and '=' in source
# paths remain within a single rustc argument. The exact generated-file rule
# comes last so it also normalizes a custom output outside the checkout.
separator=$(printf '\037')
case "$root$out" in
  *"$separator"*) printf '%s\n' 'A source/output path contains the Cargo argument separator.' >&2; exit 1 ;;
esac
CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$root=/topaz-core-kit${separator}--remap-path-prefix=$out/generated_cli.rs=/topaz-core-kit/generated/cli.rs"
export CARGO_ENCODED_RUSTFLAGS

printf '%s\n' 'Source path recipe: checkout -> /topaz-core-kit; generated CLI -> /topaz-core-kit/generated/cli.rs.'
printf '%s\n' 'Changing compiler flags can rebuild the core dependency even when canonical self-regeneration is disabled.'
exec "$root/bootstrap" --out "$out"
