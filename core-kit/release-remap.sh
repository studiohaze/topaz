# Set TOPAZ_REMAP_SOURCE, TOPAZ_REMAP_TARGET and optional
# TOPAZ_REMAP_GENERATED before sourcing. POSIX dot does not portably pass args.
: "${TOPAZ_REMAP_SOURCE:?source root is required}"
: "${TOPAZ_REMAP_TARGET:?target root is required}"
topaz_remap_separator=$(printf '\037')
topaz_remap_flags="${CARGO_ENCODED_RUSTFLAGS:-}"
topaz_remap_add() {
  [ -n "$1" ] || return 0
  if [ -n "$topaz_remap_flags" ]; then topaz_remap_flags="$topaz_remap_flags$topaz_remap_separator"; fi
  topaz_remap_flags="$topaz_remap_flags--remap-path-prefix=$1=$2"
}
topaz_remap_add "$TOPAZ_REMAP_SOURCE" /topaz/source
topaz_remap_add "$TOPAZ_REMAP_TARGET" /topaz/target
topaz_remap_add "${CARGO_HOME:-$HOME/.cargo}" /cargo
topaz_remap_add "${OUT_DIR:-}" /topaz/out
topaz_remap_add "${TOPAZ_REMAP_GENERATED:-}" /topaz/generated
CARGO_ENCODED_RUSTFLAGS=$topaz_remap_flags
export CARGO_ENCODED_RUSTFLAGS
if [ -n "${TOPAZ_REMAP_WRAPPER_DIRECTORY:-}" ]; then
  topaz_wrapper_dir="$TOPAZ_REMAP_TARGET/topaz-build-tools"
  mkdir -p "$topaz_wrapper_dir"
  cp "$TOPAZ_REMAP_WRAPPER_DIRECTORY/release-rustc-wrapper.sh" "$topaz_wrapper_dir/rustc-wrapper"
  chmod 700 "$topaz_wrapper_dir/rustc-wrapper"
  RUSTC_WRAPPER="$topaz_wrapper_dir/rustc-wrapper"
  export RUSTC_WRAPPER
fi
