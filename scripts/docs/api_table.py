"""Every core function, and its name in each language.

Reads the sources rather than a hand-kept list, so the table cannot drift:

- the core's functions: `dispatch::FUNCTIONS` (crates/kicksdk/src/dispatch.rs);
- JavaScript: each `#[wasm_bindgen(js_name = …)]` in crates/kicksdk-wasm and
  the core function its body calls;
- Python: each `def` in bindings/python/python/kicksdk/*.py and the dispatch
  name it passes;
- Dart: each static method in bindings/dart/lib/src/api.dart and the dispatch
  name it passes;
- Elixir: each `def` in bindings/elixir/lib/kicksdk/*.ex, and either the NIF
  it calls (and the core function that NIF calls, crates/kicksdk-nif) or the
  dispatch name it passes to `Kicksdk.call/2`.

`python3 scripts/docs/api_table.py` prints the table as Markdown, with the
functions a binding is missing marked; `--check` exits 1 if any is.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LANGS = ["rust", "javascript", "python", "dart", "elixir"]
MODULES = r"(events|content|realtime|public_api|website|webhooks)"


def core_functions() -> list[str]:
    src = (ROOT / "crates/kicksdk/src/dispatch.rs").read_text()
    block = src[src.index("pub const FUNCTIONS"):]
    block = block[: block.index("];")]
    return re.findall(r'"([a-z_]+\.[a-z_]+)"', block)


def rust(name: str) -> str:
    module, fn = name.split(".")
    return f"kicksdk::{module}::{fn}"


def javascript() -> dict[str, str]:
    src = (ROOT / "crates/kicksdk-wasm/src/lib.rs").read_text()
    out: dict[str, str] = {}
    for chunk in src.split("#[wasm_bindgen(js_name = ")[1:]:
        js = chunk[: chunk.index(")")]
        body = chunk[: chunk.find("#[wasm_bindgen") if "#[wasm_bindgen" in chunk else len(chunk)]
        m = re.search(MODULES + r"::(\w+)\(", body)
        if m:
            name = f"{m.group(1)}.{m.group(2)}"
            # Two JS names can reach one core function (the app token and the
            # realtime token both parse with their module's parse_token); the
            # first one, in file order, is the one for this module.
            out.setdefault(name, js)
    return out


def python() -> dict[str, str]:
    out: dict[str, str] = {}
    for path in sorted((ROOT / "bindings/python/python/kicksdk").glob("*.py")):
        module = path.stem
        current = None
        for line in path.read_text().splitlines():
            d = re.match(r"def (\w+)\(", line)
            if d:
                current = d.group(1)
            for m in re.finditer(r'"' + MODULES + r'\.(\w+)"', line):
                if current and not current.startswith("_"):
                    out.setdefault(f"{m.group(1)}.{m.group(2)}", f"kicksdk.{module}.{current}")
    return out


def dart() -> dict[str, str]:
    out: dict[str, str] = {}
    cls = None
    method = None
    for line in (ROOT / "bindings/dart/lib/src/api.dart").read_text().splitlines():
        c = re.match(r"(?:abstract final )?class (\w+)", line)
        if c:
            cls = c.group(1)
        s = re.search(r"static [\w<>?, ]+ (\w+)\(", line)
        if s:
            method = s.group(1)
        for m in re.finditer(r"'" + MODULES + r"\.(\w+)'", line):
            if cls and method:
                out.setdefault(f"{m.group(1)}.{m.group(2)}", f"{cls}.{method}")
    return out


def elixir() -> dict[str, str]:
    nif_src = (ROOT / "crates/kicksdk-nif/src/lib.rs").read_text()
    nif_to_core: dict[str, str] = {}
    for chunk in nif_src.split("#[rustler::nif]")[1:]:
        f = re.search(r"fn (\w+)", chunk)
        m = re.search(MODULES + r"::(\w+)\(", chunk)
        if f and m:
            nif_to_core[f.group(1)] = f"{m.group(1)}.{m.group(2)}"
    out: dict[str, str] = {}
    for path in sorted((ROOT / "bindings/elixir/lib/kicksdk").rglob("*.ex")):
        # `call_request(\n  "website.x", …)` as formatted: one line again.
        text = re.sub(r'\(\s*\n\s*"', '("', path.read_text())
        mod = re.search(r"defmodule ([\w.]+)", text)
        if not mod or mod.group(1) == "Kicksdk.Native":
            continue
        current = None
        for line in text.splitlines():
            d = re.match(r"\s*def (\w+[?!]?)", line)
            if d:
                current = d.group(1)
            for n in re.finditer(r"Native\.(\w+)\(", line):
                core = nif_to_core.get(n.group(1))
                if core and current:
                    out.setdefault(core, f"{mod.group(1)}.{current}")
            # Wrappers over Kicksdk.call/2 name the core function outright.
            for n in re.finditer(r'call(?:_request)?\("' + MODULES + r'\.(\w+)"', line):
                if current:
                    out.setdefault(f"{n.group(1)}.{n.group(2)}", f"{mod.group(1)}.{current}")
    return out


def table() -> list[dict[str, str | None]]:
    js, py, dt, ex = javascript(), python(), dart(), elixir()
    return [
        {
            "function": name,
            "rust": rust(name),
            "javascript": js.get(name),
            "python": py.get(name),
            "dart": dt.get(name),
            "elixir": ex.get(name),
        }
        for name in core_functions()
    ]


def markdown(rows) -> str:
    head = "| Function | Rust | JavaScript | Python | Dart | Elixir |\n|---|---|---|---|---|---|\n"
    cell = lambda v: f"`{v}`" if v else "-"  # noqa: E731
    return head + "".join(
        f"| `{r['function']}` | " + " | ".join(cell(r[lang]) for lang in LANGS) + " |\n" for r in rows
    )


if __name__ == "__main__":
    rows = table()
    missing = [(r["function"], lang) for r in rows for lang in LANGS if not r[lang]]
    if "--check" in sys.argv:
        for fn, lang in missing:
            print(f"{lang}: no binding for {fn}", file=sys.stderr)
        sys.exit(1 if missing else 0)
    print(markdown(rows))
    print(f"{len(rows)} functions; {len(missing)} gaps", file=sys.stderr)
