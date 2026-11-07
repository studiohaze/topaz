# Topaz Core Kit

Core Kit puts a practical CLI written in Topaz around the public Topaz
compiler. This repository contains its source, runtime and reconstruction
inputs. Build it from this checkout with Rust and Cargo, then try it on
one of the bundled projects.

The language documentation and complete Topaz distribution live at
[topaz.ooo](https://topaz.ooo).

[Get started](#get-started) · [Use the CLI](#use-the-cli) ·
[Build from source](#build-the-kit-from-source) · [Verification](#choose-how-much-to-verify) ·
[Roadmap](#roadmap)

## A note from Matt

Hi, I'm Matt Park, the creator of Topaz. It's a general-purpose language with
typesetting built in.

Until now, we've put self-hosting and stability first. The compiler is written
in Topaz and reaches a fixed point. A compiler built from its own generated
Rust generates that same Rust again, byte for byte, and you can rebuild it
from the public seed and compare the result yourself. Performance had to wait.
Values share a runtime representation, and even calls to known functions
allocate on the heap. The compiler doesn't work on typed data yet. These
problems require structural changes.

For [5.22](https://topaz.ooo/en/docs/project/roadmap#v5-22-0), we're replacing
the compiler's internals so it can work on typed data. Known types will get
their own Rust layouts, and calls to known functions will become plain Rust
calls. 5.22 is the final release series for the `topaz-5.21` language mode, so
existing programs will build without changes. We'll publish
[benchmarks](https://topaz.ooo/en/docs/project/roadmap#benchmarks) against
TypeScript, Python and Rust with the scripts used to run them.

[6.0.0](https://topaz.ooo/en/docs/project/roadmap#v6-0-0) will build on that
engine and rework the language. For the first time, we're writing each rule
into the specification before implementing it. Every rule gets a rationale and
examples that run against the compiler. We're rebuilding the web documentation
around the same specification and giving it a new form.

The principle is "one way to say it." Where Topaz has two ways to express the
same thing, we'll keep one. The aim is a clearer, more consistent
specification. These changes will break existing code, and we'll release a
migration tool with 6.0.0.

Where that principle leaves a choice open, typesetting comes first. I still
want a broad general-purpose language, so features can belong even when
typesetting has no use for them. Typesetting demands determinism. The same
manuscript has to produce the same pages on every machine, down to the last
line break. Any field that needs results it can reproduce and check asks for
the same discipline, so I believe typesetting and general-purpose use belong
in one language.

STUDIO HAZE develops Topaz and is where it gets used. Outside adoption is
close to zero. If even one person anywhere in the world needs a language like
this, I'll keep working to make Topaz ready for them.

Matt Park

## Why Core Kit

The website brings together the language documentation, the full CLI and the
public bootstrap seed. The full CLI includes the Rust reference compiler and
Lispex evaluation, which have their own source and licensing policies. The
seed is enough to reconstruct the public compiler, but it offers little for
everyday work.

Core Kit puts a usable CLI around the same canonical compiler core. It leaves
out the Rust reference compiler and the private Lispex evaluator. Commands,
project configuration, builds and payload preparation are written in Topaz, so
the CLI is also code you can read and learn from. The public native runtime and
a thin Rust adapter connect it to the operating system.

The source, seed inputs, runtime and build instructions live in Git, along with
the generated Rust of the canonical core, so starting from Rust and Cargo needs
no existing Topaz executable. Executables stay out of Git, and platform builds
go on GitHub Releases.

### Choosing a starting point

| Path | What it offers | Use it for |
| --- | --- | --- |
| [Website and full CLI](https://topaz.ooo) | Language documentation and the complete website distribution, including the Rust reference compiler and Lispex evaluation | Learning Topaz and using the full toolchain's features |
| Public bootstrap seed | Focused inputs and a reconstruction path for the canonical compiler | Rebuilding the compiler and comparing its generated core |
| This Core Kit repository | The same public canonical core, a Topaz CLI, local reconstruction inputs and optional platform downloads | Using, inspecting and developing the public compiler |

Core Kit includes its own seed inputs. You can follow this README without
downloading the website's seed archive or installing the website CLI.

## Roadmap

The [roadmap](https://topaz.ooo/en/docs/project/roadmap) covers every
release in the 5.21 line from 5.21.0, and the plans for 5.22.0 and 6.0.0.

| Version | Status | What it brings |
| --- | --- | --- |
| [5.21.0](https://topaz.ooo/en/docs/project/roadmap#v5-21-0) | Released | The `topaz-5.21` language mode, `std.typeset` and Length literals, and embedded Lispex 1.21 |
| [5.21.1](https://topaz.ooo/en/docs/project/roadmap#v5-21-1) | Released | Repairs to type inference, calls, diagnostics and runtime boundaries |
| [5.21.2](https://topaz.ooo/en/docs/project/roadmap#v5-21-2) | Released | Faster everyday commands and signed build records |
| [5.21.3](https://topaz.ooo/en/docs/project/roadmap#v5-21-3) | **Current release** | Signature checks in the npm installer, signed public source and release files without build paths |
| [5.22.0](https://topaz.ooo/en/docs/project/roadmap#v5-22-0) | Upcoming | A new compiler engine with typed compilation, Rust layouts for known types and direct calls, an [official executable anyone can rebuild](https://topaz.ooo/en/docs/project/roadmap#v5-22-0-rebuildable-cli) from public source, plus [benchmarks](https://topaz.ooo/en/docs/project/roadmap#benchmarks) for every release since 5.21.0 |
| [6.0.0](https://topaz.ooo/en/docs/project/roadmap#v6-0-0) | Planned | One way to say it throughout the language, a specification written before the compiler, a new reference and a migration tool |
| [Long-term initiative](https://topaz.ooo/en/docs/project/roadmap#zocle) | Not scheduled | Lighter storage for local records and collections |

5.22.0 replaces the inside of the compiler written in Topaz. The compiler
works on typed data, gives known types their own Rust layouts and calls
functions directly. Both the compiler and the programs it builds get faster.
5.22.0 keeps the `topaz-5.21` language mode, so existing programs build
without changes.

Building Core Kit twice from the same source, in separate checkouts, produces
the same bytes, so anyone can check a download by building it themselves.
5.22.0 brings this idea from the public repository back to the official
executable. The official `topaz` becomes the Core Kit base with add-ons.

5.22.0 will publish benchmark results for every release since 5.21.0, side
by side with TypeScript, Python and Rust running the same programs under the
same conditions. The programs, the scripts that run them and the measurement
method will be in the public repository before 5.22.0.

Patch releases 5.22.x may follow 5.22.0. The line after 5.22 is 6.0.0, not
5.23. It starts the `topaz-6.0` language mode, built on the 5.22 engine, and
comes with a migration guide and a migration tool.

## Topaz and Lispex

Lispex is a safe language for business rules. The full Topaz CLI includes
the Lispex evaluator, so Topaz apps can load rules written in Lispex and run
them without giving them access to files or the network. Lispex has its own
releases, published at
[studiohaze/lispex](https://github.com/studiohaze/lispex).

With Topaz 5.22, the public parts of Lispex move to that repository. That
covers the language specification, the embedding contract Topaz uses and a
conformance suite.

## Get started

To read the CLI, start with [its entry point](core-kit/cli/src/main.tpz).
It parses a command and passes it to the [command handlers](core-kit/cli/src/commands.tpz).
To run the code, build the Kit below.

## Build the Kit from source

Install Rust and Cargo 1.96.0 and your platform's native linker and SDK.
The bootstrap uses the compiler, runtime and seed inputs in this checkout.
It runs offline once the toolchain is installed.

```sh
git clone https://github.com/studiohaze/topaz.git
cd topaz
```

### Linux and macOS

```sh
./bootstrap
./out/core-kit-bootstrap/topaz-core-kit --version
./out/core-kit-bootstrap/topaz-core-kit check --source examples/core-kit/hello.tpz
```

### Windows

Run these commands from Command Prompt or PowerShell. Clone into a short
folder such as `C:\src\topaz`. Paths inside the checkout get long, and a
tested installation failed when the installed executable's path reached 285
characters.

```powershell
.\bootstrap.cmd
.\out\core-kit-bootstrap\topaz-core-kit.exe --version
.\out\core-kit-bootstrap\topaz-core-kit.exe check --source examples\core-kit\hello.tpz
```

Bootstrap builds the public compiler and runtime, compiles the Topaz CLI and
links the executable you just ran. The first native build can take substantial
time and memory. Keep `compiler/target` for subsequent builds.

To rebuild after editing the CLI, run `kit build` with the bootstrap executable.
The new Kit is written under `out/core-kit-build`. The
[build guide](core-kit/BUILD.md) explains the two build paths and the optional
compiler self-regeneration check.

### What installation selects

The [download catalogue](core-kit/downloads.data) lists the release archives
for this checkout. When it lists an archive for your platform, `install-core-kit` or
`install-core-kit.cmd` verifies and installs those bytes. The root `topaz`
and `topaz.cmd` launchers select that version. For the source build above,
run the executable under `out/core-kit-bootstrap` directly.

The [download guide](dist/README.md) covers offline installation from a
matching archive and switching between versions.

## Use the CLI

Pass these commands to the Kit you built. Each path in the examples starts
at the root of the checkout.

| Command | What it does |
| --- | --- |
| `help` / `--version` | Show available commands or the Kit and compiler versions |
| `check --root <directory>` | Check a project using its `topaz.toml` and module inputs |
| `build --root <directory> --out-dir <directory>` | Build a project as a native application |
| `check --source <file>` | Check a single source file with the canonical core |
| `emit-rust --source <file> --out <file>` | Write the core's generated Rust |
| `request --source <file> --out <file>` | Save a complete compiler input snapshot |
| `check --request <file>` | Check a saved compiler input snapshot |
| `build --source <file> --out-dir <directory>` | Build a single-file native application |
| `hash <file>` | Print the file's SHA-256 |
| `kit binary-info --file <file> [--other <file>]` | Inspect a native image or compare two images without running them |
| `kit build` | Rebuild Core Kit from the local sources |
| `kit verify` | Check the recorded source and bootstrap input inventory |
| `kit install --check` | Verify the complete installed payload against the pinned archive and current checkout inputs |
| `kit refresh --check` | Check the current CLI, its callable contract and recorded inputs without changing files |
| `kit refresh` | Update CLI source mappings, the initial snapshot and verification inventories together |

For a project, `--root` selects the directory containing `topaz.toml`. Core Kit
reads the project configuration and resolves its modules and local dependencies
before submitting them to the canonical compiler. Use `--source` for a small
standalone program, or `--request` when you want the compiler to consume a
saved, complete set of inputs. `emit-rust` and `build` also accept saved
requests. The [command guide](core-kit/cli/README.md) describes each mode.

With the native build prerequisites installed, try the bundled hello sample.

```sh
./out/core-kit-bootstrap/topaz-core-kit emit-rust --source examples/core-kit/hello.tpz --out out/hello.rs
./out/core-kit-bootstrap/topaz-core-kit build --source examples/core-kit/hello.tpz --out-dir out/hello-native
```

On Windows, pass the same options to the bootstrap executable.

```powershell
.\out\core-kit-bootstrap\topaz-core-kit.exe emit-rust --source examples\core-kit\hello.tpz --out out\hello.rs
.\out\core-kit-bootstrap\topaz-core-kit.exe build --source examples\core-kit\hello.tpz --out-dir out\hello-native
```

Paths are relative to your working directory. The commands create the output
directories they need and report the files they produce. Native builds use an
empty output directory or one owned by an earlier Core Kit build and refuse
to overwrite an unrelated project. The build reports the executable path so
you can run the program when ready.

For distribution work, `kit payload-assemble` prepares a candidate from a
completed build and a repository recipe. `kit payload-compare` compares two
complete candidates. `kit payload-promote` stages approved bytes in a new
output directory, and `kit payload-pack` prepares the archive and download
record for publication. The separate `kit payload-record` and
`kit payload-verify` commands check file inventories. The
[payload guide](core-kit/PAYLOADS.md) describes the release workflow.

## Inside the checkout

| Location | Role |
| --- | --- |
| [`compiler/crates/topaz_self_frontend/topaz/`](compiler/crates/topaz_self_frontend/topaz) | Canonical compiler source, written in Topaz |
| [`compiler/generated/native/`](compiler/generated/native) | Canonical generated Rust and its identity manifest |
| [`compiler/seed/initial/`](compiler/seed/initial) | Compiler reconstruction inputs, source copies, std modules and provenance |
| [`core-kit/cli/`](core-kit/cli) | Topaz commands, project and build policy, initial CLI inputs and native-program templates |
| [`compiler/crates/`](compiler/crates) and [`core-kit/native/`](core-kit/native) | Public runtime, seed runner and the necessary native adapter and bootstrap generator |
| [`core-kit/distribution/`](core-kit/distribution) | Platform recipes, component records and required notice inputs |
| [`core-kit/downloads.data`](core-kit/downloads.data) | Pinned release assets and their source-input identities |
| `out/core-kit-installed/` | Downloaded local installations, excluded from Git |
| [`examples/README.md`](examples/README.md) | Core Kit programs, project inputs and language samples |

The files in `compiler/seed/initial/topaz/` are exact copies of the canonical
compiler source. The seed keeps its own copy so it can reconstruct the
compiler without reaching into the rest of the checkout.

The compiler core and the surrounding CLI have separate jobs. Core Kit calls
the public core's existing compile interface, while the Topaz CLI handles
commands and project policy. The generated Rust, public native runtime and
adapter are all available in the source tree. A failed or unsupported operation
returns an error without switching to the Rust reference compiler or a private
evaluator.

## Choose how much to verify

You can use the installed tool, inspect its recorded inputs, regenerate the
compiler or reproduce the complete distribution. Each check answers a
different question.

| Check | What a successful result establishes |
| --- | --- |
| `kit verify` | The recorded compiler, source and bootstrap input hashes match this checkout |
| `kit build-id` | SHA-256 of the generated CLI Rust linked into the running Kit |
| `kit refresh --check` | The current CLI passes type and callable checks and agrees with its recorded source mappings and inventories |
| `kit payload-verify --payload-dir <directory>` | The payload files and declared repository inputs match the manifest, including checks for extra or missing files |
| `kit payload-compare --payload-dir <first> --other <second>` | Two complete assembled candidates have the same files, bytes, manifests and required modes |
| `./bootstrap --verify-core` | Regenerated canonical compiler Rust matches the checked-in canonical Rust byte for byte |
| Independent payload reproduction | A fresh build under the recorded target environment produces the same executable and accompanying payload files |

Check the source and bootstrap inputs with the Kit you built.

```sh
./out/core-kit-bootstrap/topaz-core-kit kit verify
```

```powershell
.\out\core-kit-bootstrap\topaz-core-kit.exe kit verify
```

To inspect those inputs without running a Kit executable, use
`sh core-kit/verify-core.sh` on POSIX or `core-kit/verify-core.ps1` in an
existing PowerShell script environment.

Run the longer canonical compiler comparison when you want to reconstruct
the core from its Topaz sources.

```sh
./bootstrap --verify-core
```

```powershell
.\bootstrap.cmd --verify-core
```

The canonical core is the part shared with the public seed. The complete
Core Kit executable also includes the CLI, runtime and adapter, so its
reproduction covers more than the core comparison alone.

Each published payload has a record of its inputs, native toolchain and build
environment. Follow the [reproduction guide](core-kit/REPRODUCIBILITY.md) to
rebuild and compare the complete payload for that target. The
[validation records](core-kit/VALIDATION.md) identify the revisions,
environments and comparisons behind those results. A file inventory checks
consistency without doing a rebuild. A candidate comparison establishes equal
outputs. Keep the separate build records that show how each candidate was
constructed when using that comparison as evidence of reproduction.

These checks are optional for ordinary use. Changes to the CLI or packaging
around unchanged compiler inputs do not require repeating the expensive
canonical self-regeneration.

### Checking the website release

A workflow in this repository checks a Linux x86_64 release from the
website. It verifies the signed checksum lists, checks that the compiler Rust
inside the website's seed matches `compiler/generated/native/compiler.rs`,
then attests the release's files. [VERIFY.md](VERIFY.md) lists the files for
each release and shows how to check the attestation with `gh`. The
attestation covers those bytes. It doesn't claim that the website CLI was
built from this checkout.

## Working on Core Kit

Start with the [Topaz CLI modules](core-kit/cli/src). Command handling,
project processing, build decisions and verification belong there. Keep the
Rust adapter focused on the necessary runtime and operating-system
connections. Public Topaz code should be easy to follow, with diagnostics
that help people fix their programs.

After changing CLI sources, use `kit refresh` to keep source mappings, the
initial snapshot and verification inventories together. It discovers added
and removed modules, checks the current CLI and preserves the protected
compiler inputs. Use `kit refresh --check` when you only want a read-only
report. The [maintenance guide](core-kit/MAINTENANCE.md) explains the retained
backups and recovery record.

```sh
./out/core-kit-bootstrap/topaz-core-kit kit refresh
./out/core-kit-bootstrap/topaz-core-kit kit build
```

```powershell
.\out\core-kit-bootstrap\topaz-core-kit.exe kit refresh
.\out\core-kit-bootstrap\topaz-core-kit.exe kit build
```

Keep the refreshed records with the source change in Git. Canonical compiler
updates have their own source, seed and provenance work, described in the
[build guide](core-kit/BUILD.md).

After a local build, run the smoke checks, including the native sample and
payload inventory checks.

```sh
./core-kit/smoke.sh out/core-kit-build/topaz-core-kit --native-build
```

```powershell
.\core-kit\smoke.cmd .\out\core-kit-build\topaz-core-kit.exe --native-build
```

These commands check the Kit built above. Omit the executable argument to
check the ordinary bootstrap output, or supply another local executable.

Retain the logs and Cargo cache when investigating a failure. The
[local validation guide](core-kit/LOCAL-VALIDATION.md) explains what to report.

### Prepare a platform payload

Choose a checked-in recipe under `core-kit/distribution/` for the target and
build environment being reproduced. Its file list includes the component
records, notices and platform runtime files. The recipe path is relative to
the Core Kit checkout.

| Command | Purpose |
| --- | --- |
| `kit payload-assemble --build-dir <build> --recipe <recipe> --out-dir <new-directory>` | Check the build inputs and native image, then copy the executable, generated CLI and required companions into a new candidate directory |
| `kit payload-compare --payload-dir <first> --other <second>` | Compare complete candidates from the recorded builds |
| `kit payload-promote --payload-dir <first> --other <second> --out-dir <new-directory>` | Run native candidate checks and stage the matching bytes unchanged |
| `kit payload-pack --payload-dir <first> --other <second> --out-dir <new-directory> --repository <owner/name> --tag <tag> --source-commit <commit>` | Approve the native pair and prepare its release archive and download record |

Assembly keeps `assembly.json` and `payload.json` beside the candidate. They
identify its recipe, inputs, file roles and byte inventory. The target's
assembly record also carries its platform requirements. Use `kit binary-info`
to inspect an executable or narrow down a difference before repeating a build.

Promotion requires a recipe approved for release and execution on the
candidate's native target. It preserves the compared manifest and executable,
copies the executable last and refuses an existing destination. Packaging
prepares a named release asset and a matching catalogue record. Publish the
verified archive through GitHub Releases and keep the source, small catalogue
and build evidence connected. These commands do not upload assets, create a
release or make a Git commit. Executable archives and local installations stay
outside Git history.

- [Command reference](core-kit/cli/README.md)
- [Build sequence and source maintenance](core-kit/BUILD.md)
- [CLI snapshot and inventory maintenance](core-kit/MAINTENANCE.md)
- [Independent reproduction](core-kit/REPRODUCIBILITY.md)
- [Payload recording and publication](core-kit/PAYLOADS.md)
- [Validation records](core-kit/VALIDATION.md)
- [Selected upstream port and provenance](core-kit/UPSTREAM-PORT.md)

## Licenses and component scope

The component-specific terms and notices are recorded in [LICENSE](LICENSE),
[LICENSE-COMPONENTS.txt](LICENSE-COMPONENTS.txt), [NOTICE](NOTICE),
[SOURCE-COMPONENTS.json](SOURCE-COMPONENTS.json) and the accompanying third-party
notices. The original source snapshot inventory is retained as provenance.
Topaz Core Kit files and the `topaz_host_native` crate are licensed under
Apache-2.0 as listed in the
[Core Kit notice scope](core-kit/distribution/NOTICE-SCOPE.md).
The [port record](core-kit/UPSTREAM-PORT.md) identifies the selected public
runtime scope. Published platform payloads include the component records and
notices that apply to their actual files.
