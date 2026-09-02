# Compiler observation contracts v1

This directory is the machine identity registry for the non-normative
Bootstrap Foundations observation formats. `schemas.json` is itself canonical
`topaz.canonical-json/v1`; the Rust validator reads it and refuses a registry
whose exact identity set drifts.

The semantic field and ordering rules are implemented by the exhaustive Rust
models and validator in `topaz_kernel`. Adversarial fixtures live with those
models so a new syntax variant fails the exhaustive projection match at
compile time and mutations of source-set, tokens, AST, resolved, diagnostics,
request, response, provenance, manifest, ordering, or digests fail validation.

These observations do not replace the language SPEC, decision ledger,
diagnostic registry, package format, or release manifest.

`bootstrap-profile.json` is the machine-readable allow/deny inventory for
`topaz check --profile bootstrap --locked`. It narrows current canonical
Topaz without adding grammar or a compiler-only dialect.

`stage2-fixed-point-corpus.json` is the bounded C1/C2 comparison inventory.
It keeps semantic observations, raw generated source, producer provenance,
and native binaries as separate dispositions; only the first two are
fixed-point equality layers.
