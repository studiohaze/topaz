# Shared mechanical download selection for the POSIX installer and launcher.
# This checked-in program is sourced; downloads.data is parsed only as data.

_kit_fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

_kit_host_target() {
  _kit_system=$(uname -s)
  _kit_machine=$(uname -m)
  case "$_kit_system:$_kit_machine" in
    Linux:x86_64) _kit_target=x86_64-unknown-linux-gnu ;;
    Linux:aarch64|Linux:arm64) _kit_target=aarch64-unknown-linux-gnu ;;
    Darwin:x86_64) _kit_target=x86_64-apple-darwin ;;
    Darwin:arm64|Darwin:aarch64) _kit_target=aarch64-apple-darwin ;;
    *) _kit_fail "No Core Kit download target for $_kit_system/$_kit_machine. See core-kit/BUILD.md for source builds." ;;
  esac
  _kit_executable=topaz-core-kit
}

_kit_load_download() {
  _kit_catalogue="$_kit_root/core-kit/downloads.data"
  [ -d "$_kit_root/core-kit" ] && [ ! -L "$_kit_root/core-kit" ] ||
    _kit_fail 'core-kit must be a real checkout directory.'
  [ -f "$_kit_catalogue" ] && [ ! -L "$_kit_catalogue" ] ||
    _kit_fail 'core-kit/downloads.data must be a regular checkout file.'
  _kit_catalogue_bytes=$(wc -c < "$_kit_catalogue")
  [ "$_kit_catalogue_bytes" -gt 0 ] && [ "$_kit_catalogue_bytes" -le 65536 ] ||
    _kit_fail 'The download catalogue must contain between 1 and 65536 bytes.'
  # Reject controls, NUL, CR, non-ASCII bytes and a missing final LF before
  # line parsing. The size is checked again in case the file changed.
  LC_ALL=C od -An -v -t u1 "$_kit_catalogue" | LC_ALL=C awk '
    { for (i=1; i<=NF; i++) {
        n++; last=$i
        if (($i != 10 && ($i < 32 || $i > 126)) || n > 65536) bad=1
      }
    }
    END { if (bad || n == 0 || last != 10) exit 1 }
  ' || _kit_fail 'The download catalogue must use plain ASCII with LF line endings and a final LF.'
  _kit_row=$(LC_ALL=C awk -F '|' -v wanted="$_kit_target" '
    function fail(message) { print message; bad=1; exit 1 }
    function token(value) {
      return length(value) >= 1 && length(value) <= 128 &&
        value ~ /^[A-Za-z0-9][A-Za-z0-9._-]*$/ && value !~ /[.]$/
    }
    function hex(value, size) { return length(value) == size && value ~ /^[0-9a-f]+$/ }
    function number(value, maximum) {
      return length(value) <= 9 && value ~ /^[1-9][0-9]*$/ && value + 0 <= maximum
    }
    NR == 1 {
      if ($0 != "topaz.core-kit-downloads/v1") fail("Unsupported Core Kit download catalogue.")
      next
    }
    {
      if (NF != 10 || NR > 7) fail("A catalogue row must have ten fields and there may be at most six targets.")
      target=$1
      if (target != "aarch64-apple-darwin" && target != "aarch64-pc-windows-msvc" &&
          target != "aarch64-unknown-linux-gnu" && target != "x86_64-apple-darwin" &&
          target != "x86_64-pc-windows-msvc" && target != "x86_64-unknown-linux-gnu")
        fail("Unsupported target in the Core Kit download catalogue.")
      if (previous != "" && target <= previous) fail("Catalogue targets must be sorted and unique.")
      previous=target
      if (split($2, repository, "/") != 2 || !token(repository[1]) || !token(repository[2]))
        fail("Invalid GitHub repository in the Core Kit download catalogue.")
      if (!token($3) || tolower($3) == "latest") fail("A catalogue release tag must be a fixed portable name.")
      if (!hex($4,40) || !hex($5,64) || !hex($8,64) || !hex($10,64))
        fail("A catalogue source identity or SHA-256 is invalid.")
      if ($6 != "topaz-core-kit-" target ".tar.gz") fail("The release asset name does not match its target.")
      if (!number($7,536870912) || !number($9,268435456)) fail("A catalogue byte count is invalid or exceeds its limit.")
      if (target == wanted) selected=$0
    }
    END { if (bad) exit 1; if (NR == 0) exit 1; if (selected != "") print selected }
  ' "$_kit_catalogue") || _kit_fail "$_kit_row"
  [ -n "$_kit_row" ] ||
    _kit_fail "Missing platform row: this checkout has no Core Kit archive for $_kit_target. Build from source with ./bootstrap."
  IFS='|' read -r _kit_selected_target _kit_repository _kit_tag _kit_source_commit _kit_inputs_sha _kit_asset _kit_asset_bytes _kit_asset_sha _kit_exe_bytes _kit_exe_sha <<EOF
$_kit_row
EOF
  [ "$_kit_selected_target" = "$_kit_target" ] || _kit_fail 'The selected catalogue target changed.'
}

_kit_choose_hash_tool() {
  if command -v sha256sum >/dev/null 2>&1; then _kit_hash_tool=sha256sum
  elif command -v shasum >/dev/null 2>&1; then _kit_hash_tool=shasum
  else _kit_fail 'Install sha256sum or shasum to verify the selected download.'
  fi
}

_kit_check_file() {
  _kit_check_path=$1
  _kit_check_bytes=$2
  _kit_check_sha=$3
  _kit_check_kind=${4:-Download}
  [ -f "$_kit_check_path" ] && [ ! -L "$_kit_check_path" ] ||
    _kit_fail 'The selected download must be a regular file without a symlink.'
  _kit_actual_bytes=$(wc -c < "$_kit_check_path")
  [ "$_kit_actual_bytes" -eq "$_kit_check_bytes" ] ||
    _kit_fail "$_kit_check_kind size mismatch: use the archive selected by this catalogue, or install from a later commit containing the matching catalogue rows."
  if [ "$_kit_hash_tool" = sha256sum ]; then
    _kit_hash_line=$(sha256sum < "$_kit_check_path")
  else
    _kit_hash_line=$(shasum -a 256 < "$_kit_check_path")
  fi
  _kit_actual_sha=${_kit_hash_line%% *}
  [ "$_kit_actual_sha" = "$_kit_check_sha" ] ||
    _kit_fail "$_kit_check_kind hash mismatch: use the archive selected by this catalogue, or install from a later commit containing the matching catalogue rows."
}

_kit_damaged_installation() {
  _kit_fail "$1
To reinstall, move this version directory aside, then run ./install-core-kit:
$_kit_version"
}

_kit_installed_path() {
  _kit_version="$_kit_root/out/core-kit-installed/$_kit_target/$_kit_asset_sha"
  for _kit_directory in "$_kit_root/out" "$_kit_root/out/core-kit-installed" \
    "$_kit_root/out/core-kit-installed/$_kit_target" "$_kit_version"; do
    [ ! -L "$_kit_directory" ] || _kit_fail "Unexpected symlink in the installation cache: $_kit_directory"
    [ -e "$_kit_directory" ] || _kit_fail 'This Core Kit version is not installed. Run ./install-core-kit.'
    [ -d "$_kit_directory" ] && [ -x "$_kit_directory" ] ||
      _kit_fail "Expected an accessible installation cache directory: $_kit_directory"
  done
  _kit_receipt="$_kit_version/installation.data"
  [ -f "$_kit_receipt" ] && [ ! -L "$_kit_receipt" ] ||
    _kit_damaged_installation 'The local Core Kit installation receipt is missing or invalid.'
  printf 'topaz.core-kit-installation/v1|%s|%s|%s\n' "$_kit_target" "$_kit_asset_sha" "$_kit_inputs_sha" |
    cmp -s - "$_kit_receipt" || {
      [ "$?" -eq 1 ] || _kit_fail "Cannot compare the local installation receipt: $_kit_receipt"
      _kit_damaged_installation 'The local Core Kit installation receipt does not match this checkout catalogue.'
    }
  _kit_directory="$_kit_version/payload"
  [ -d "$_kit_directory" ] && [ ! -L "$_kit_directory" ] ||
    _kit_damaged_installation 'The matching Core Kit payload directory is missing or invalid.'
  [ -x "$_kit_directory" ] || _kit_fail "Cannot access the local payload directory: $_kit_directory"
  _kit_payload="$_kit_directory/$_kit_executable"
  [ -f "$_kit_payload" ] && [ ! -L "$_kit_payload" ] && [ -x "$_kit_payload" ] ||
    _kit_damaged_installation 'The matching Core Kit executable is missing or not executable.'
}
