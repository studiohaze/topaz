#!/bin/sh
set -eu
compiler=$1
shift
if [ -n "${OUT_DIR:-}" ]; then
  set -- "$@" "--remap-path-prefix=$OUT_DIR=/topaz/out"
fi
exec "$compiler" "$@"
