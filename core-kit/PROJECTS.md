# Projects and saved compiler inputs

Project mode reads `topaz.toml` and gives the canonical compiler access to the
project's source files through its existing fact-provider interface. The
compiler parses imports and discovers the module graph. The CLI supplies
configuration, file contents, directory entries and physical path information.

## A local project

The repository includes a small project with two source modules.

```sh
./topaz check --root examples/core-kit/project
./topaz emit-rust --root examples/core-kit/project --out out/project.rs
./topaz request --root examples/core-kit/project --out out/project.request.json
./topaz build --root examples/core-kit/project --out-dir out/project-native
```

Use `.\topaz.cmd` with the same options on Windows. A newly built development
Kit accepts these commands through its executable path as well.

A project manifest declares its entry explicitly.

```toml
[package]
name = "example"
version = "0.1.0"
language = "5.21"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.21"
```

All ordinary module imports start at the selected project root. For example,
`import src.greeting { greeting }` loads `src/greeting.tpz`. Imports can span
any number of modules within the admitted source limits. The CLI does not
parse imports or rewrite their names.

This mode accepts one package, its local source modules and the bundled `std`
dependency. Other dependency declarations require a separate package resolver
and are rejected explicitly. Extern evaluation and the website's private
components are not used. Unknown manifest fields also fail so configuration
cannot be silently ignored.

Manifest paths use forward slashes and must stay relative to the project.
Empty segments, parent traversal and absolute paths are rejected. The native
adapter inspects the physical location of requested files and directories.
Links that leave the selected root are refused. Directory facts preserve the
original names so the compiler can diagnose case and Unicode spelling
collisions using the same rules as its other inputs.

Alias identities use canonical paths relative to the project root. Symlink
aliases therefore share an identity, while distinct hard-link paths remain
separate names. Inspection followed by a read is not atomic against concurrent
directory replacement.

Standard modules come from the checkout's explicit standard-module inventory.
A missing standard import cannot be satisfied by placing a replacement under
an unrelated project-local `std` directory.

## Filesystem grants for native programs

A project can declare read and write access separately.

```toml
[capabilities.fs]
read = ["data"]
write = ["results"]
```

The emitted native program uses the public runtime's filesystem capability
host. Missing read or write arrays grant no access for that operation. A
project with no filesystem declaration has empty read and write grants.
The special path `"."` grants that operation within the runtime base.

These paths are relative to the application's working directory when it is
launched. The generated program does not embed the build machine's absolute
project path. Launch it from the directory containing the data and output
locations described by its configuration. The grants apply to the public
filesystem operations performed by that program.

Single-file `--source` mode has no project manifest and retains its ordinary
process filesystem access. Use a project manifest when the application needs
explicit filesystem grants.

## Save and replay a project request

`request --root` lets the canonical compiler discover the required sources
and records the returned facts with the project configuration. It writes a
`topaz.core-kit.project-request/v1` envelope containing the complete public
compiler request and the filesystem grants from the manifest.

```sh
./topaz request --root examples/core-kit/project --out out/project.request.json
./topaz check --request out/project.request.json
./topaz emit-rust --request out/project.request.json --out out/replayed-project.rs
./topaz build --request out/project.request.json --out-dir out/replayed-project
```

The snapshot uses logical paths and a deterministic fact order. It does not
store the project's absolute directory. Replay uses the captured sources and
configuration, so edits to the original project cannot silently change the
saved request. The source project is not required for replay. A missing fact
fails instead of falling back to the ambient filesystem.

The raw `topaz.compiler.stage1-exchange/v2` format remains accepted for
single-file and explicitly assembled compiler requests. A raw request has no
project filesystem configuration. Preserve the project envelope when you
want a native rebuild to retain its declared grants.

## Resource boundaries

A manifest is limited to 1 MiB. A project can supply at most 4,096 source
facts, with source text and bundled standard modules together limited to
16 MiB. One source file is also limited to 16 MiB. Source reads use the public
whole-file API, so the byte limit is checked after the read.

The provider admits at most 16,384 queried facts and 65,536 entries in a
requested directory. It reads source files only when the compiler requests
them. Unrelated source trees and build directories are not recursively copied
into the request.

A saved request is limited to 128 MiB and goes through the same compiler
identity, fact and resource-budget checks as other closed requests. Its saved
configuration is checked again before native build policy uses it.
