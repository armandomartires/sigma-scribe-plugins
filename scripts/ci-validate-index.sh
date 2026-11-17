#!/bin/sh
# Checks a generated plugin index (from scripts/make-index.sh) without
# publishing it. Used by GitLab CI (P1-02).
#
# Usage: scripts/ci-validate-index.sh [out-dir]   (default: site)
set -eu
dir="${1:-site}"
index="$dir/index.json"

fail() { echo "ci-validate-index: $*" >&2; exit 1; }

[ -d "$dir" ] || fail "no such directory: $dir"
[ -f "$index" ] || fail "missing $index"
[ -s "$index" ] || fail "empty $index"
[ -f "$dir/index.html" ] || fail "missing landing page $dir/index.html"

serial=$(sed -n 's/^serial=//p' "$index" | head -1)
case "$serial" in
  ''|*[!0-9]*) fail "serial is missing or not an integer: ${serial:-<empty>}" ;;
esac

got_nl=$(awk -F= '/^id=/{print $2}' "$index" | LC_ALL=C sort)
want_nl=$(printf '%s\n' contents-pane graph-map scratch-pane todo-pane)
[ "$got_nl" = "$want_nl" ] || fail "plugin ids mismatch
expected:
$want_nl
got:
$got_nl"

# Walk the index in one awk pass. Fields: id|version|url|size|sha256|entry
records=$(mktemp)
trap 'rm -f "$records"' EXIT
awk '
  function emit() {
    if (id == "") return
    printf "%s|%s|%s|%s|%s|%s\n", id, version, url, size, sha256, entry
  }
  /^id=/     { emit(); id=substr($0,4); version=""; url=""; size=""; sha256=""; entry=""; next }
  /^version=/{ version=substr($0,9); next }
  /^url=/    { url=substr($0,5); next }
  /^size=/   { size=substr($0,6); next }
  /^sha256=/ { sha256=substr($0,8); next }
  /^entry=/  { entry=substr($0,7); next }
  END { emit() }
' "$index" > "$records"

while IFS='|' read -r id version url size sha256 entry; do
  [ -n "$id" ] || fail "empty id in index"
  [ -n "$version" ] || fail "$id: missing version"
  [ -n "$url" ] || fail "$id: missing url"
  [ -n "$size" ] || fail "$id: missing size"
  [ -n "$sha256" ] || fail "$id: missing sha256"
  pkg=$(basename "$url")
  want="$id-$version.tar"
  [ "$pkg" = "$want" ] || fail "$id: url basename $pkg != $want"
  path="$dir/$pkg"
  [ -f "$path" ] || fail "$id: missing package $path"
  got_size=$(wc -c < "$path")
  got_size=$(echo "$got_size" | tr -d ' ')
  [ "$got_size" = "$size" ] || fail "$id: size $got_size != declared $size"
  got_hash=$(sha256sum "$path" | cut -d' ' -f1)
  [ "$got_hash" = "$sha256" ] || fail "$id: sha256 $got_hash != declared $sha256"
  tar tf "$path" | grep -qx 'plugin.conf' || fail "$id: package has no plugin.conf"
  if [ -n "$entry" ]; then
    tar tf "$path" | grep -qx "$entry" || fail "$id: package is missing entry $entry"
  fi
done < "$records"

count=$(awk -F= '/^id=/{n++} END{print n+0}' "$index")
[ "$count" = "4" ] || fail "expected 4 plugins, got $count"
echo "ci-validate-index: ok serial=$serial plugins=$count"
