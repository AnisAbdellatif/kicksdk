# Examples

One small app per language, all doing the same thing: look a channel up and
print its chat as it arrives. Run from each folder; the argument is the
channel's slug.

| Language | Run | First |
|---|---|---|
| Rust | `cargo run -- xqc` | - |
| JavaScript | `node chat.mjs xqc` | `scripts/build-js.sh` |
| Python | `python chat.py xqc` | `pip install "../../bindings/python[aio]"` |
| Dart | `dart run bin/chat.dart xqc` | - (the build hook compiles the library) |
| Elixir | `elixir chat.exs xqc` | - (Mix.install compiles the NIF) |

The Rust and Elixir apps drive `Session` themselves around a plain HTTP client
and WebSocket; JavaScript, Python and Dart use their package's `connectChat`,
which does the same. Every one of them needs Rust for now, since they build
kicksdk from this repository rather than downloading a release.
