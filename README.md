# Topaz

Topaz is a general-purpose programming language with typesetting built in. Code that does the same job looks the same no matter who writes it.

[Website](https://topaz.ooo) · [Roadmap](https://topaz.ooo/en/roadmap) · [Download 5.21.3](https://topaz.ooo/en/download) · [Built with Topaz](https://topaz.ooo/en/built-with-topaz)

## Two lines of Topaz

| Version | Status | What it is |
| --- | --- | --- |
| 5.21.3 | **Stable** | The release you can install and use today. This repository holds its Core Kit source. |
| 6.0 | **Spec-first redesign** | A new compiler engine and a language that its specification defines first. It is validated in ATLAS before its public release. |

## Where 6.0 is going

Topaz 6.0 starts from the specification. Every rule in the new language reference says why it exists and links to examples that run on the release before it ships. Behavior the compiler does not follow yet is marked as a known limitation until it does.

The new engine works on typed data, gives known types their own Rust layouts and calls known functions directly. Anyone will be able to rebuild the official base executable from public source and compare it byte for byte with the release. Where Topaz has two ways to say the same thing, 6.0 keeps one, and a migration tool moves existing programs across in one pass.

## Built for ATLAS first

ATLAS is the flagship program of STUDIO HAZE. Its line-breaking rules are already written in Topaz, and its typesetting engine is planned to run on Topaz 6.0. So 6.0 is applied and validated in ATLAS before it reaches anyone else.

The release conditions on the [roadmap](https://topaz.ooo/en/roadmap) include these three. ATLAS runs on 6.0. It produces the same pages as the current Rust engine from the same manuscripts. Its benchmark results, measured alongside TypeScript, Python and Rust under the same conditions, are published. The [roadmap](https://topaz.ooo/en/roadmap) shows the progress toward each of these goals, generated from the checks that run on every change.

## A note from Matt

Hi, I'm Matt Park, the creator of Topaz.

Until now, we've put self-hosting and stability first. The compiler is written in Topaz and reaches a fixed point. A compiler built from its own generated Rust generates that same Rust again, byte for byte, and you can rebuild it from the public seed and compare the result yourself. Performance had to wait, and fixing it required structural changes. That is what 6.0 is for.

Typesetting demands determinism. The same manuscript has to produce the same pages on every machine, down to the last line break. Any field that needs results it can reproduce and check asks for the same discipline, so I believe typesetting and general-purpose use belong in one language. Where a choice is open, typesetting comes first, but features can belong even when typesetting has no use for them.

Every product we build with Topaz will move to Topaz 6. If even one person anywhere in the world needs a language like this, I'll keep working to make Topaz ready for them.

Matt Park

## Use Topaz 5.21.3 today

Install the stable release from the [download page](https://topaz.ooo/en/download) and start with the [Topaz 5 manual](https://topaz.ooo/v5.21/en/docs/introduction). To check a release you downloaded, follow [VERIFY.md](VERIFY.md).

This repository holds Topaz Core Kit 5.21.3, including the public compiler and a CLI written in Topaz. To build it from source, install Rust and Cargo 1.96.0 and run the bootstrap. On Windows, clone into a short folder such as `C:\src\topaz`, because long paths inside the checkout can make installation fail.

```sh
git clone https://github.com/studiohaze/topaz.git
cd topaz
./bootstrap            # Windows: .\bootstrap.cmd
```

The [build guide](core-kit/BUILD.md) covers installation, the CLI commands, verification and platform payloads.

## Topaz and Lispex

[Lispex](https://github.com/studiohaze/lispex) is a safe language for business rules. Topaz apps can load rules written in Lispex and run them without giving them access to files or the network. In Topaz 6, Lispex runs as a separate WebAssembly module inside a public host.

## License

Topaz Core Kit files are released under the Apache License 2.0. The component terms and notices are in [LICENSE](LICENSE), [LICENSE-COMPONENTS.txt](LICENSE-COMPONENTS.txt), [NOTICE](NOTICE) and [SOURCE-COMPONENTS.json](SOURCE-COMPONENTS.json).
