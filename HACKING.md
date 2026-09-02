# Hacking on Topaz

This file is for people who want to read the compiler, watch it work on a
program of their own, or change it. Nothing here is required to use Topaz.
For the language itself, see https://topaz.ooo.

## Where things are

The Rust workspace lives in `compiler/`. A program moves through it in this order.

| crate | what it does |
|---|---|
| `topaz_lexer` | Raw lexing, template lexing, and layout normalization. |
| `topaz_parser` | Parsing and parser recovery. |
| `topaz_resolve` | Module and name resolution. |
| `topaz_check` | Static checking and typed program data. |
| `topaz_lower` | Lowering checked programs into engine-neutral IR. |
| `topaz_interp` | Running checked programs directly. |
| `topaz_emit` | The Rust source backend. |
| `topaz_emit_py` | The Python source backend. |
| `topaz_cli` | The `topaz` command. |

Around that spine:

| crate | what it does |
|---|---|
| `topaz_syntax` | Token kinds, language versions, and AST types. |
| `topaz_diag` | Source locations, source maps, and diagnostics. |
| `topaz_hir` | Typed and lowered intermediate representations. |
| `topaz_value` | The runtime value shared by the interpreter and generated code. |
| `topaz_rt` | Runtime linked into generated Rust programs. |
| `topaz_package` | Manifests, lockfiles, and package sources. |
| `topaz_kernel` | The compiler-kernel facade and its source-fact protocol. |
| `topaz_self_frontend` | The compiler written in Topaz, and its Rust integration. |
| `topaz_stage1_runtime` | Loader and runtime shell for the checked-in compiler program image. |
| `topaz_product_runtime` | Runtime for compiler program images and lowered products. |
| `topaz_lispex_embed`, `topaz_lispex_product` | The embedded Lispex evaluator and its packaging. |
| `topaz_host_none`, `topaz_host_native`, `topaz_host_http` | Host services for generated programs. |
| `topaz_execution_sandbox`, `topaz_execution_supervisor` | Sandboxing and supervision for executions. |
| `topaz_mcp`, `topaz_mcp_worker` | The stdio MCP server and its worker. |

`topaz_difftest*` and the Lispex suite crates are differential and profile
harnesses. They are excluded from the root workspace and run from
`compiler/difftest/` or an explicit manifest path.

The compiler written in Topaz lives in
`compiler/crates/topaz_self_frontend/topaz/`. Thirty-eight files across the
top level and the phase modules, of which `checker.tpz`, `parser.tpz`, and
`resolver.tpz` are the facades of the three big phases. Select it with
`--compiler self`. The Rust crates above implement the same language, are
the default when no compiler is selected, and stay in the tree as the
bootstrap and recovery path.

## Watching the compiler work

Take any program, for example `examples/hello.tpz`, and ask for the
front end's view of it.

```sh
topaz compiler preview examples/hello.tpz --terminal ast --out-dir /tmp/hello-ast
```

The output directory contains `tokens.jsonl`, `ast.jsonl`, and
`resolved.jsonl`, one record per line, plus the diagnostics and the request
that produced them. `--terminal typed` goes one phase further and includes
the typed observation. Change a line in the program, run it again, and diff
the two directories. Then open the crate that produced the difference.

`topaz run` executes a program with the interpreter. `topaz build` writes a
native program, and `--target python` writes the Python form. All three start
from the same checked program, which is the easiest place to start reading
if you want to follow one construct end to end.

## Building

Rust 1.96.0 is pinned in `compiler/rust-toolchain.toml`. From `compiler/`:

```sh
cargo build
cargo test
```

The Python backend tests use CPython 3.13.14. The differential harnesses are
separate workspaces and are not part of the default `cargo test`.

## Changing the compiler

The compiler builds itself, so a change to the compiler sources takes two
builds to become the compiler you run. The loop is the usual one for a
self-hosting language.

1. Edit the Topaz sources under `compiler/crates/topaz_self_frontend/topaz/`.
   Write them in the language the current compiler already accepts. A new
   construct can be used inside the compiler itself only from the next
   generation on.

2. `cargo build`. The edited sources are embedded into the binary, but the
   binary still runs the previous compiler image.

3. Regenerate the compiler image with the current compiler and re-embed it.
   Two commands, both run from `compiler/`.

   ```sh
   cargo run --release --locked -p topaz_self_frontend --bin regenerate_stage1_c1 -- \
     --producer interpreted \
     --artifact-manifest generated/topaz_compiler_generated_artifacts.json \
     --manifest-identity refresh \
     --out-rust /tmp/R0.rs \
     --out-manifest /tmp/c1.manifest.json
   cargo run --release --locked -p topaz_program_image_extract -- \
     --generated-rust /tmp/R0.rs \
     --out-image generated/topaz_compiler_program_image.bin \
     --out-target-facts generated/topaz_compiler_target_facts.json \
     --out-manifest generated/topaz_compiler_generated_artifacts.json
   ```

   The first command runs the compiler sources through the interpreter and
   produces the generated Rust for the new compiler. The second extracts the
   program image and target facts from that Rust and records what it
   extracted, so the next build can embed it.

4. `cargo build` again. The binary now runs your compiler.

If you add or remove a file under `topaz_self_frontend/topaz/`, list it in
`compiler/crates/topaz_self_frontend/src/source_inventory.rs` as well.

Changes to the standard library, the backends, the CLI, or the language
server do not go through this loop. They are ordinary Rust edits followed by
`cargo build`.

## Examples

`examples/` holds small complete programs: `hello.tpz`, `factorial.tpz`,
`choseong/`, `json-format/`, `char-counter/`, `data-lens/`, and others.
Each one runs with `topaz run` and builds with `topaz build`.
