#!/usr/bin/env bash
# Packages every plugin and writes the repository index.
# Usage: scripts/make-index.sh [out-dir]      (default: site/)
#
# A plugin's files come from dist/<id>/ (built screen and pane plugins; see
# build-plugin.sh) or, for data packs, plugins/<id>/. The package is an
# uncompressed ustar archive of those files, named <id>-<version>.tar. The
# index lists each with its size and sha256; `serial` is the time, so it only
# ever rises. index.html, the landing page, is generated from the index by
# make-landing.sh. Publish the out-dir at the URL in sigma-core's INDEX_URL.
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:-site}"
base="https://armandomartires.github.io/sigma-scribe-plugins"
rm -rf "$out" && mkdir -p "$out"
index="$out/index.json"
printf 'serial=%s\n' "$(date +%s)" > "$index"
for conf in plugins/*/plugin.conf; do
  id="$(basename "$(dirname "$conf")")"
  [ "$id" = sdk ] && continue
  src="plugins/$id"; [ -d "dist/$id" ] && src="dist/$id"
  [ -f "$src/plugin.conf" ] || { echo "no plugin.conf in $src" >&2; exit 1; }
  version="$(sed -n 's/^version=//p' "$src/plugin.conf" | head -1)"
  [ -n "$version" ] || { echo "$id: no version" >&2; exit 1; }
  # Screen and pane plugins must have been built.
  entry="$(sed -n 's/^entry=//p' "$src/plugin.conf" | head -1)"
  [ -z "$entry" ] || [ -f "$src/$entry" ] || { echo "$id: $entry not built (scripts/build-plugin.sh $id)" >&2; exit 1; }
  pkg="$id-$version.tar"
  files="$(cd "$src" && find . -type f ! -name 'Cargo.*' ! -path './src/*' ! -path './target/*' | sed 's|^\./||' | LC_ALL=C sort)"
  # shellcheck disable=SC2086
  tar --format=ustar --owner=0 --group=0 --numeric-owner --mtime=@0 -cf "$out/$pkg" -C "$src" $files
  {
    printf '\n'
    grep -v '^[[:space:]]*\(#.*\)\?$' "$src/plugin.conf"
    printf 'url=%s/%s\nsize=%s\nsha256=%s\n' "$base" "$pkg" \
      "$(stat -c %s "$out/$pkg")" "$(sha256sum "$out/$pkg" | cut -d' ' -f1)"
  } >> "$index"
done
scripts/make-landing.sh "$out"
echo "wrote $index"; grep -c '^id=' "$index" | sed 's/$/ plugins/'
