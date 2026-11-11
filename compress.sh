#!/usr/bin/env bash
set -euo pipefail

# Usage: ./compress.sh <source> <destination>
#
#   source      a directory, walked recursively for jpgs, or a file holding a
#               newline-delimited list of paths (see files.txt)
#   destination where the compressed jpgs land, flat
SRC="${1:?usage: ./compress.sh <source-dir-or-file-list> <destination-dir>}"
OUTPUT_DIR="${2:?usage: ./compress.sh <source-dir-or-file-list> <destination-dir>}"

if [ ! -d "$SRC" ] && [ ! -f "$SRC" ]; then
  echo "error: source is neither a directory nor a file: $SRC" >&2
  exit 1
fi

mkdir -p "$OUTPUT_DIR"

export OUTPUT_DIR

if [ -d "$SRC" ]; then
  find "$SRC" -type f -iname '*.jpg' -print0
else
  grep -vE '^[[:space:]]*$' "$SRC" | tr '\n' '\0'
fi | \
parallel -0 --bar --halt soon,fail=1 '
  in="{}"

  # Skip if input file no longer exists
  [ -f "$in" ] || exit 0

  base="$(basename "$in")"
  name="${base%.*}"

  out="$OUTPUT_DIR/${name}.jpg"

  # Skip if already processed
  [ -f "$out" ] && exit 0

  ffmpeg -y -loglevel error \
    -i "$in" \
    -vf scale=2048:-1:flags=lanczos \
    -q:v 2 \
    "$out"
'
