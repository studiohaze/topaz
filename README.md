# Topaz

Topaz is a programming language for typesetting. The compiler is written in Topaz and emits Rust.

This repository contains the compiler source and examples for Topaz 5.21.1.

> ### A quick note from the author
>
> The release bot has been doing all the talking here, so I thought I'd leave a note of my own.
>
> Topaz already has a website for people who just want to use it. The CLI, docs, and bootstrap seeds will stay at [topaz.ooo](https://topaz.ooo). I don't want to make that page any busier.
>
> I'd like to turn this repo into a small Topaz Core Kit someday. The compiler is already written in Topaz, and I'd like to write the CLI in Topaz too. We'll need a bit of handwritten Rust to make it all work, of course.
>
> Just source code here. No generated compiler Rust or prebuilt binaries. Anyone who wants to build it will need the matching bootstrap seed from the website.
>
> There's nothing to try yet. For now, it's still a source snapshot.
>
> I just wanted to leave this here before the bot comes back.

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
