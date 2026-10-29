#!/usr/bin/env bash
# Writes the landing page (index.html) next to index.json, from index.json
# itself, so the page cannot drift from what the app sees.
# Usage: scripts/make-landing.sh <out-dir>      (called by make-index.sh)
set -euo pipefail
out="${1:?usage: make-landing.sh <out-dir>}"
index="$out/index.json"
[ -f "$index" ] || { echo "no $index" >&2; exit 1; }

cards="$(awk '
function esc(s) { gsub(/&/, "\\&amp;", s); gsub(/</, "\\&lt;", s); gsub(/>/, "\\&gt;", s); gsub(/"/, "\\&quot;", s); return s }
function flush(   kb, sz) {
  if (id == "") return
  sz = size + 0
  kb = (sz >= 1048576) ? sprintf("%.1f MB", sz / 1048576) : sprintf("%.0f KB", sz / 1024)
  printf "<tr>\n"
  printf "<td data-l=\"Plugin\"><strong>%s</strong><br><code>%s</code></td>\n", esc(name), esc(id)
  printf "<td data-l=\"Version\">%s</td>\n", esc(version)
  printf "<td data-l=\"Kind\">%s</td>\n", esc(kind)
  printf "<td data-l=\"About\">%s</td>\n", esc(desc)
  printf "<td data-l=\"Size\">%s</td>\n", kb
  printf "<td data-l=\"sha256\"><code title=\"%s\">%s</code></td>\n", esc(sha), esc(substr(sha, 1, 12))
  printf "<td data-l=\"Package\"><a href=\"%s\">%s</a></td>\n", esc(url), esc(pkg)
  printf "</tr>\n"
  id = ""
}
/^id=/ { flush(); id = substr($0, 4); name = id; version = kind = desc = url = size = sha = pkg = "" }
/^name=/ { name = substr($0, 6) }
/^version=/ { version = substr($0, 9) }
/^kind=/ { kind = substr($0, 6) }
/^description=/ { desc = substr($0, 13) }
/^url=/ { url = substr($0, 5); n = split(url, p, "/"); pkg = p[n] }
/^size=/ { size = substr($0, 6) }
/^sha256=/ { sha = substr($0, 8) }
END { flush() }
' "$index")"
count="$(grep -c '^id=' "$index")"

cat > "$out/index.html" <<HTML
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Σscribe plugins</title>
<meta name="description" content="Plugins for Σscribe, the Markdown notebook for the Kobo Forma.">
<style>
:root { --bg:#fff; --fg:#111; --mute:#555; --line:#ccc; --soft:#f2f2f2; }
@media (prefers-color-scheme: dark) { :root { --bg:#111; --fg:#eee; --mute:#aaa; --line:#444; --soft:#1c1c1c; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 17px/1.55 system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width: 56rem; margin: 0 auto; padding: 2rem 16px 4rem; }
h1, h2 { font-family: Georgia, "Times New Roman", serif; font-weight: normal; line-height: 1.2; }
h1 { font-size: 2.2rem; margin: 0 0 .3rem; }
h2 { font-size: 1.4rem; margin: 2.4rem 0 .6rem; padding-bottom: .3rem; border-bottom: 1px solid var(--line); }
.lede { color: var(--mute); margin: 0 0 1rem; }
a { color: var(--fg); text-decoration-thickness: 1px; text-underline-offset: 2px; }
a:hover { background: var(--fg); color: var(--bg); }
code, pre { font: .88em ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace; background: var(--soft); }
code { padding: .05em .3em; border-radius: 3px; word-break: break-word; }
pre { padding: .8rem 1rem; overflow-x: auto; border: 1px solid var(--line); }
ol, ul { padding-left: 1.3rem; }
li { margin: .3rem 0; }
table { width: 100%; border-collapse: collapse; font-size: .95rem; }
th, td { text-align: left; vertical-align: top; padding: .55rem .6rem; border-bottom: 1px solid var(--line); }
th { font-weight: 600; border-bottom: 2px solid var(--fg); }
@media (max-width: 40rem) {
  thead { position: absolute; left: -9999px; }
  table, tbody, tr, td { display: block; }
  tr { border: 1px solid var(--line); margin: 0 0 1rem; padding: .3rem 0; }
  td { border: 0; padding: .25rem .8rem; }
  td::before { content: attr(data-l); display: block; font-size: .75rem; color: var(--mute); text-transform: uppercase; letter-spacing: .05em; }
}
footer { margin-top: 3rem; color: var(--mute); font-size: .9rem; }
</style>
</head>
<body>
<main>
<h1>Σscribe plugins</h1>
<p class="lede">Extra features for <a href="https://github.com/armandomartires/sigma-scribe">Σscribe</a>, the Markdown notebook for the Kobo Forma.</p>

<h2>What plugins are</h2>
<p>A plugin is a small package that adds a font, a dictionary, a theme, a side-drawer pane or a full screen to Σscribe. Code plugins are WebAssembly modules and run in a sandbox: they can only do what the app offers them through its plugin interface. Data packs run no code at all. Each package is checked against the size and sha256 listed below before the app unpacks it.</p>

<h2>Install from the app</h2>
<ol>
<li>Connect the Kobo to Wi-Fi.</li>
<li>Open <strong>Settings → Plugins → Repository → Check for plugins</strong>.</li>
<li>Pick a plugin and install it, then switch it on in Settings → Plugins.</li>
</ol>

<h2>Available plugins ($count)</h2>
<table>
<thead><tr><th>Plugin</th><th>Version</th><th>Kind</th><th>About</th><th>Size</th><th>sha256</th><th>Package</th></tr></thead>
<tbody>
$cards
</tbody>
</table>
<p class="lede">The same list the app reads, as <a href="index.json">index.json</a>.</p>

<h2>Install by hand over USB</h2>
<ol>
<li>Download the package (<code>.tar</code>) above and check its sha256.</li>
<li>Connect the Kobo to your computer by USB and open its storage.</li>
<li>Unpack the archive into <code>.adds/sigma-scribe/plugins/&lt;id&gt;/</code>, so that <code>plugin.conf</code> sits directly inside that folder.</li>
</ol>
<pre>mkdir -p /media/KOBOeReader/.adds/sigma-scribe/plugins/graph-map
tar -xf graph-map-&lt;version&gt;.tar -C /media/KOBOeReader/.adds/sigma-scribe/plugins/graph-map</pre>
<p>Eject the Kobo, then open Σscribe and switch the plugin on in Settings → Plugins.</p>

<h2>Links</h2>
<ul>
<li><a href="https://github.com/armandomartires/sigma-scribe">Σscribe on GitHub</a>, the app</li>
<li><a href="https://armandomartires.github.io/sigma-scribe/">Project page</a></li>
<li><a href="https://github.com/armandomartires/sigma-scribe/wiki">Wiki</a></li>
<li><a href="https://github.com/armandomartires/sigma-scribe/blob/main/docs/dev/plugin-api.md">Plugin API</a>, for authors</li>
<li><a href="https://github.com/armandomartires/sigma-scribe-plugins#readme">This repository</a>, with the manifest format and how to publish a plugin</li>
</ul>

<footer>Generated from index.json by scripts/make-landing.sh.</footer>
</main>
</body>
</html>
HTML
echo "wrote $out/index.html"
