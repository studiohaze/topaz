# Examples

Small, self-contained Topaz programs. Every file here is checked by
the toolchain — it parses, type-checks (`topaz check`), and runs
(`topaz run`). Each is a single module, so it can also be compiled to a
native binary with `topaz build <file> --out-dir out --run`.

| File | Shows |
| --- | --- |
| [`hello.tpz`](hello.tpz) | String interpolation — the smallest program. |
| [`factorial.tpz`](factorial.tpz) | A typed, recursive function with an early `return`. |
| [`accumulate.tpz`](accumulate.tpz) | Mutable state (`let mut`), an array literal, a `for` loop, `+=` accumulation, and `.length`. |
| [`pipelines.tpz`](pipelines.tpz) | Closures, composition (`>>`), the pipe operator (`\|>`), and default arguments. |
| [`records.tpz`](records.tpz) | Record literals, functional update, and destructuring. |
| [`pattern-matching.tpz`](pattern-matching.tpz) | `match` with literals, guards, ranges, and a catch-all. |
| [`result-pipeline.tpz`](result-pipeline.tpz) | `Option`/`Result`, matching `Some`/`None`, and the `?` operator. |

Run the following commands from the repository root.

```text
topaz run examples/factorial.tpz
```

Or compile and run it as a native binary.

```text
topaz build examples/factorial.tpz --out-dir out --run
```

## Dogfood

Concrete v5.4 "Rust replacement" targets that exercise stdlib and tooling surface together.

| File | Shows |
| --- | --- |
| [`dogfood/manifest-audit.tpz`](dogfood/manifest-audit.tpz) | Topaz manifest auditing with TOML, JSONValue accessors, records, and sorted arrays. |
| [`dogfood/corpus-report.tpz`](dogfood/corpus-report.tpz) | Corpus manifest reporting with JSON decode, arrays, and deterministic sorting. |
| [`dogfood/signature-site.tpz`](dogfood/signature-site.tpz) | Signature-site Markdown generation with CSV and Regex. |
| [`dogfood/http-handler.tpz`](dogfood/http-handler.tpz) | Deterministic HTTP handler logic over explicit request/response values. |
| [`dogfood/mini-expr.tpz`](dogfood/mini-expr.tpz) | A tiny tokenizer/interpreter using enums, Result, Regex, and loops. |
| [`dogfood/hash-tool.tpz`](dogfood/hash-tool.tpz) | CLI file/stdin hashing with explicit args, FS bytes, Bytes, and Hash. |

## Readiness probes

The v5.6 local-RC probes are executable evidence, not Lispex adoption claims.

| File | Shows |
| --- | --- |
| [`readiness/lispex-recursive-values.tpz`](readiness/lispex-recursive-values.tpz) | General recursive values through an enum containing `Array<Form>`. |
| [`readiness/lispex-cell-state.tpz`](readiness/lispex-cell-state.tpz) | Assignment-cell-like state through a mutable closure capture. |
| [`readiness/lispex-trampoline.tpz`](readiness/lispex-trampoline.tpz) | A 10,000-step explicit trampoline without recursive stack growth. |
| [`readiness/lispex-deterministic-surface.tpz`](readiness/lispex-deterministic-surface.tpz) | Deterministic derived rendering and a structured JSON error value. |
| [`readiness/lispex-portable-data.tpz`](readiness/lispex-portable-data.tpz) | Portable records, enums, arrays, maps, and sets. |

## Applications

Larger programs that are whole apps written in Topaz (each ships as a single offline WASM,
with its source public):

| App | What it is |
| --- | --- |
| [`living-docs/`](living-docs/) | **Flagship.** A computational, reproducible Markdown editor: fenced ```` ```topaz ```` blocks **execute** and render their results inline (value / table / sparkline), the document is one offline WASM artifact, and a reproducibility stamp verifies the computed blocks recompute. The default document is a real monthly budget. |
| [`markdown-live/`](markdown-live/) | The Markdown→HTML renderer (pure Topaz) Living Docs builds on — a live editor with default-deny link/image security. [`markdown-live/README.md`](markdown-live/README.md). |
