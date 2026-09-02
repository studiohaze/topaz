# Topaz Application Loop example

`release-inventory/` is the maintained Topaz 5.6.6 Application Loop workload. It uses
multiple application modules, a registry dependency that is vendored before
offline execution, explicit filesystem capabilities, CSV and TOML data, and
deterministic `Result` failures.

The source registry under `registry/` is an input fixture, not a runtime
dependency. The installed-product check copies this tree, runs `topaz vendor`,
deletes the copied registry, and requires `--locked` for every later package
operation. Generated vendor, lock, docs, build, and report outputs stay outside
the committed example.

Run the complete installed-product loop with:

```text
node compiler/scripts/check-application-loop.mjs --topaz <installed-topaz>
```

The checker copies the CLI into an isolated directory, initializes the final
package root, and authors the maintained workload inventory into that root. It
removes the registry after vendoring and deletes the application source after
building, then returns one `topaz.application-loop/v2` JSON result. It is a
focused product gate; it does not invoke the full differential, LIT, or
platform campaigns.
