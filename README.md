# Topaz

Topaz is a programming language for typesetting. The compiler is written in Topaz and emits Rust.

This repository contains the compiler source and examples for Topaz 5.21.0.

## Install

On macOS and Linux, run the following command.

```sh
curl -fsSL https://topaz.ooo/install.sh | sh
```

On Windows, run the following command in PowerShell.

```powershell
irm https://topaz.ooo/install.ps1 | iex
```

With Node.js, install from npm.

```sh
npm install -g --allow-scripts=topaz-lang topaz-lang
topaz version
```

## Source and examples

- [Compiler](compiler/crates/topaz_self_frontend/topaz): parsing, type checking and Rust code generation.
- [Lispex application interface](compiler/contracts/lispex-application/v1/std.lispex.tpz): rule values, execution limits, results and evidence.
- [Examples](examples/README.md): programs with run commands and expected output.

## Documentation

[Getting started](https://topaz.ooo/v5.21/en/docs/getting-started) · [Language reference](https://topaz.ooo/v5.21/en/docs/language/types) · [Standard library](https://topaz.ooo/v5.21/en/docs/stdlib/overview)

## License

The Topaz source is available under Apache-2.0. See [LICENSE](LICENSE), [LICENSE-COMPONENTS.txt](LICENSE-COMPONENTS.txt), [NOTICE](NOTICE) and the third-party notices included here. Official executables carry the accompanying executable and component terms.
