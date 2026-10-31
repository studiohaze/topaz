# Topaz examples

These examples work with the public compiler through Core Kit. Run commands
from the repository root after installing the matching Kit or building one
from source. The [getting-started guide](../README.md#get-started) describes
both paths.

Use `./topaz` on Linux or macOS and `.\topaz.cmd` on Windows for an installed
Kit. For a source build, substitute the executable reported by bootstrap,
such as `./out/core-kit-bootstrap/topaz-core-kit` or
`.\out\core-kit-bootstrap\topaz-core-kit.exe`.

## Start with the Core Kit examples

| Example | Purpose |
| --- | --- |
| [hello.tpz](core-kit/hello.tpz) | A small standalone program for source checking, Rust emission and native building |
| [project/](core-kit/project) | A project with `topaz.toml`, two source modules and the bundled standard library |
| [type-error.tpz](core-kit/type-error.tpz) | An intentionally invalid program for checking failure diagnostics |

Check the standalone program and the project.

```sh
./topaz check --source examples/core-kit/hello.tpz
./topaz check --root examples/core-kit/project
```

```powershell
.\topaz.cmd check --source examples\core-kit\hello.tpz
.\topaz.cmd check --root examples\core-kit\project
```

With the pinned Rust/Cargo toolchain and native linker installed, build either
example as a native application.

```sh
./topaz build --source examples/core-kit/hello.tpz --out-dir out/hello-native
./topaz build --root examples/core-kit/project --out-dir out/project-native
```

```powershell
.\topaz.cmd build --source examples\core-kit\hello.tpz --out-dir out\hello-native
.\topaz.cmd build --root examples\core-kit\project --out-dir out\project-native
```

The build reports the application executable's path. Run that executable to
see the program's output. The build command leaves execution to you. The
[build guide](../core-kit/BUILD.md) explains host prerequisites and the
[project guide](../core-kit/PROJECTS.md) describes module resolution and
filesystem grants.

## Inspect and replay compiler inputs

Generate Rust or save the complete input of a checked project.

```sh
./topaz emit-rust --source examples/core-kit/hello.tpz --out out/hello.rs
./topaz request --root examples/core-kit/project --out out/project.request.json
./topaz check --request out/project.request.json
```

The saved project request retains the compiler's source facts and project
configuration. It can be replayed without reopening the original project
files. See the [command reference](../core-kit/cli/README.md) for request and
native replay details.

The type-error fixture is expected to fail its check and produce a compiler
diagnostic.

```sh
./topaz check --source examples/core-kit/type-error.tpz
```

## Language samples

The language documentation's small source examples and expected-output files
are also checked in. Their source files can be passed to Core Kit's
`check --source` or `build --source` commands.

| Topic | Source | Expected program output |
| --- | --- | --- |
| Variables and output | [first-program.tpz](../docs/samples/learn/first-program.tpz) | [first-program.stdout](../docs/samples/learn/first-program.stdout) |
| Functions and state | [values-functions.tpz](../docs/samples/learn/values-functions.tpz) | [values-functions.stdout](../docs/samples/learn/values-functions.stdout) |
| Records and control flow | [data-control.tpz](../docs/samples/learn/data-control.tpz) | [data-control.stdout](../docs/samples/learn/data-control.stdout) |
| Pipelines | [pipelines-placeholders.tpz](../docs/samples/language/pipelines-placeholders.tpz) | [pipelines-placeholders.stdout](../docs/samples/language/pipelines-placeholders.stdout) |

```sh
./topaz check --source docs/samples/learn/first-program.tpz
./topaz build --source docs/samples/learn/first-program.tpz --out-dir out/first-program
```

The website's full CLI has its own command interface, including `topaz run`.
Core Kit uses the explicit check, build and executable workflow above. The
[language guide](https://topaz.ooo/v5.21/en/docs/getting-started) explains the
language features demonstrated by these sources.
