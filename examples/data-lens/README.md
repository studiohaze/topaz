# Data Lens

Data Lens is the maintained non-toy Web Application Loop workload for the
Topaz 5.8 product program. The application opens and parses JSON and CSV locally,
normalizes Unicode labels through a content-hash-pinned registry dependency,
filters and sorts typed rows, computes aggregate summaries, and renders an
interactive offline product through the generated Topaz Web host. It exports
the transformed rows and explicitly saves, restores, or forgets only the
query, active-filter, and sort preferences; imported source and rows are not
persisted.

The package deliberately exercises multiple source and test modules, text,
checkbox, select, button, keyboard, and submit events, bounded local file and
state capabilities, deterministic data-path errors, styles, package locking,
documentation, LSP, development serving, and source-free static execution.
JavaScript remains generated host and ABI glue; the application model,
parsing, transformation, session encoding, update, and view logic are Topaz.

The source registry under `registry/` is an authoring fixture. Run `topaz
vendor --root . --from registry` to create the locked offline dependency before
using `--locked` package commands.

From this directory, the maintained local lifecycle is:

```text
topaz check --root . --locked
topaz fmt --root . --check
topaz test tests/application.tpz --root . --locked
topaz test tests/exporting.tpz --root . --locked
topaz test tests/parsing.tpz --root . --locked
topaz test tests/transform.tpz --root . --locked
topaz doc --root . --locked --out-dir <docs-dir>
topaz build --root . --locked --release --out-dir <product-dir>
```

The selected test entries remain inside the package context, so they use the
same manifest, lockfile, vendored dependency, language mode, and module root as
the application. A no-entry `topaz test --root . --locked` runs the manifest
entry; it is not a replacement for the four explicit test modules above.

The installed-product check also opens a generated 256-row multilingual CSV
and 128-row JSON document through Unicode filenames, validates semantic
filter/sort/summary/export results, repeats file open and session save eight
times, checks focus recovery, and reloads with source removed and external
network access blocked.
