"""Writes the docs site's own pages into <site>/: the landing page (install,
links to each language's reference, the function table), and docs/KICK.md and
docs/architecture.md as pages. The references themselves are built by
scripts/build-docs.sh into <site>/<language>/.

    python3 scripts/docs/build_site.py <site>

Needs the `markdown` package.
"""

from __future__ import annotations

import html
import re
import sys
from pathlib import Path

import markdown

sys.path.insert(0, str(Path(__file__).parent))
import api_table  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
REPO = "https://github.com/AnisAbdellatif/kicksdk"

CSS = """
:root { --bg:#fbfbfa; --fg:#1d1f21; --muted:#5f6368; --line:#e3e3e0; --card:#ffffff; --accent:#1f8a4c; --code:#f1f1ee; }
@media (prefers-color-scheme: dark) {
  :root { --bg:#111214; --fg:#e6e6e3; --muted:#9a9ea4; --line:#2a2c30; --card:#17181b; --accent:#53fc18; --code:#1d1f23; }
}
* { box-sizing: border-box; }
body { margin:0; background:var(--bg); color:var(--fg); font:16px/1.6 system-ui,-apple-system,"Segoe UI",sans-serif; }
main { max-width: 1080px; margin: 0 auto; padding: 32px 20px 80px; }
header.top { display:flex; justify-content:space-between; align-items:center; gap:16px; padding-bottom:16px; border-bottom:1px solid var(--line); }
header.top a.brand { font-weight:700; font-size:20px; color:var(--fg); text-decoration:none; }
header.top a.brand span { color:var(--accent); }
nav a { color:var(--muted); text-decoration:none; margin-left:18px; font-size:14px; }
nav a:hover { color:var(--fg); }
h1 { font-size:34px; line-height:1.2; margin:40px 0 12px; }
h2 { margin-top:48px; font-size:22px; }
p.lead { font-size:18px; color:var(--muted); max-width:760px; }
a { color:var(--accent); }
code, pre { font-family: ui-monospace,SFMono-Regular,Menlo,Consolas,monospace; font-size:13.5px; }
code { background:var(--code); padding:1px 5px; border-radius:4px; }
pre { background:var(--code); padding:14px 16px; border-radius:8px; overflow-x:auto; }
pre code { background:none; padding:0; }
.cards { display:grid; grid-template-columns:repeat(auto-fill,minmax(300px,1fr)); gap:14px; margin-top:16px; }
.card { background:var(--card); border:1px solid var(--line); border-radius:10px; padding:16px 18px; }
.card h3 { margin:0 0 6px; font-size:17px; }
.card p { margin:6px 0; color:var(--muted); font-size:14px; }
.card pre { margin:10px 0 8px; font-size:12.5px; }
.card .links a { font-size:14px; margin-right:14px; }
.note { border-left:3px solid var(--accent); padding:4px 0 4px 14px; margin:14px 0; }
.tablewrap { overflow-x:auto; border:1px solid var(--line); border-radius:10px; }
table { border-collapse:collapse; width:100%; font-size:13px; }
th, td { text-align:left; padding:7px 10px; border-bottom:1px solid var(--line); white-space:nowrap; }
th { position:sticky; top:0; background:var(--card); }
tr:last-child td { border-bottom:none; }
td.missing { color:#c0392b; }
article { max-width: 860px; }
article table { white-space:normal; }
article td, article th { white-space:normal; vertical-align:top; }
footer { margin-top:64px; color:var(--muted); font-size:13px; }
"""

LANGS = [
    {
        "name": "Rust",
        "install": "cargo add kicksdk",
        "how": "The crate itself. Build requests, parse answers, drive a Session.",
        "ref": "rust/kicksdk/index.html",
        "registry": ("docs.rs", "https://docs.rs/kicksdk"),
    },
    {
        "name": "JavaScript / TypeScript",
        "install": "npm install kicksdk",
        "how": "WebAssembly, with connectChat for fetch + WebSocket. Types included.",
        "ref": "js/index.html",
        "registry": ("npm", "https://www.npmjs.com/package/kicksdk"),
    },
    {
        "name": "Python",
        "install": 'pip install "kicksdk[aio]"',
        "how": "PyO3, abi3 wheels; kicksdk.aio.connect_chat over asyncio.",
        "ref": "python/kicksdk.html",
        "registry": ("PyPI", "https://pypi.org/project/kicksdk/"),
    },
    {
        "name": "Dart / Flutter",
        "install": "dart pub add kicksdk",
        "how": "dart:ffi; the build hook fetches the native library. connectChat over dart:io.",
        "ref": "dart/index.html",
        "registry": ("pub.dev", "https://pub.dev/packages/kicksdk"),
    },
    {
        "name": "Elixir",
        "install": '{:kicksdk, "~> 0.1"}',
        "how": "A precompiled NIF. Values come back as string-keyed maps.",
        "ref": "elixir/index.html",
        "registry": ("hex", "https://hex.pm/packages/kicksdk"),
    },
]


def page(title: str, body: str, active: str = "") -> str:
    def nav(href: str, label: str) -> str:
        style = ' style="color:var(--fg)"' if label == active else ""
        return f'<a href="{href}"{style}>{label}</a>'

    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{html.escape(title)}</title><style>{CSS}</style></head>
<body><main>
<header class="top"><a class="brand" href="index.html">kick<span>sdk</span></a>
<nav>{nav("index.html#api", "API")}{nav("kick.html", "Kick reference")}{nav("architecture.html", "Architecture")}{nav(REPO, "GitHub")}</nav></header>
{body}
<footer>MIT OR Apache-2.0. Not affiliated with Kick. Built from <a href="{REPO}">{REPO.split("github.com/")[1]}</a>.</footer>
</main></body></html>
"""


def landing() -> str:
    rows = api_table.table()
    cards = "".join(
        f"""<div class="card"><h3>{lang["name"]}</h3><p>{lang["how"]}</p>
<pre><code>{html.escape(lang["install"])}</code></pre>
<div class="links"><a href="{lang["ref"]}">API reference</a><a href="{lang["registry"][1]}">{lang["registry"][0]}</a></div></div>"""
        for lang in LANGS
    )
    # Each column without its package's prefix, which the header names once.
    prefix = {"rust": "kicksdk::", "javascript": "", "python": "kicksdk.", "dart": "", "elixir": "Kicksdk."}
    head = "".join(
        f"<th>{h}</th>"
        for h in ["Rust <code>kicksdk::</code>", "JavaScript", "Python <code>kicksdk.</code>", "Dart", "Elixir <code>Kicksdk.</code>"]
    )

    def cell(r, lang):
        v = r[lang]
        if not v:
            return '<td class="missing">missing</td>'
        return f"<td><code>{html.escape(v.removeprefix(prefix[lang]))}</code></td>"

    body_rows = "".join("<tr>" + "".join(cell(r, lang) for lang in api_table.LANGS) + "</tr>" for r in rows)
    return page(
        "kicksdk",
        f"""
<h1>Kick, without the I/O.</h1>
<p class="lead">kicksdk knows Kick: which requests to make and what the answers look like, how the realtime chat
feed speaks, how webhooks are signed. Your code sends the requests and holds the sockets; kicksdk does the rest,
from one Rust core, in five languages.</p>

<h2>Install</h2>
<div class="cards">{cards}</div>

<h2>Two things to know first</h2>
<p class="note"><strong>The chat feed only accepts kick.com.</strong> Kick's realtime server refuses a WebSocket
whose <code>Origin</code> is not <code>https://kick.com</code>. Servers set it (every <code>connectChat</code>
does); a browser cannot, so in a browser the feed works only on kick.com, in a page or an extension's content
script. Anywhere else, relay it through a server.</p>
<p class="note"><strong>Much of Kick is unofficial.</strong> The website API and the realtime feed are what
kick.com's own frontend uses, not a published API, and can change without notice. The official API and webhooks
are the stable part. Everything known about each, with where it was seen, is in the
<a href="kick.html">Kick reference</a>.</p>

<h2 id="api">Every function, in every language</h2>
<p>Each core function and its name in each package, read from the sources when this site was built. Requests
come back as <code>{{method, url, headers, body}}</code> for your HTTP client; parsers take the status and body.
The realtime <code>Session</code> (<code>open</code>, <code>receive</code>, <code>tick</code>, <code>refresh</code>,
<code>closed</code>) and <code>connectChat</code> are in each language's reference.</p>
<div class="tablewrap"><table><thead><tr>{head}</tr></thead><tbody>{body_rows}</tbody></table></div>
""",
    )


def markdown_page(src: Path, title: str, active: str) -> str:
    text = src.read_text()
    # Links between the docs point at their pages here.
    text = text.replace("](../CONTRIBUTING.md)", f"]({REPO}/blob/main/CONTRIBUTING.md)")
    text = re.sub(r"\]\((?!https?:)([^)#]+\.md)\)", lambda m: f"]({REPO}/blob/main/{m.group(1)})", text)
    body = markdown.markdown(text, extensions=["tables", "fenced_code", "toc"])
    return page(title, f"<article>{body}</article>", active)


def main(site: Path) -> None:
    site.mkdir(parents=True, exist_ok=True)
    (site / "index.html").write_text(landing())
    (site / "kick.html").write_text(markdown_page(ROOT / "docs/KICK.md", "Kick reference - kicksdk", "Kick reference"))
    (site / "architecture.html").write_text(
        markdown_page(ROOT / "docs/architecture.md", "Architecture - kicksdk", "Architecture")
    )
    (site / ".nojekyll").write_text("")


if __name__ == "__main__":
    main(Path(sys.argv[1] if len(sys.argv) > 1 else "site"))
