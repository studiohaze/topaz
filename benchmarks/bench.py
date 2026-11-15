#!/usr/bin/env python3
"""OS adapter for harness.tpz. Results are private, including failed runs."""

from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
import urllib.parse
import urllib.request
import uuid
import zipfile

from platform_process import capture, machine_metadata, measure


ROOT = Path(__file__).resolve().parent
PUBLIC_REPOSITORY = "https://github.com/studiohaze/topaz"
LANGUAGES = ("topaz", "typescript", "python", "rust")
PROGRAMS = ("empty", "fibonacci", "sieve", "matrix", "records", "json")
EXTENSIONS = {"topaz": "tpz", "typescript": "ts", "python": "py", "rust": "rs"}
SEMVER = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class SetupError(Exception):
    pass


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path: Path, value: object) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def required(mapping: dict, key: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value.strip():
        raise SetupError(f"Missing nonempty {key}")
    return value


def version_tuple(version: str) -> tuple[int, ...]:
    if not SEMVER.fullmatch(version):
        raise SetupError(f"Use an exact stable release version: {version}")
    return tuple(map(int, version.split(".")))


def read_url(url: str) -> bytes:
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or parsed.username or parsed.password:
        raise SetupError("Public inputs must use HTTPS without credentials")
    request = urllib.request.Request(url, headers={"User-Agent": "topaz-benchmarks"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def download(spec: dict, cache: Path) -> Path:
    url = required(spec, "url")
    expected = required(spec, "sha256")
    if not SHA256.fullmatch(expected):
        raise SetupError("An exact lowercase SHA-256 is required for every downloaded artifact")
    target = cache / expected
    if target.is_file() and digest(target) == expected:
        return target
    payload = read_url(url)
    if hashlib.sha256(payload).hexdigest() != expected:
        raise SetupError(f"Downloaded bytes do not match the pinned SHA-256: {url}")
    target.write_bytes(payload)
    return target


def safe_relative(value: str) -> PurePosixPath:
    path = PurePosixPath(value)
    if path.is_absolute() or not path.parts or any(p in ("..", ".") for p in path.parts) or "\\" in value or ":" in value:
        raise SetupError("Archive and recipe paths must be relative without traversal")
    return path


def extract_source(archive: Path, destination: Path) -> Path:
    """Extract regular files/directories; never links or device nodes."""
    destination.mkdir()
    with tarfile.open(archive, "r:gz") as bundle:
        for member in bundle:
            relative = safe_relative(member.name.rstrip("/"))
            target = destination.joinpath(*relative.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                source = bundle.extractfile(member)
                if source is None:
                    raise SetupError("Missing archive member")
                with source, target.open("wb") as output:
                    shutil.copyfileobj(source, output)
                target.chmod(member.mode & 0o755)
            else:
                raise SetupError("Source archives containing links or special files are unsupported")
    roots = list(destination.iterdir())
    if len(roots) != 1 or not roots[0].is_dir():
        raise SetupError("Source archive must contain one repository root")
    return roots[0]


def extract_executable(archive: Path, spec: dict, destination: Path) -> None:
    archive_format = required(spec, "format")
    if archive_format == "raw":
        data = archive.read_bytes()
    elif archive_format == "gzip":
        with gzip.open(archive, "rb") as stream:
            data = stream.read()
    elif archive_format in ("tar.gz", "zip"):
        member = str(safe_relative(required(spec, "member")))
        if archive_format == "zip":
            with zipfile.ZipFile(archive) as bundle:
                data = bundle.read(member)
        else:
            with tarfile.open(archive, "r:gz") as bundle:
                item = bundle.getmember(member)
                if not item.isfile():
                    raise SetupError("Executable member is not a regular file")
                stream = bundle.extractfile(item)
                if stream is None:
                    raise SetupError("Executable member is missing")
                with stream:
                    data = stream.read()
    else:
        raise SetupError(f"Unsupported artifact format: {archive_format}")
    expected = required(spec, "executable_sha256")
    if not SHA256.fullmatch(expected) or hashlib.sha256(data).hexdigest() != expected:
        raise SetupError("Unpacked official executable SHA-256 does not match")
    destination.write_bytes(data)
    destination.chmod(0o755)


def official_releases(selection: str) -> tuple[list[str], list[dict]]:
    observed = []
    if selection != "all":
        versions = selection.split(",")
        if len(set(versions)) != len(versions):
            raise SetupError("Duplicate releases are not allowed")
        for version in versions:
            if version_tuple(version) < (5, 21, 0):
                raise SetupError("The release series starts at 5.21.0")
            url = f"https://api.github.com/repos/studiohaze/topaz/releases/tags/v{version}"
            data = json.loads(read_url(url))
            if data.get("draft") or data.get("prerelease") or data.get("tag_name") != "v" + version:
                raise SetupError(f"Not an official stable release: {version}")
            observed.append({"version": version, "url": url, "published_at": data.get("published_at")})
        return sorted(versions, key=version_tuple), observed
    versions = []
    page = 1
    while True:
        url = f"https://api.github.com/repos/studiohaze/topaz/releases?per_page=100&page={page}"
        batch = json.loads(read_url(url))
        if not isinstance(batch, list):
            raise SetupError("Public release discovery did not return a list")
        for release in batch:
            tag = release.get("tag_name", "")
            version = tag[1:] if tag.startswith("v") else ""
            if SEMVER.fullmatch(version) and version_tuple(version) >= (5, 21, 0) and not release.get("draft") and not release.get("prerelease"):
                versions.append(version)
                observed.append({"version": version, "url": release["html_url"], "published_at": release.get("published_at")})
        if len(batch) < 100:
            break
        page += 1
    if "5.21.0" not in versions:
        raise SetupError("The public release inventory does not contain the required 5.21.0 release")
    return sorted(set(versions), key=version_tuple), observed


def command(argv: list[str], cwd: Path, env: dict[str, str], timeout: float, logs: Path, name: str) -> bytes:
    try:
        result = capture(argv, cwd, env, timeout, logs)
    except (OSError, RuntimeError) as error:
        raise SetupError(f"Command could not complete: {argv[0]} ({type(error).__name__})") from error
    (logs / f"{name}.stdout").write_bytes(result["stdout"])
    (logs / f"{name}.stderr").write_bytes(result["stderr"])
    if result["timed_out"] or result["exit_code"] != 0:
        raise SetupError(f"Command failed or timed out; inspect private logs/{name}.stderr")
    return result["stdout"]


def executable_identity(path: Path) -> dict:
    return {"path": str(path), "sha256": digest(path), "size_bytes": path.stat().st_size}


def inspect_tool(name: str, spec: dict, argv_suffix: list[str], env: dict[str, str], logs: Path, prefix: list[str] | None = None) -> dict:
    path = Path(required(spec, "path")).expanduser().resolve(strict=True)
    version = required(spec, "version")
    if not re.fullmatch(r"v?[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?", version):
        raise SetupError(f"{name} needs an exact version, including its patch")
    argv = [*(prefix or []), str(path), *argv_suffix]
    output = command(argv, ROOT, env, 60, logs, "version-" + name).decode("utf-8", errors="strict").strip()
    if not re.search(r"(?<![0-9.])" + re.escape(version) + r"(?![0-9.])", output):
        raise SetupError(f"{name} does not report configured version {version}")
    identity = {**executable_identity(path), "version": version, "version_output": output, "version_argv": argv}
    if name == "typescript":
        package_root = path.parent.parent
        package = json.loads((package_root / "package.json").read_text(encoding="utf-8"))
        if package.get("name") != "typescript" or package.get("version") != version:
            raise SetupError("TypeScript path must identify the matching package's lib/tsc.js")
        identity["package_files"] = [{"path": str(file.relative_to(package_root)), "sha256": digest(file)} for file in sorted(package_root.rglob("*")) if file.is_file()]
    return identity


def base_environment(config: dict) -> dict[str, str]:
    env = dict(os.environ)
    for name in ("NODE_OPTIONS", "NODE_PATH", "PYTHONPATH", "PYTHONHOME", "PYTHONHASHSEED", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_TARGET_DIR", "TS_NODE_PROJECT", "RUSTUP_TOOLCHAIN", "TOPAZ_STORAGE_DIR", "XDG_RUNTIME_DIR", "TOPAZ_COMPILER_MEMORY_MIB", "TOPAZ_COMPILER_TIME_MS", "TOPAZ_SELF_FRONTEND_METRICS"):
        env.pop(name, None)
    env.update({"TZ": "UTC", "LC_ALL": "C", "LANG": "C", "PYTHONDONTWRITEBYTECODE": "1", "CARGO_BUILD_JOBS": "1", "CARGO_NET_OFFLINE": "true", "RUST_BACKTRACE": "0"})
    cargo_home = config.get("cargo_home")
    if cargo_home:
        env["CARGO_HOME"] = str(Path(cargo_home).expanduser().resolve(strict=True))
    return env


def acquire_release(version: str, spec: dict, cache: Path, work: Path, env: dict[str, str], logs: Path, method: str, timeout: float) -> tuple[Path, dict]:
    if spec.get("channel") != "npm":
        raise SetupError("Core Kit has a different CLI contract and cannot fill official run/native lanes")
    artifact = spec.get("artifact")
    if not isinstance(artifact, dict):
        raise SetupError("Official artifact URL and exact checksums have not been supplied")
    url = required(artifact, "url")
    official_prefixes = (f"https://topaz.ooo/dl/v{version}/", f"https://github.com/studiohaze/topaz/releases/download/v{version}/")
    if not any(url.startswith(prefix) for prefix in official_prefixes):
        raise SetupError("The artifact must use the exact official versioned download route")
    npm_url = f"https://registry.npmjs.org/topaz-lang/{version}"
    npm_bytes = read_url(npm_url)
    npm = json.loads(npm_bytes)
    if npm.get("name") != "topaz-lang" or npm.get("version") != version:
        raise SetupError("npm package identity does not match the requested release")
    artifact_file = download(artifact, cache)
    release_dir = work / ("release-" + version)
    release_dir.mkdir()
    binary = release_dir / ("topaz.exe" if os.name == "nt" else "topaz")
    extract_executable(artifact_file, artifact, binary)
    identity = inspect_tool("topaz-" + version, {"path": str(binary), "version": version}, ["version"], env, logs)
    provenance = {"version": version, "channel": "npm", "package": "topaz-lang@" + version, "npm_metadata_url": npm_url, "npm_metadata_sha256": hashlib.sha256(npm_bytes).hexdigest(), "npm_distribution": npm.get("dist"), "artifact": artifact, "executable": identity}
    # Only the benchmark method continues to the public-source rebuild, so the
    # other methods can never reach verified_public_source_same_bytes.
    if method == "smoke":
        return binary, {**provenance, "eligibility": "not_checked_correctness_smoke"}
    if method == "baseline":
        return binary, {**provenance, "eligibility": "official_download_identity_only"}
    if method != "benchmark":
        raise SetupError(f"Unknown method: {method}")
    source = spec.get("source")
    if not isinstance(source, dict):
        raise SetupError("Public-source official CLI rebuild recipe is missing; artifact checksums do not prove rebuildability")
    commit = required(source, "commit")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise SetupError("A full immutable public source commit is required")
    if source.get("url") != f"{PUBLIC_REPOSITORY}/archive/{commit}.tar.gz":
        raise SetupError("Source archive must pin the exact public repository commit")
    source_archive = download(source, cache)
    source_root = extract_source(source_archive, release_dir / "source")
    recipe_relative = safe_relative(required(source, "recipe"))
    recipe = source_root.joinpath(*recipe_relative.parts)
    if not recipe.is_file() or digest(recipe) != required(source, "recipe_sha256"):
        raise SetupError("The pinned public build recipe does not match the source archive")
    recipe_args = source.get("arguments", [])
    if not isinstance(recipe_args, list) or any(not isinstance(value, str) for value in recipe_args):
        raise SetupError("Recipe arguments must be a list of strings")
    interpreter = required(source, "interpreter")
    if interpreter not in ("bash", "pwsh", "python"):
        raise SetupError("Public recipe interpreter must be bash, pwsh, or python")
    interpreter_path = Path(required(source, "interpreter_path")).expanduser().resolve(strict=True)
    prefix = [str(interpreter_path)]
    if interpreter == "pwsh":
        prefix += ["-NoProfile", "-File"]
    rebuild_env = dict(env)
    rebuild_env["CARGO_TARGET_DIR"] = str(release_dir / "rebuild-target")
    rebuild_env["SOURCE_DATE_EPOCH"] = required(source, "source_date_epoch")
    configured_env = source.get("environment", {})
    if not isinstance(configured_env, dict) or any(not isinstance(k, str) or not isinstance(v, str) for k, v in configured_env.items()):
        raise SetupError("Recipe environment must contain strings")
    if any(name.upper() in ("CARGO_TARGET_DIR", "PATH", "HOME", "USERPROFILE") for name in configured_env):
        raise SetupError("Recipe environment cannot override output isolation or tool lookup")
    rebuild_env.update(configured_env)
    rebuilt = source_root.joinpath(*safe_relative(required(source, "executable")).parts)
    if rebuilt.exists():
        raise SetupError("Declared rebuild output already exists in the source archive; require a fresh output path")
    argv = prefix + [str(recipe)] + recipe_args
    command(argv, source_root, rebuild_env, timeout, logs, "rebuild-" + version)
    if not rebuilt.is_file():
        raise SetupError("The public recipe did not create its declared executable")
    if rebuilt.read_bytes() != binary.read_bytes():
        raise SetupError("Public-source rebuild and official executable differ byte for byte")
    provenance.update({"eligibility": "verified_public_source_same_bytes", "source_repository": PUBLIC_REPOSITORY, "source": source, "recipe_url": f"{PUBLIC_REPOSITORY}/blob/{commit}/{recipe_relative}", "recipe_argv": argv, "recipe_interpreter": executable_identity(interpreter_path), "recipe_environment": {k: rebuild_env[k] for k in ("CARGO_TARGET_DIR", "SOURCE_DATE_EPOCH", *configured_env)}, "rebuilt_executable": executable_identity(rebuilt), "byte_comparison": "equal"})
    return binary, provenance


def native_toolchain(binary: Path, version: str, program: str, work: Path, env: dict, tools: dict, logs: Path, timeout: float) -> dict:
    """Observe the release's embedded pin instead of trusting PATH or RUSTC."""
    probe = work / ("toolchain-" + version)
    source = ROOT / "programs" / program / "main.tpz"
    command([str(binary), "emit", str(source), "--out-dir", str(probe)], ROOT, env, timeout, logs, "emit-toolchain-" + version)
    pin_file = probe / "rust-toolchain.toml"
    with pin_file.open("rb") as stream:
        channel = tomllib.load(stream)["toolchain"]["channel"]
    if not isinstance(channel, str) or not channel:
        raise SetupError("Emitted native toolchain channel is missing")
    observed = {"channel": channel, "emitted_pin_sha256": digest(pin_file), "rustup": tools["rustup"]}
    for name in ("rustc", "cargo"):
        output = command([tools["rustup"]["path"], "which", "--toolchain", channel, name], ROOT, env, timeout, logs, f"resolve-{version}-{name}")
        path = output.decode("utf-8").strip()
        suffix = ["--version", "--verbose"] if name == "rustc" else ["--version"]
        identity = inspect_tool(f"native-{version}-{name}", {"path": path, "version": tools[name]["version"]}, suffix, env, logs)
        if identity["sha256"] != tools[name]["sha256"]:
            raise SetupError("Topaz's embedded native toolchain differs from the configured common Rust toolchain")
        observed[name] = identity
    return observed


def write_workspace(case: dict, work: Path) -> Path:
    directory = work / "cases" / case["id"]
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    source = ROOT / "programs" / case["program"]
    extension = EXTENSIONS[case["language"]]
    shutil.copyfile(source / ("main." + extension), directory / ("main." + extension))
    if (source / "input.json").exists():
        shutil.copyfile(source / "input.json", directory / "input.json")
    (directory / "tmp").mkdir()
    (directory / "runtime").mkdir(mode=0o700)
    (directory / "product").mkdir()
    if case["language"] == "typescript":
        write_json(directory / "product" / "package.json", {"type": "commonjs"})
    return directory


def case_environment(env: dict[str, str], directory: Path, tools: dict) -> dict[str, str]:
    result = dict(env)
    result.update({"CARGO_TARGET_DIR": str(directory / "product" / "target"), "TOPAZ_STORAGE_DIR": str(directory / "topaz-storage"), "XDG_RUNTIME_DIR": str(directory / "runtime"), "TMPDIR": str(directory / "tmp"), "TMP": str(directory / "tmp"), "TEMP": str(directory / "tmp"), "XDG_CACHE_HOME": str(directory / "cache")})
    if "rustc" in tools:
        result["RUSTC"] = tools["rustc"]["path"]
    return result


def commands(case: dict, directory: Path, tools: dict, releases: dict) -> tuple[list[str] | None, list[str]]:
    language = case["language"]
    extension = EXTENSIONS[language]
    source = "main." + extension
    executable = "program.exe" if os.name == "nt" else "program"
    if language == "topaz":
        binary = str(releases[case["release"]])
        if case["mode"] == "run":
            return None, [binary, "run", source]
        return [binary, "build", source, "--release", "--out-dir", "product"], [str(directory / "product" / "target" / "release" / executable)]
    if language == "typescript":
        build = [tools["node"]["path"], tools["typescript"]["path"], source, "--target", "ES2022", "--module", "commonjs", "--strict", "--incremental", "--tsBuildInfoFile", "product/build.tsbuildinfo", "--outDir", "product", "--pretty", "false"]
        return build, [tools["node"]["path"], str(directory / "product" / "main.js")]
    if language == "python":
        return None, [tools["python"]["path"], "-I", "-B", source]
    return [tools["rustc"]["path"], "--edition=2024", "-C", "opt-level=3", "-C", "debuginfo=0", "-o", "product/" + executable, source], [str(directory / "product" / executable)]


def correct_output(case: dict, output: bytes, edited: bool = False) -> bool:
    expected = (ROOT / "programs" / case["program"] / "expected.txt").read_bytes()
    if edited:
        expected = expected.replace(b"bench-v1:", b"bench-v2:", 1)
    # Text-mode standard output on Windows uses CRLF in some standard libraries.
    return output.replace(b"\r\n", b"\n") == expected


def artifact_identities(case: dict, directory: Path, run: list[str]) -> list[dict]:
    paths = [directory / ("main." + EXTENSIONS[case["language"]])]
    if case["mode"] == "native":
        paths.append(Path(run[0]))
    if case["language"] == "typescript":
        paths.append(directory / "product" / "main.js")
    if (directory / "input.json").is_file():
        paths.append(directory / "input.json")
    return [executable_identity(path) for path in paths]


def prepare_case(case: dict, work: Path, env: dict, tools: dict, releases: dict, logs: Path, timeout: float, check: bool) -> dict:
    directory = write_workspace(case, work)
    build, run = commands(case, directory, tools, releases)
    process_env = case_environment(env, directory, tools)
    (directory / "product").mkdir(exist_ok=True)
    if build:
        command(build, directory, process_env, timeout, logs, case["id"] + "-prepare-build")
    if check:
        output = command(run, directory, process_env, timeout, logs, case["id"] + "-preflight")
        if not correct_output(case, output):
            raise SetupError("Preflight output differs from the shared correctness fixture")
    return {"directory": directory, "build": build, "run": run, "env": process_env}


def run_job(job: dict, case: dict, state: dict, work: Path, env: dict, tools: dict, releases: dict, logs: Path, timeout: float, method: str, ordinal: int) -> dict:
    metric = job["metric"]
    edited = metric == "build_edit"
    if metric == "build_clean":
        state["clean_iteration"] = None
        directory = write_workspace(case, work)
        (directory / "product").mkdir(exist_ok=True)
        build, run = commands(case, directory, tools, releases)
        state.update(directory=directory, build=build, run=run, env=case_environment(env, directory, tools))
    directory = state["directory"]
    if edited:
        if state.get("clean_iteration") != job["iteration"]:
            return {**job, "status": "baseline_failed", "reason": "The paired clean build did not complete correctly", "elapsed_ns": None, "peak_rss_bytes": None}
        source = directory / ("main." + EXTENSIONS[case["language"]])
        original = source.read_bytes()
        if original.count(b"bench-v1:") != 1:
            raise SetupError("The one-file edit requires one baseline output marker")
        source.write_bytes(original.replace(b"bench-v1:", b"bench-v2:", 1))
    is_build = metric.startswith("build_")
    argv = state["build"] if is_build else state["run"]
    if argv is None:
        raise SetupError("Build job scheduled for a language without a build command")
    label = f"sample-{ordinal}-{case['id']}-{metric}"
    sample = {**job, "argv": argv, "cwd": str(directory), "status": "ok", "elapsed_ns": None, "peak_rss_bytes": None, "memory_scope": "not_measured"}
    if method == "smoke":
        output = command(argv, directory, state["env"], timeout, logs, label)
    else:
        # The baseline times execution only; memory belongs to the benchmark method.
        missing_tree_memory = method == "benchmark" and os.name == "nt" and case["language"] == "topaz" and case["mode"] == "run" and not is_build
        observed = measure(argv, directory, state["env"], timeout, logs, memory=method == "benchmark" and not is_build and not missing_tree_memory)
        output = observed.pop("stdout")
        errors = observed.pop("stderr")
        (logs / (label + ".stdout")).write_bytes(output)
        (logs / (label + ".stderr")).write_bytes(errors)
        sample.update(observed)
        if missing_tree_memory:
            sample["memory_scope"] = "unavailable-process-tree-rss"
            sample["memory_reason"] = "Windows root peak working set does not include Topaz compiler workers; process-tree RSS is unavailable"
        if observed["timed_out"] or observed["exit_code"] != 0:
            sample["status"] = "timeout" if observed["timed_out"] else "process_failed"
            return sample
    if is_build:
        output = command(state["run"], directory, state["env"], timeout, logs, label + "-verify")
    if not correct_output(case, output, edited=edited):
        sample["status"] = "output_mismatch"
    if metric == "build_clean" and sample["status"] == "ok":
        state["clean_iteration"] = job["iteration"]
    sample["stdout_sha256"] = hashlib.sha256(output).hexdigest()
    sample["artifacts"] = artifact_identities(case, directory, state["run"])
    return sample


def planned_cases(programs: list[str], languages: list[str], versions: list[str], tools: dict, method: str) -> list[dict]:
    """Every program/lane case of one invocation, in schedule input order.

    The baseline measures generated code, so Topaz contributes only its native
    executable. The interpreter route, topaz run, belongs to the release
    comparison and the smoke.
    """
    topaz_modes = ("native",) if method == "baseline" else ("run", "native")
    cases = []
    for program in programs:
        for language in languages:
            variants = [(version, mode) for version in versions for mode in topaz_modes] if language == "topaz" else [(tools[{"typescript": "typescript", "python": "python", "rust": "rustc"}[language]]["version"], "native" if language == "rust" else "interpreted")]
            for version, mode in variants:
                cases.append({"id": f"{program}-{language}-{version}-{mode}", "program": program, "language": language, "release": version, "mode": mode, "build": program != "empty" and (mode == "native" or language == "typescript"), "status": "ready"})
    return cases


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=ROOT / "toolchains.local.json")
    parser.add_argument("--releases", default="all", help="all, or comma-separated exact official versions")
    parser.add_argument("--languages", default=",".join(LANGUAGES))
    parser.add_argument("--programs", default="all")
    methods = parser.add_mutually_exclusive_group()
    methods.add_argument("--quick", action="store_true", help="one program, one release, one correctness pass; no performance measurements")
    methods.add_argument("--baseline", action="store_true", help="execution and startup time of downloaded official executables identified by hash; no public-source rebuild, builds, memory or source lines")
    parser.add_argument("--repetitions", type=int, default=15)
    parser.add_argument("--warmups", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--rebuild-timeout", type=float, default=3600)
    args = parser.parse_args(argv)
    args.method = "smoke" if args.quick else "baseline" if args.baseline else "benchmark"
    args.languages = args.languages.split(",")
    if len(set(args.languages)) != len(args.languages) or any(x not in LANGUAGES for x in args.languages):
        parser.error("--languages must select distinct topaz,typescript,python,rust")
    args.programs = (["fibonacci"] if args.quick else list(PROGRAMS)) if args.programs == "all" else args.programs.split(",")
    if len(set(args.programs)) != len(args.programs) or any(x not in PROGRAMS for x in args.programs):
        parser.error("--programs must select distinct known programs")
    if args.quick:
        if args.releases == "all" or len(args.releases.split(",")) != 1 or len(args.programs) != 1:
            parser.error("--quick requires one explicit --releases version and one program")
        args.repetitions, args.warmups = 1, 0
    if args.repetitions < 1 or args.warmups < 0 or args.timeout <= 0 or args.rebuild_timeout <= 0:
        parser.error("Counts and timeouts must be positive (warmups may be zero)")
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    run_id = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8]
    result_dir = ROOT / "results" / run_id
    work = ROOT / ".work" / run_id
    cache = ROOT / ".cache"
    for directory in (result_dir, work):
        directory.mkdir(parents=True)
    cache.mkdir(parents=True, exist_ok=True)
    logs = result_dir / "logs"
    logs.mkdir()
    raw = {"schema": "topaz-benchmarks-raw/v1", "method": args.method, "repetitions": args.repetitions, "warmups": args.warmups, "started_at": dt.datetime.now(dt.timezone.utc).isoformat(), "machine": machine_metadata(), "command": sys.argv if argv is None else argv, "cases": [], "samples": [], "line_counts": [], "toolchains": {}, "releases": [], "status": "setup"}
    raw_path = result_dir / "raw.json"
    write_json(raw_path, raw)
    try:
        config = json.loads(args.config.read_text(encoding="utf-8"))
        if config.get("schema") != "topaz-benchmarks-toolchains/v1":
            raise SetupError("Unsupported toolchain manifest schema")
        raw["configuration_sha256"] = digest(args.config)
        raw["host_notes"] = config.get("host_notes", "unspecified")
        raw["suite_files"] = [{"path": str(path.relative_to(ROOT)), "sha256": digest(path)} for path in sorted(ROOT.rglob("*")) if path.is_file() and not any(part.startswith(".") or part in ("results", "__pycache__") for part in path.relative_to(ROOT).parts) and path.name != "toolchains.local.json"]
        env = base_environment(config)
        versions, inventory = official_releases(args.releases)
        raw["release_inventory"] = inventory
        raw["selection"] = {"releases": versions, "languages": args.languages, "programs": args.programs}
        specs = {entry["version"]: entry for entry in config.get("releases", [])}
        needed = []
        if "typescript" in args.languages:
            needed += ["node", "typescript"]
        if "python" in args.languages:
            needed += ["python"]
        if "rust" in args.languages or "topaz" in args.languages:
            needed += ["rustc", "cargo"]
        if "topaz" in args.languages:
            needed += ["rustup"]
        tools = raw["toolchains"]
        tool_specs = config.get("tools", {})
        for name in needed:
            suffix = ["--version", "--verbose"] if name == "rustc" else ["--version"]
            prefix = [tools["node"]["path"]] if name == "typescript" else None
            tools[name] = inspect_tool(name, tool_specs.get(name, {}), suffix, env, logs, prefix)
        tool_paths = [str(Path(info["path"]).parent) for name, info in tools.items() if name != "typescript"]
        env["PATH"] = os.pathsep.join(tool_paths + [env.get("PATH", "")])
        raw["environment"] = {name: env.get(name) for name in ("PATH", "TZ", "LC_ALL", "LANG", "CARGO_HOME", "CARGO_BUILD_JOBS", "CARGO_NET_OFFLINE", "PYTHONDONTWRITEBYTECODE", "RUST_BACKTRACE")}
        raw["environment"]["python_hash_seed"] = "runtime_default_isolated"
        releases = {}
        unavailable = {}
        if "topaz" in args.languages:
            for version in versions:
                try:
                    binary, identity = acquire_release(version, specs.get(version, {}), cache, work, env, logs, args.method, args.rebuild_timeout)
                    identity["native_toolchain"] = native_toolchain(binary, version, args.programs[0], work, env, tools, logs, args.timeout)
                    releases[version] = binary
                    raw["releases"].append(identity)
                except (SetupError, OSError, ValueError, KeyError, tarfile.TarError, zipfile.BadZipFile) as error:
                    unavailable[version] = str(error)
                    raw["releases"].append({"version": version, "eligibility": "unavailable", "requested": specs.get(version), "reason": str(error)})
        driver_spec = config.get("driver", {})
        if not driver_spec.get("path") and releases:
            driver_version = sorted(releases, key=version_tuple)[-1]
            driver_spec = {"path": str(releases[driver_version]), "version": driver_version}
        driver = inspect_tool("driver", driver_spec, ["version"], env, logs)
        raw["driver"] = driver
        states = {}
        line_sources = []
        for case in planned_cases(args.programs, args.languages, versions, tools, args.method):
            raw["cases"].append(case)
            if case["language"] == "topaz" and case["release"] in unavailable:
                case.update(status="unavailable", reason=unavailable[case["release"]])
                continue
            try:
                states[case["id"]] = prepare_case(case, work, env, tools, releases, logs, args.timeout, check=args.method != "smoke")
                if args.method == "benchmark":
                    source = ROOT / "programs" / case["program"] / ("main." + EXTENSIONS[case["language"]])
                    line_sources.append({"case_id": case["id"], "path": str(source)})
            except (SetupError, OSError, RuntimeError) as error:
                case.update(status="setup_failed", reason=str(error))
        request = {"method": args.method, "repetitions": args.repetitions, "warmups": args.warmups, "line_sources": line_sources, "cases": [case for case in raw["cases"] if case["status"] == "ready"]}
        request_path, plan_path = work / "request.json", work / "plan.json"
        write_json(request_path, request)
        driver_command = [driver["path"], "run", str(ROOT / "harness.tpz"), "--"]
        command(driver_command + ["plan", str(request_path), str(plan_path)], ROOT, env, args.timeout, logs, "plan")
        plan = json.loads(plan_path.read_text(encoding="utf-8"))
        if plan.get("schema") != "topaz-benchmarks-plan/v1":
            raise SetupError("Unexpected Topaz scheduling protocol")
        raw["plan"] = plan
        raw["line_counts"] = plan["line_counts"]
        raw["status"] = "running"
        write_json(raw_path, raw)
        cases_by_id = {case["id"]: case for case in raw["cases"]}
        for ordinal, job in enumerate(plan["jobs"]):
            case = cases_by_id[job["case_id"]]
            try:
                sample = run_job(job, case, states[case["id"]], work, env, tools, releases, logs, args.timeout, args.method, ordinal)
            except (SetupError, OSError, RuntimeError) as error:
                sample = {**job, "status": "execution_failed", "reason": str(error), "elapsed_ns": None, "peak_rss_bytes": None}
            raw["samples"].append(sample)
            write_json(raw_path, raw)
        failed = any(case["status"] != "ready" for case in raw["cases"]) or any(sample["status"] != "ok" for sample in raw["samples"])
        if args.method == "benchmark":
            failed = failed or any(sample["metric"] in ("runtime", "startup") and sample.get("peak_rss_bytes") is None for sample in raw["samples"])
        raw["status"] = "failed" if failed else "passed"
        raw["finished_at"] = dt.datetime.now(dt.timezone.utc).isoformat()
        write_json(raw_path, raw)
        command(driver_command + ["report", str(raw_path), str(result_dir / "results.json"), str(result_dir / "results.md")], ROOT, env, args.timeout, logs, "report")
        print(f"{raw['status']}: {'correctness smoke' if args.method == 'smoke' else args.method}; private files: {result_dir}")
        return 1 if failed else 0
    except (SetupError, OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        raw.update(status="setup_failed", reason=str(error))
        write_json(raw_path, raw)
        write_json(result_dir / "results.json", {"schema": "topaz-benchmarks/v1", "kind": args.method, "raw": raw, "summaries": []})
        (result_dir / "results.md").write_text("# Setup failed\n\nNo performance result was produced. Inspect `raw.json` and the private logs.\n", encoding="utf-8")
        print(f"setup_failed: {error}; private files: {result_dir}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
