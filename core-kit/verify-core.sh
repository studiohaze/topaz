#!/bin/sh
set -eu
LC_ALL=C
export LC_ALL

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
data="$root/core-kit/verify-core.data"

fail() { printf '%s\n' "$*" >&2; exit 1; }
[ -r "$data" ] && [ -s "$data" ] || fail 'cannot read non-empty verifier inventory'

# This is a strict, deliberately small data format, not a JSON parser. The
# inventory pins the complete JSON manifests and request, as well as the bytes
# they describe. Changing those inputs requires reviewing the inventory too.
awk -F '|' '
  function bad(message) {
    print "invalid verifier inventory at line " NR ": " message > "/dev/stderr"
    failed=1
  }
  function digest(s) { return length(s) == 64 && s ~ /^[0-9a-f]+$/ }
  function path(s) {
    return s ~ /^[A-Za-z0-9_.\/-]+$/ && s !~ /^\// && s !~ /\/$/ &&
           s !~ /\/\// && s !~ /(^|\/)\.\.?($|\/)/
  }
  /^#/ || /^$/ { next }
  $1 == "format" || $1 == "compiler_source_set" ||
  $1 == "compiler_sources" || $1 == "snapshot_files" || $1 == "input_files" {
    if (NF != 2 || seen[$1]++) { bad("invalid or duplicate metadata"); next }
    metadata[$1]=$2
    next
  }
  $1 == "compiler_source" || $1 == "snapshot_file" || $1 == "input_file" {
    if (NF != 3 || !path($2) || !digest($3)) { bad("invalid file record"); next }
    if ($1 != "compiler_source" && tolower($2) == "core-kit/downloads.data") {
      bad("download catalogue must remain outside the recorded build inputs"); next
    }
    key=$1 SUBSEP $2
    if (seen[key]++) bad("duplicate file record")
    count[$1]++
    if ($1 == "input_file") inputs[$2]=1
    next
  }
  { bad("unknown record") }
  END {
    if (metadata["format"] != "topaz.core-kit-inputs/v2") bad("unsupported format")
    id=metadata["compiler_source_set"]
    if (substr(id, 1, 7) != "sha256:" || !digest(substr(id, 8))) bad("invalid source set")
    split("compiler_sources snapshot_files input_files", keys, " ")
    split("compiler_source snapshot_file input_file", kinds, " ")
    for (i=1; i<=3; i++) {
      n=metadata[keys[i]]
      if (n !~ /^[1-9][0-9]*$/ || n != count[kinds[i]]) bad("file count mismatch")
    }
    required["SOURCE-COMPONENTS.json"]=1
    required["compiler/generated/native/compiler.json"]=1
    required["compiler/generated/native/compiler.rs"]=1
    required["compiler/seed/initial/request.json"]=1
    required["compiler/seed/initial/REQUEST-PROVENANCE.json"]=1
    required["compiler/seed/initial/std/bytes.tpz"]=1
    required["compiler/seed/initial/std/json.tpz"]=1
    required["compiler/seed/initial/std/fs.tpz"]=1
    required["compiler/seed/initial/std/io.tpz"]=1
    required["core-kit/cli/REQUEST.json"]=1
    required["core-kit/cli/initial/request.json"]=1
    for (file in required) if (!(file in inputs)) bad("required input missing: " file)
    if (failed) exit 1
  }
' "$data" || fail 'verifier inventory validation failed'

if command -v sha256sum >/dev/null 2>&1; then
  hash_kind=sha256sum
elif command -v shasum >/dev/null 2>&1; then
  hash_kind=shasum
else
  fail 'SHA-256 verification needs sha256sum (Linux) or shasum (macOS)'
fi

check_hash() {
  file=$1
  expected=$2
  [ -f "$file" ] && [ -r "$file" ] || fail "cannot read file: $file"
  # Hash stdin so spaces and backslashes in the checkout path cannot alter
  # the checksum output format. Capture the tool status before parsing it.
  if [ "$hash_kind" = sha256sum ]; then
    result=$(sha256sum < "$file") || fail "cannot hash file: $file"
  else
    result=$(shasum -a 256 < "$file") || fail "cannot hash file: $file"
  fi
  actual=${result%% *}
  [ "$actual" = "$expected" ] || fail "hash mismatch: $file"
}

compiler_count=0
snapshot_count=0
input_count=0
while IFS='|' read -r kind path expected || [ -n "$kind$path$expected" ]; do
  case "$kind" in
    compiler_source)
      check_hash "$root/compiler/crates/topaz_self_frontend/topaz/$path" "$expected"
      check_hash "$root/compiler/seed/initial/topaz/$path" "$expected"
      compiler_count=$((compiler_count + 1))
      ;;
    snapshot_file)
      check_hash "$root/$path" "$expected"
      snapshot_count=$((snapshot_count + 1))
      ;;
    input_file)
      check_hash "$root/$path" "$expected"
      input_count=$((input_count + 1))
      ;;
  esac
done < "$data"

printf 'Core Kit input hashes match: %s compiler source pairs, %s snapshot files, %s input files\n' \
  "$compiler_count" "$snapshot_count" "$input_count"
printf '%s\n' 'This checks the recorded input bytes; it does not run the compiler or prove reproducibility.'
