# Core Kit notice scope

This bundle preserves the repository's existing license and notice texts.
This record also states the license of the Topaz Core Kit files.
COMPONENTS.json describes the selected Core Kit composition, its license
authorizations and the policy record for each component.

## Core Kit license

Studio Haze Inc. licenses the following Topaz Core Kit files under the
Apache License, Version 2.0, the same terms as the existing public source.
The full license text is in LICENSE. This license was authorized on
2026-10-31.

- `core-kit/**`, including the Topaz CLI sources, the PowerShell and sh
  scripts and the Rust adapter in `core-kit/native`
- `compiler/core-kit-native/**`
- `compiler/seed/**`
- `compiler/crates/topaz_host_native/**`, the native host crate ported from
  the development source
- `compiler/crates/topaz_seed_compiler/**`
- `compiler/generated/native/*`
- `compiler/Cargo.toml`, `compiler/Cargo.lock`, `compiler/rust-toolchain.toml`
  and `compiler/README.md`
- `examples/core-kit/**`
- `dist/README.md`
- the root `bootstrap*`, `install-core-kit*` and `topaz*` launchers and the
  root `.gitignore`

The six public seed runtime crates and the standalone seed runner were
licensed under Apache-2.0 on 2026-10-26. These are `topaz_diag`,
`topaz_syntax`, `topaz_value`, `topaz_product_runtime`, `topaz_rt`,
`topaz_host_none` and `topaz_seed` under `compiler/crates`.
`compiler/seed/runtime.json` records that authorization.

The root LICENSE-COMPONENTS.txt points to this record. Third-party material
keeps the terms granted by its copyright holders. The notices below still
apply to a built executable.

## Public compiler and runtime

The canonical compiler source and generated core are identified by
SOURCE-COMPONENTS.json and the compiler's generated-source manifest. NOTICE
contains the full Unicode notice for the compiler's derived Unicode tables.

SEED-RUNTIME.json preserves the recorded authorization for the six public
seed runtime crates and the standalone seed runner. UPSTREAM-PORT.md explains
the selected public port, the separate native host boundary and the local
Core Kit additions. The standalone runner belongs to reconstruction and is
not linked into the native Core Kit executable.

LICENSE, LICENSE-COMPONENTS.txt and BINARY-LICENSE.txt retain their original
terms and scopes. This bundle does not replace those texts. Files that are
neither in a snapshot inventory nor in the Core Kit license above gain no
license from this record.

## Inherited website notices

THIRD-PARTY-NOTICES.md and THIRD-PARTY-LICENSES.txt describe the broader
website distribution. Their original texts are retained here. They are not
presented as the minimal dependency graph of this executable.

The current Core Kit Cargo lockfile contains local workspace packages and
no registry packages. COMPONENTS.json lists the nine packages in the native
executable's dependency closure. Rust's standard library and platform runtime
dependencies are accounted for separately.

## Rust standard library

RUST-STDLIB-NOTICES.html is the unmodified standard-library notice supplied
with Rust 1.96.0. It includes the notice's embedded license texts. The copied
file's identity, source archive and exact member path are recorded in
RUST-STDLIB-PROVENANCE.json.

This upstream notice describes Rust library source and dependencies across
targets. Including it does not assert that every component it lists is linked
into this particular executable. It also does not replace the notices of
separately bundled platform runtime files.

## Platform and generated application scope

The selected platform recipe lists required companion files and classifies
observed system dependencies. Its platform description and readiness policy
need to agree with the retained native validation and build evidence.

The repository policy refers to GENERATED-OUTPUT-NOTICE.txt for generated
application output. That file is absent from this checkout. This scope record
does not invent replacement terms or claim that a new notice has been supplied.
Application output notice work remains recorded in COMPONENTS.json.

Payload byte comparison establishes matching supplied files. It does not
independently authenticate a publisher or decide component licensing, platform
support or the independence of the builds.
