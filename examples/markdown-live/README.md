# Markdown Live — a Markdown editor whose application logic is written in Topaz

A live Markdown editor in the browser: you type Markdown on the left, rendered HTML appears
on the right, re-rendered on every keystroke. The maintained Web App target parses Markdown
directly into Topaz `Html<Msg>` values; the generated host creates the safe DOM, and no
JavaScript Markdown library or raw-markup escape hatch is used. The original single-file
Markdown→HTML renderer remains in `render.tpz` for CLI and Playground compatibility.
The maintained application can explicitly open a Markdown file, save or forget a browser-local
draft, recover that draft after reload, and download the current source.

> Submitted to a coding competition by a 방송대 (Korea National Open University) Computer
> Science student — in a language they wrote themselves. The parser is the app; the app
> drove new language features (see _How the app grew the compiler_).

## Legacy renderer correctness boundary

Topaz can run this renderer two ways from the same source: `topaz run` through the
interpreter, or `topaz build` through the native Rust-emitting backend. Those two paths are
held byte-identical by a differential test suite: **816 pinned fixtures** compare interpreter
and emitted output for the same input.

The legacy browser demo runs the same Topaz renderer source through the WASM playground runner
(`run_with_input` — the interpreter compiled to WebAssembly). That wrapper returns captured
stdout without the final newline that CLI `print` writes, so the browser preview is the same
rendered HTML _content_, not a byte-for-byte copy of CLI stdout. A dedicated integration test
(`markdown_live_renderer_produces_safe_html`) pins representative rendered HTML and the
security properties below.

## Supported Markdown

**Blocks:** headings (`#`/`##`/`###`), paragraphs, unordered lists (`-`), ordered lists
(`1.`), fenced code blocks (` ``` `), blockquotes (`>`), horizontal rules (`---`), and
GitHub-flavored **tables** (`| h | h |` + `|---|:--:|` + rows).

**Inline:** `**bold**`, `*italic*`, `` `code` ``, `~~strikethrough~~`, links `[text](url)`,
images `![alt](url)`, and backslash escapes (`\*`, `` \` ``, `\[`, …).

All user text is HTML-escaped; CRLF input is normalized.

_Not yet supported (deliberately): nested lists, nested emphasis, reference links, raw HTML
passthrough. The renderer never emits raw user HTML._

## Legacy HTML renderer security boundary

The legacy compatibility renderer emits escaped HTML for its CLI and Playground path. The
maintained Web App does not inject that output through `innerHTML`; it builds typed `Html<Msg>`
values and the generated host creates DOM nodes. The legacy renderer retains these defenses:

- **All text is HTML-escaped** (`&` `<` `>`) in every position — paragraphs, headings, list
  items, table cells, code, link/image text. The renderer emits only its own tags.
- **Link/image URLs use a default-deny allowlist.** A URL gets an `href`/`src` only if it is
  `http://`, `https://`, `mailto:`, root-relative `/`, anchor `#`, `./`/`../`, or a
  scheme-less relative path. For anything else — `javascript:`, `data:`, `vbscript:`, unknown
  schemes, protocol-relative `//host`, backslash-authority `\\host`, and whitespace tricks
  (the URL's leading whitespace is trimmed before the check, and any `:` before the first `/`
  disqualifies it) — no `href`/`src` is emitted; the link text or image alt text remains inert.
- **Attribute values are escaped** (including `"` → `&quot;`), so a URL or alt text cannot
  break out of the quoted attribute to inject an event handler.

## Build the artifacts

The maintained package lifecycle is:

```sh
topaz check --root . --locked
topaz fmt --root . --check
topaz test tests/application.tpz --root . --locked
topaz test tests/render.tpz --root . --locked
topaz doc --root . --locked --out-dir /tmp/markdown-live-docs
topaz build --root . --locked --release --out-dir /tmp/markdown-live-product
```

Serve the final output directory from a local static origin and open its
`index.html`. The complete product contains the generated host, WebAssembly,
styles, licenses, and artifact manifest; it does not load Topaz source or a
Markdown library at runtime. Its file and draft operations are explicit
lifecycle v2 commands and require the declared `open_text`, `download_text`,
and `local_state` capabilities.

The installed-product check opens a generated 16-64 KiB multilingual Markdown
document through Unicode filenames, repeats open and draft save eight times,
checks typed-DOM structure and raw-HTML-as-text behavior, reloads the saved
draft, and compares the downloaded bytes after source and network removal.
The maintained renderer coalesces adjacent plain text into one `Html.Text`
node so document size does not become one DOM node per scalar.

The older Playground/CLI compatibility artifacts remain available:

`./examples/markdown-live/build.sh` (from the repo root) builds both submission artifacts from the single Topaz source: a native binary `dist/markdown-live` (stdin Markdown → stdout HTML) and the browser WASM module. Interpreter, native binary, and the WASM playground runner all render the same HTML for the same input (the differential suite holds `topaz run` ≡ `topaz build` byte-identical).

## Run it

**In the browser (the single-WASM app):**

```sh
wasm-pack build playground/topaz_wasm --target web --out-dir pkg
# then serve the repo root and open playground/markdown_live/index.html
python3 -m http.server
```

**On the command line (same renderer, native):**

```sh
echo '# Hello *Topaz*' | topaz run examples/markdown-live/render.tpz
# <h1>Hello <em>Topaz</em></h1>

# prove the two engines agree:
echo '# Hi' | topaz run   examples/markdown-live/render.tpz
echo '# Hi' | topaz build examples/markdown-live/render.tpz --out-dir /tmp/m --run
```

## How the app grew the compiler

Building the editor surfaced real gaps in Topaz, each closed by its own reviewed,
differential-tested change (the point of the exercise was to _push the language_, not stay
inside it):

- **`input()`** — a host builtin so a Topaz program can receive the textarea text (the
  bridge that makes a single-WASM interactive app possible).
- **Array stdlib `slice` / `join`** — read-only `Array<T>` methods used throughout the
  character-array parser, turning quadratic gymnastics into clean substring/join code.
  (`indexOf` exists in the same stdlib surface, but this renderer does not currently call it.)

## How it works

The renderer is a pure function `mdToHtml(src: string) -> string`, so rendering is
**stateless** — each keystroke re-renders from scratch, no event loop, no shared mutable
state. Strings are inspected as scalar arrays (`.scalars()`); a line-driven block loop
handles block structure and a recursive `inline` function handles spans. The entry point is
literally `print(mdToHtml(input()))`.
