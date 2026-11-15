# Topaz release benchmarks

This is the portable program, runner, and method bundle for the
[6.0.0 benchmark commitment](https://topaz.ooo/en/docs/project/roadmap#benchmarks).
It needs no compiler source and no development compiler, only official releases.
The results of the release comparison described below are published with 6.0.0.
The execution-time baseline of 5.21.3 is published in the next section.

The full comparison includes every stable official release from 5.21.0 onward,
with TypeScript, Python, and Rust using the same workload sources and inputs.
Topaz's checked interpreter and native release executable are separate lanes.
The suite deliberately combines computation and ordinary application work;
it does not calculate an overall language ranking.

## 5.21.3 baseline

These are microbenchmarks of the code generator. Each program runs for well under a second and the time includes process start, so they show where generated code stands beside hand-written Rust, Python and Node, not how a large application performs.

This baseline is not the release comparison described below. The official 5.21.3 executable cannot yet be rebuilt byte for byte from the public source, so the full method reports it as unavailable. The baseline measures the downloaded official executable, identified by its published hash, and times execution only.

The baseline times the native executable. The interpreter route, `topaz run`, is measured in the release comparison.

| Program | Topaz 5.21.3 native | Rust | Python | TypeScript on Node |
| --- | ---: | ---: | ---: | ---: |
| `fibonacci` | 64.8 (1.3) | 22.1 (0.3) | 62.0 (1.4) | 74.4 (3.6) |
| `sieve` | 45.7 (1.5) | 22.2 (0.8) | 69.7 (4.5) | 76.5 (2.2) |
| `matrix` | 67.4 (1.0) | 22.4 (1.0) | 157.3 (5.6) | 83.5 (3.1) |
| `records` | 79.1 (1.0) | 24.7 (0.7) | 90.7 (2.7) | 80.9 (4.7) |
| `json` | 53.8 (1.0) | 30.4 (0.4) | 66.7 (0.8) | 78.3 (3.1) |
| `empty` | 21.9 (0.7) | 21.7 (0.2) | 42.0 (0.6) | 70.3 (2.8) |

`empty` is process start alone.

Each value is the median in milliseconds, with the interquartile range in parentheses.

### Conditions

- Intel Core i3-10100F CPU with 4 physical cores and 8 logical processors
- 24 GB of RAM
- Windows 11, build 26200
- The official Topaz 5.21.3 executable `topaz-windows-x86_64.exe`, downloaded from its GitHub release, SHA-256 `81111a53ac5da2ebc403438922b42fc1dfdd68782c6324fed4219297b7a7e445`
- Rust 1.96.0 for the host `x86_64-pc-windows-gnu`, the toolchain Topaz 5.21.3 pins for its native builds
- Python 3.12.10
- Node 24.18.0 with TypeScript 5.9.2
- 3 warmups and 15 measured runs for every cell
- Measured on 2026-11-15 (UTC)
- A desktop session was active. Whole-machine CPU use averaged 7 percent over the 30 seconds before the run and 14 percent over the 30 seconds after it.

Run this from the repository root, after filling `toolchains.local.json` as described in Machine-local configuration. The baseline needs only the `artifact` fields of the 5.21.3 entry.

```sh
python benchmarks/bench.py --config benchmarks/toolchains.local.json --baseline --releases 5.21.3 --languages topaz,typescript,python,rust
```

### Known limits

- Process start is a large share of every figure. The `empty` row shows that share.
- In 5.21 every function is emitted as an asynchronous Rust function, so each recursive call makes one heap allocation. The `fibonacci` row shows it. This is on the list for the 6.0 engine.

## Programs and selection

Each `programs/<name>/` directory contains one implementation per language and
`expected.txt`. The JSON program also contains `input.json`. Input constants
and correctness fixtures are program specifications, not performance results.

| Program | Algorithm and input | What it exercises and why it is included |
| --- | --- | --- |
| `empty` | Empty source; Rust has its required empty `main` | Process startup, loader/runtime initialization, and Topaz's normal checking/interpreter entry cost, without output or application work. |
| `fibonacci` | Naive recursive Fibonacci of 26, without memoization | Recursive function calls and branching. This deliberately keeps recursive overhead visible. |
| `sieve` | Sieve of Eratosthenes through 100000, inclusive | Iteration, Boolean arrays, indexed mutation, and integer arithmetic. Reports both prime count and sum. |
| `matrix` | Eight passes of dense 40 by 40 integer matrix multiplication, in row/column/inner-index order | Numeric computation, arrays, indexing, and allocation. Integer values stay within exact ranges in all four languages, so correctness needs no floating-point tolerance. |
| `records` | Six text rows repeated 2000 times; split/trim, construct records, filter/map arrays, group in a Map, sort keys | Common application transformations and allocation, including language standard-library costs. |
| `json` | Twenty passes reading the same UTF-8 JSON file, parsing its complete tree, inspecting fields, counting Unicode scalars, aggregating, and serializing an integer array | File I/O, JSON, strings, records, arrays, and output. The shared fixture includes escapes, surrogate pairs, booleans, null, and nested data. |

All implementations use their language's standard library, ordinary data
structures, and the same algorithm and input. There is no SIMD, unsafe code,
FFI, external JSON package, precomputed answer, special backend, or
language-specific fast path. Standard compiler optimizations remain enabled
for release native programs. Each language handles its usual errors and
boilerplate in its own source.

Rust's standard library has no JSON codec. Its JSON source therefore includes
an ordinary recursive-descent parser for the complete input tree, including
Unicode escape handling and the JSON numeric grammar. The entire parser counts
toward Rust's source lines and runs inside the measured process. The other
languages use their standard JSON codecs. This is a visible consequence of the
standard-library-only requirement; JSON results describe the complete program
and its library support, not a standalone parser contest. Serialization uses an
integer array to avoid differences in object-key order.

### Correctness

A full run first builds each applicable lane and checks it against the shared
`expected.txt` without recording measurements. Every subsequent sample must
also exit successfully and produce that output. Comparison folds CRLF to LF
for the standard libraries' platform text-output conventions; it performs no
other trimming, sorting, tolerance, or value normalization. Raw output and its
SHA-256 remain in the private run. The same unchanged sources and fixtures are
used for every Topaz patch. A failing patch receives a visible failure, not a
patch-specific rewrite or a replacement compiler.

## Official release eligibility

`--releases all` paginates the public repository's release inventory and selects
stable `vMAJOR.MINOR.PATCH` tags from 5.21.0 onward. It requires 5.21.0 to remain
present. Explicit versions are checked against their public release records.
Missing configuration or unavailable releases remain in the run's inventory
and make the overall full run fail. New releases are discovered automatically;
the local manifest must then supply their exact acquisition and rebuild inputs.

For every **measured** Topaz executable in the full comparison, the adapter:

1. Downloads the exact versioned official artifact, checks its archive hash,
   extracts the real executable, and checks its executable hash.
2. Checks `topaz version` and the exact `topaz-lang@VERSION` npm metadata.
3. Downloads a public source archive pinned to a full immutable commit and
   checks its hash. It extracts a fresh source tree, rejecting links and path
   traversal, and verifies the public build recipe's hash.
4. Executes that reviewed recipe, outside measurement, in the fresh tree. The
   declared rebuilt executable must not already exist in the archive.
5. Compares the complete rebuilt executable with the complete official
   executable byte for byte. A hash alone, a generated-Rust match, an
   attestation, or an operator-supplied Boolean is insufficient.
6. Runs the downloaded official executable in the comparison; the rebuilt copy
   establishes its source relationship and is not substituted into the lane.

The public recipe must actually rebuild the CLI from the pinned public source,
with its dependencies and build tools pinned and available. Review it before
executing it, just as with any downloaded build script. A recipe that copies a
release binary does not establish source reproducibility. The adapter records
the recipe, source, command, environment, interpreter, logs, and both executable
identities so others can inspect and repeat that relationship.

### Current distribution distinction

The [public release verifier](https://github.com/studiohaze/topaz/blob/1e7085458a134b9329175ed776365e3b3bd15cd9/VERIFY.md)
explicitly does not establish that the website/npm CLI rebuilds from the public
repository. The [Core Kit reproducibility method](https://github.com/studiohaze/topaz/blob/1e7085458a134b9329175ed776365e3b3bd15cd9/core-kit/REPRODUCIBILITY.md)
applies to the separate Core Kit product. Its command surface does not supply
the full CLI's `topaz run` and `topaz build --release` comparison lanes.

Consequently, this bundle does not invent complete official-CLI rebuild recipes
for 5.21.x. The template leaves those inputs unset. Supply a reviewed public
recipe when one exists; otherwise the corresponding full benchmark is
`unavailable`. A Core Kit installation is rejected as a substitute. Official
release tags and signed checksums remain useful identity inputs, but do not
close the source-rebuild gap by themselves.

The npm installation route is `topaz-lang@VERSION`. Its installer obtains the
native CLI from the versioned `https://topaz.ooo/dl/vVERSION/` release route
(which can redirect to GitHub Releases). The adapter downloads that native
artifact directly and records both routes. It does not time the npm JavaScript
shim, which would add a Node process. Core Kit acquisition is separately
documented in the public repository's `core-kit/downloads.data`; those artifacts
are not entered as full CLI measurements.

## Running the suite

### Required tools

- Python 3.11 or later for the OS adapter, using the standard library only.
- An installed official Topaz CLI to run `harness.tpz`. When a selected release
  passes acquisition, its executable can also serve as the driver. The driver
  is identified separately and is outside all measured intervals.
- Node and a complete TypeScript package for the TypeScript lane. Pass the real
  `typescript/lib/tsc.js`; no `npx` or package download is performed inside a
  sample. No `@types/node` package is needed.
- The selected Python executable for the Python lane.
- Actual Rust and Cargo binaries, plus rustup for Topaz native lanes. Use paths
  returned by `rustup which`, rather than Rust/Cargo proxy symlinks. Preinstall
  each official release's pinned toolchain; the runner does not install one.
- The interpreter and native linker/SDK required by the reviewed public rebuild
  recipe. Prepare dependencies outside the timed trials.

The OS adapter supports Linux, macOS, and Windows. Full memory measurements use
kernel high-water counters. A missing counter causes an explicit failure rather
than a sampled approximation or an invented zero. Windows uses only Python's
`ctypes` to access process and Job Object APIs; no additional Python package is
required. Use one physical machine, OS, power plan, and toolchain setup for one
comparison. Do not combine different platforms' raw memory counters into a
single comparison.

### Machine-local configuration

Copy `toolchains.template.json` to the ignored `toolchains.local.json`. Fill each
selected tool's absolute `path` and exact `version`, including the patch. Paths
may contain spaces. The adapter passes argument arrays without a shell. Record
power mode, CPU governor, affinity, virtualization, background load, and thermal
conditions in `host_notes`. Configuration and all generated records stay
private; do not paste them into public discussions.

Each release entry has `version`, `channel: "npm"`, `artifact`, and `source`.
The template deliberately contains no pretend artifact hashes or measurement
values. Populate these fields from the exact official release and its reviewed
public rebuild instructions:

| Object | Required fields and meaning |
| --- | --- |
| `artifact` | `url`: exact versioned official route; `sha256`: archive or download hash; `format`: `raw`, `gzip`, `tar.gz`, or `zip`; `member`: executable archive member for tar/zip; `executable_sha256`: complete unpacked executable hash. |
| `source` | `url`: `https://github.com/studiohaze/topaz/archive/COMMIT.tar.gz`; `sha256`: archive hash; `commit`: full immutable commit; `recipe`: relative public script path; `recipe_sha256`: script hash. |
| `source` build fields | `interpreter`: `bash`, `pwsh`, or `python`; `interpreter_path`: absolute executable; `arguments`: argument array; `executable`: relative fresh output path under the extracted repository; `source_date_epoch`: the recipe's decimal timestamp as a string; `environment`: exact additional string settings required by the recipe. |
| Optional `cargo_home` | Existing dependency-source cache for commands that honor it. It is outside timed clean-build outputs. Topaz's official native builder may construct its own vendored Cargo home instead. |

The recipe cannot override the adapter's target-directory isolation, `PATH`, or
home directories through its environment map. If a release's reproducible build
requires a different environment, configure the host and public recipe to
match its published method; do not relax executable equality.

### Full comparison

From the repository root:

```sh
python benchmarks/bench.py --config benchmarks/toolchains.local.json --releases all --languages topaz,typescript,python,rust
```

To select exact releases and programs, use comma-separated arguments:

```sh
python benchmarks/bench.py --config benchmarks/toolchains.local.json --releases 5.21.0,5.21.1,5.21.2,5.21.3 --languages topaz,typescript,python,rust --programs fibonacci,sieve,matrix,records,json,empty
```

`--repetitions`, `--warmups`, `--timeout`, and `--rebuild-timeout` make the trial
policy explicit in the raw record. The defaults are 15 measured repetitions and
3 warmups per applicable case and metric. Timeouts mark failed observations;
they do not remove a slow result from a successful distribution.

### Windows correctness smoke

From PowerShell at the repository root, after configuring the official artifact
and the required tool paths:

```powershell
py -3 benchmarks/bench.py --config benchmarks/toolchains.local.json --quick --releases 5.21.3 --languages topaz --programs fibonacci
```

This selects one program and one release, prepares its native product once,
and executes each selected lane once with no warmups. It does not run the clean
and edited build trial series or silently add the empty startup program. Add
`typescript,python,rust` to `--languages` to check those implementations in the
same invocation.

The smoke retains correctness statuses and provenance, **no elapsed-time,
memory, or source-line measurements**. It checks the official download identity
but does not claim public-source rebuild eligibility. Thus a Windows machine can
check the script and official program behavior while the source-rebuild gap is
still unresolved. A smoke pass cannot be used as a benchmark result or a
substitute for the full eligibility checks.

### Execution-time baseline

```sh
python benchmarks/bench.py --config benchmarks/toolchains.local.json --baseline --releases 5.21.3 --languages topaz,typescript,python,rust
```

The baseline checks each selected official executable as the smoke does (exact
versioned download route, archive and executable hashes, `topaz version`, and
npm metadata). It does not run the public-source rebuild, and its release
records carry `eligibility: "official_download_identity_only"`. Only the full
comparison can record `verified_public_source_same_bytes`. `--baseline` and
`--quick` cannot be combined.

It times the empty program's startup and the other programs' execution for
every selected lane, with the full comparison's warmups, repetitions, lane
rotation, output checks, and statistics. Topaz takes part through its native
executable only. The Topaz run lane belongs to the full comparison and the
smoke. The baseline runs no build trials and collects
no memory or source-line measurements. Its `raw.json` records
`method: "baseline"`, and `results.json` and `results.md` are labelled as a
baseline, so the run cannot be read as the release comparison.

### Output and failures

Each invocation creates a distinct ignored `results/<run>/` with `raw.json`,
`results.json`, `results.md`, and private command logs. Scratch products and
download caches live in ignored `.work/` and `.cache/`. The console prints a
status and the output directory, not measurements. Raw observations are written
after each sample, so an interruption leaves its completed observations visible.
Start a new run for a complete distribution; do not merge partial runs.

Exit status is zero for complete success, one for failed/unavailable cases, and
two for setup or reporting failure. Setup failures still produce JSON and a
status table without manufactured statistics. A failed warmup, incorrect output,
duplicate iteration, or incomplete measured series prevents aggregation for that
case/metric. Unavailable values are `null` with a status/reason, never zero.
Missing required startup/execution memory also makes a full run fail, while
retaining valid timing observations with an explicit unavailable memory status.
An edit sample is invalid unless its paired clean build succeeded.

## Measurement method

### Execution routes and build commands

| Lane | Invocation and preparation |
| --- | --- |
| Topaz run | Exact official native CLI, `topaz run main.tpz`; includes its checking and interpreter startup on every invocation. |
| Topaz native | `topaz build main.tpz --release --out-dir product`, then execute `product/target/release/program[.exe]` directly. |
| TypeScript | Identified Node plus `tsc.js main.ts --target ES2022 --module commonjs --strict --incremental --tsBuildInfoFile product/build.tsbuildinfo --outDir product --pretty false`; execute emitted `main.js` with that Node. A private CommonJS package boundary makes relocation independent of a parent package's module type. |
| Python | Identified interpreter with `-I -B main.py`; no bytecode cache or separate build phase. Its normal isolated-runtime hash randomization remains enabled. |
| Rust | Identified `rustc --edition=2024 -C opt-level=3 -C debuginfo=0 -o product/program[.exe] main.rs`; execute the resulting binary directly. No optional incremental compilation is enabled. |

For each Topaz release, an untimed `topaz emit` of the selected program exposes
its embedded `rust-toolchain.toml`. The adapter resolves that exact channel with
the recorded rustup executable and hashes the actual Rust/Cargo binaries. They
must match the configured common Rust toolchain. This matters because the
official native builder resolves its own pin and does not accept inherited
`RUSTC` or Cargo overrides as a toolchain selection. A mismatch is an explicit
unavailable release, not an unrecorded toolchain change.

### Timed intervals and memory

- **Startup:** wall time from immediately before creating the empty program's
  process until that process exits. Compilation/preparation happens outside this
  interval. Topaz run retains its normal checking/interpreter entry work.
- **Execution:** the same whole-process interval for a nonempty workload. It
  includes loading, parsing/JIT where applicable, computation, file I/O, and
  output. The empty-program time is not subtracted. This does not claim an
  in-process steady-state kernel measurement.
- **Peak memory:** the OS high-water resident-memory counter for startup and
  execution. Linux `wait4` reports KiB, converted to bytes; macOS reports bytes.
  POSIX's counter is the maximum for the process and any waited descendants,
  not a sum of simultaneous process memory. Windows reports the root process's
  `PeakWorkingSetSize` in bytes. Workload lanes execute their binaries directly,
  without npm/shell wrappers. The exact scope is recorded per observation.
  Build-process memory is intentionally not aggregated with workload memory.
  On Windows, the root counter omits Topaz run's isolated compiler workers;
  that lane therefore reports unavailable process-tree RSS, not a misleading
  root-only value or committed-memory substitute. Other native workload lanes
  use their root-process counter. The correctness smoke and the baseline collect
  no memory.
- **Clean first build:** delete the case's entire working directory, recreate
  identical source and input, and start with no application products, build
  information, case-owned compiler target, or case-owned Topaz storage. Time the
  build command through its exit. Copying files, dependency acquisition,
  verifying output, and hashing artifacts happen outside the interval.
- **One-file rebuild:** retain the preceding successful clean build's directory,
  replace the sole literal `bench-v1:` with `bench-v2:` in its entry source, and
  time the same build command. Verify the new prefix afterward. Algorithm and
  input remain unchanged, while a stale executable cannot pass. No source file
  is touched by this edit except the entry file.

The empty workload has startup measurements and untimed compilation preparation,
but no build trials. Python and Topaz run have no separate build metric; their
entries are `not_applicable`. Do not represent those entries as zero-duration
builds or substitute syntax-check time.

Retaining products does not promise that a compiler has incremental reuse.
TypeScript retains its normal build-info cache. Rust recompiles with its normal
optimized single-file command. The inspected official Topaz native builder
creates a fresh internal workspace on each build, uses one Cargo build job and
disables Cargo incremental compilation. Both Topaz build timings include that
behavior. Each case receives private `TOPAZ_STORAGE_DIR`, `XDG_RUNTIME_DIR`,
temporary, and cache paths; clean trials remove those paths too. The adapter
does not modify compiler or runtime implementations to improve build results.

Downloaded tools, OS page caches, and dependency source caches are not flushed
between trials. Clean describes application/compiler build outputs, not a cold
machine or fresh toolchain installation. File-read trials use the naturally
warmed OS cache after preflight. All languages receive the same fixture bytes,
working-directory layout, empty stdin, output capture mechanism, machine state,
and serial execution policy. Background activity, power management, and thermal
changes must be controlled and recorded by the operator.

### Repetitions, ordering, and statistics

The Topaz harness schedules warmups and measured rounds across individual lanes,
rotating the first lane and reversing traversal direction on alternate rounds.
It never runs an entire language/version's measurements as one batch. Execution
rounds precede build rounds. Within each build round, all clean builds alternate
across lanes, then all paired edited builds alternate across lanes. Builds and
programs never run concurrently. The complete job order is retained in JSON.

Warmups use fresh processes and the same commands; they warm filesystem/runtime
infrastructure rather than preserve a Node JIT process across repetitions.
Discard warmups from statistics, retain all measured repetitions, and remove no
outliers. Report the median, first quartile, third quartile, and interquartile
range separately for each program, language, version, route, and metric.
For sorted observations, quantile `p` linearly interpolates at index
`p * (n - 1)`; IQR is Q3 minus Q1. Nanosecond and byte units remain explicit.
The Topaz code uses exact integer quarter-units for these quantiles, then emits
numeric JSON values, avoiding the 5.21 serializer's restriction on native floats.

Source lines are counted once per unchanged entry file. A line counts when it
contains any non-whitespace character. This includes comments, imports, type
declarations, braces, error handling, and Rust's JSON parser. Blank lines,
fixtures, expected output, generated products, and the benchmark tools themselves
are excluded. The same rule applies to all four languages. No measured source
line totals are checked into this repository.

## Recorded identities and conditions

The private JSON includes machine CPU/model, logical and physical cores, RAM,
architecture, OS/kernel, available affinity information, clock implementation,
operator notes, sanitized environment, selection, schedule, and every raw sample.
Unavailable machine facts are named explicitly. It also includes:

- Exact Node, TypeScript, Python, Rust, Cargo, rustup, driver, and official Topaz
  versions, their version command output, absolute paths, and executable SHA-256.
- The TypeScript package's files and hashes, so a `tsc.js` loader alone is not
  mistaken for the complete compiler identity.
- Release inventory records, exact npm and artifact acquisition routes, hashes,
  public source commit and recipe, independent rebuilt executable identity, and
  byte-comparison outcome. Core Kit cannot be mislabeled as the official CLI.
- Workload, fixture, harness, and configuration hashes; exact command arrays and
  working directories; generated JavaScript/native executable hashes and raw
  output hashes; trial status, timeout, exit code, and memory-counter scope.

## Implementation boundary and verification limits

New scheduling, ordering, source-line counting, statistics, and report generation are written in Topaz
in `harness.tpz`. `bench.py` and `platform_process.py` contain the OS boundary:
downloads/archive extraction, filesystem workspace operations, process control,
monotonic timing, kernel memory counters, tool discovery, and executable hashes.
The documented 5.21 standard library does not expose a portable subprocess API
or kernel per-process high-water memory counters. Python's standard library
provides those boundaries on Windows and POSIX without third-party dependencies.
The small setup-failure report also lives there so a missing Topaz driver can
still leave a useful machine-readable record; it computes no statistics.

Current documentation describes the 5.21 APIs used here, but that is not an
execution claim for every patch. In particular, 5.21.2 synchronized public/Core
Kit `std.fs`, `std.io`, and `std.json` sources with the official CLI; 5.21.0 and
5.21.1 namespace and checker behavior must be checked on their exact binaries.
The programs use dynamic JSON accessors instead of imported nominal
`json.parseAs<T>` decoding. The harness still requires each patch's documented
JSONValue methods, structural JSON serialization, filesystem permissions, and
CLI argument contract. Its driver is versioned separately from measured lanes.
Never infer those properties from a different patch's success.

Run the OS-boundary behavior checks with:

```sh
python -m unittest discover -s benchmarks/tests
```

Record actual commands and pass/fail/setup outcomes in the PR, without timings,
memory measurements, source-line totals, or numerical example results. Passing
adapter tests does not claim that an unexecuted Topaz, Rust, Windows, or macOS
path has passed. After validation, keep generated `results/`, `.work/`, `.cache/`,
and the local toolchain manifest out of commits and public reports. The
5.21.3 baseline section publishes only its table and conditions.
