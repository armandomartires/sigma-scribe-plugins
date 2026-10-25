#!/usr/bin/env bash
# Builds a screen plugin into dist/<id>/ (plugin.conf + the wasm module).
# Usage: scripts/build-plugin.sh graph-map
set -euo pipefail
id="${1:?usage: build-plugin.sh <plugin-id>}"
cd "$(dirname "$0")/.."
crate="plugins/$id"
[ -f "$crate/plugin.conf" ] || { echo "no such plugin: $id" >&2; exit 1; }
entry="$(sed -n 's/^entry=//p' "$crate/plugin.conf")"
cargo build --release --target wasm32-unknown-unknown -p "$id" --locked
rm -rf "dist/$id" && mkdir -p "dist/$id"
cp "$crate/plugin.conf" "dist/$id/"
cp "target/wasm32-unknown-unknown/release/$(echo "$id" | tr - _).wasm" "dist/$id/$entry"
( cd "dist/$id" && sha256sum "$entry" )
ls -l "dist/$id"
