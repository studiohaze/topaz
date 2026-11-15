"""Adapter behavior checks using temporary inputs and mocked network boundaries.

The miniature build copies a Python source to its product path. It exercises
clean/edit orchestration and observable output without substituting for a
language compiler or producing benchmark measurements.
"""

import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import bench


class AdapterTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.suite = self.root / "suite"
        self.program = self.suite / "programs" / "probe"
        self.program.mkdir(parents=True)
        self.original = b'print("bench-v1:checked")\n'
        (self.program / "main.py").write_bytes(self.original)
        (self.program / "expected.txt").write_bytes(b"bench-v1:checked\n")
        (self.program / "input.json").write_bytes(b"[]\n")
        self.work = self.root / "work"
        self.logs = self.root / "logs"
        self.cache = self.root / "cache"
        for directory in (self.work, self.logs, self.cache):
            directory.mkdir()
        root_patch = patch.object(bench, "ROOT", self.suite)
        root_patch.start()
        self.addCleanup(root_patch.stop)
        self.case = {
            "id": "probe-python", "program": "probe", "language": "python",
            "mode": "interpreted", "build": True,
        }
        self.env = bench.base_environment({})

    def fixture_commands(self, case, directory, tools, releases):
        copy = "import shutil; shutil.copyfile('main.py', 'product/program.py')"
        return [sys.executable, "-c", copy], [sys.executable, str(directory / "product/program.py")]

    def run_job(self, metric, state, *, iteration=0):
        job = {"case_id": self.case["id"], "metric": metric,
               "iteration": iteration, "warmup": False}
        return bench.run_job(job, self.case, state, self.work, self.env,
                             {}, {}, self.logs, 10, "smoke", 0)

    def archive(self, path, files=None, link=None):
        with tarfile.open(path, "w:gz") as bundle:
            for name, payload in (files or {}).items():
                member = tarfile.TarInfo(name)
                member.size = len(payload)
                member.mode = 0o644
                bundle.addfile(member, io.BytesIO(payload))
            if link is not None:
                member = tarfile.TarInfo("source/link")
                member.type = link
                member.linkname = "../escaped"
                bundle.addfile(member)

    def test_clean_then_edit_changes_real_output_without_collecting_metrics(self):
        state = {}
        with patch.object(bench, "commands", side_effect=self.fixture_commands), \
                patch.object(bench, "measure", side_effect=AssertionError("quick mode must not measure")):
            clean = self.run_job("build_clean", state)
            edited = self.run_job("build_edit", state)
        self.assertEqual(clean["status"], "ok")
        self.assertEqual(edited["status"], "ok")
        for sample in (clean, edited):
            self.assertIsNone(sample["elapsed_ns"])
            self.assertIsNone(sample["peak_rss_bytes"])
        output = self.logs / "sample-0-probe-python-build_edit-verify.stdout"
        self.assertEqual(output.read_bytes().replace(b"\r\n", b"\n"), b"bench-v2:checked\n")
        self.assertEqual((self.program / "main.py").read_bytes(), self.original)
        self.assertEqual((state["directory"] / "input.json").read_bytes(), b"[]\n")

    def test_failed_clean_build_invalidates_a_previous_success_and_blocks_edit(self):
        state = {"clean_iteration": 0}
        with patch.object(bench, "commands", side_effect=self.fixture_commands), \
                patch.object(bench, "command", side_effect=bench.SetupError("build failed")):
            with self.assertRaises(bench.SetupError):
                self.run_job("build_clean", state)
        with patch.object(bench, "command") as launch, patch.object(bench, "measure") as measure:
            edited = self.run_job("build_edit", state)
        self.assertEqual(edited["status"], "baseline_failed")
        launch.assert_not_called()
        measure.assert_not_called()
        self.assertEqual((state["directory"] / "main.py").read_bytes(), self.original)

    def test_incorrect_clean_output_cannot_authorize_edit(self):
        state = {}
        with patch.object(bench, "commands", side_effect=self.fixture_commands), \
                patch.object(bench, "command", side_effect=[b"", b"wrong\n"]):
            clean = self.run_job("build_clean", state)
        self.assertEqual(clean["status"], "output_mismatch")
        with patch.object(bench, "command") as launch:
            edited = self.run_job("build_edit", state)
        self.assertEqual(edited["status"], "baseline_failed")
        launch.assert_not_called()

    def test_edit_requires_the_same_clean_iteration(self):
        directory = bench.write_workspace(self.case, self.work)
        state = {"directory": directory, "clean_iteration": 0}
        with patch.object(bench, "command") as launch:
            result = self.run_job("build_edit", state, iteration=1)
        self.assertEqual(result["status"], "baseline_failed")
        launch.assert_not_called()

    def test_windows_topaz_run_does_not_substitute_root_memory_for_workers(self):
        (self.program / "main.tpz").write_text('import std.io\nio.writeLine("bench-v1:checked")\n', encoding="utf-8")
        case = {**self.case, "id": "probe-topaz", "language": "topaz", "mode": "run", "build": False}
        directory = bench.write_workspace(case, self.work)
        state = {"directory": directory, "run": ["topaz", "run", "main.tpz"],
                 "build": None, "env": self.env}
        job = {"case_id": case["id"], "metric": "runtime", "iteration": 0, "warmup": False}
        observation = {"elapsed_ns": None, "peak_rss_bytes": None,
                       "memory_scope": "not-measured", "timed_out": False,
                       "exit_code": 0, "stdout": b"bench-v1:checked\n", "stderr": b""}
        with patch.object(bench, "os", SimpleNamespace(name="nt")), \
                patch.object(bench, "measure", return_value=observation) as measure:
            sample = bench.run_job(job, case, state, self.work, self.env,
                                   {}, {}, self.logs, 10, "benchmark", 0)
        self.assertFalse(measure.call_args.kwargs["memory"])
        self.assertEqual(sample["status"], "ok")
        self.assertIsNone(sample["peak_rss_bytes"])
        self.assertEqual(sample["memory_scope"], "unavailable-process-tree-rss")
        self.assertTrue(sample["memory_reason"])
        self.assertIn("elapsed_ns", sample)

    def test_baseline_times_execution_without_collecting_memory(self):
        directory = bench.write_workspace(self.case, self.work)
        state = {"directory": directory, "build": None, "env": self.env,
                 "run": [sys.executable, str(directory / "main.py")]}
        job = {"case_id": self.case["id"], "metric": "runtime", "iteration": 0, "warmup": False}
        observation = {"elapsed_ns": 1000, "peak_rss_bytes": None,
                       "memory_scope": "not-measured", "timed_out": False,
                       "exit_code": 0, "stdout": b"bench-v1:checked\n", "stderr": b""}
        with patch.object(bench, "measure", return_value=observation) as measure:
            sample = bench.run_job(job, self.case, state, self.work, self.env,
                                   {}, {}, self.logs, 10, "baseline", 0)
        self.assertFalse(measure.call_args.kwargs["memory"])
        self.assertEqual(sample["status"], "ok")
        self.assertEqual(sample["elapsed_ns"], 1000)
        self.assertIsNone(sample["peak_rss_bytes"])
        self.assertNotIn("memory_reason", sample)

    def test_download_and_unpacked_executable_reject_changed_bytes(self):
        expected = hashlib.sha256(b"expected official bytes").hexdigest()
        spec = {"url": "https://example.test/artifact", "sha256": expected}
        with patch.object(bench, "read_url", return_value=b"changed bytes"):
            with self.assertRaises(bench.SetupError):
                bench.download(spec, self.cache)
        self.assertFalse((self.cache / expected).exists())
        artifact = self.root / "artifact"
        artifact.write_bytes(b"changed bytes")
        destination = self.root / "executable"
        with self.assertRaises(bench.SetupError):
            bench.extract_executable(artifact, {"format": "raw", "executable_sha256": expected}, destination)
        self.assertFalse(destination.exists())

    def test_core_kit_is_rejected_before_acquisition(self):
        with patch.object(bench, "read_url") as network:
            with self.assertRaises(bench.SetupError):
                bench.acquire_release("5.21.0", {"channel": "core-kit"}, self.cache,
                                      self.work, self.env, self.logs, "benchmark", 10)
        network.assert_not_called()

    def release_fixture(self, official, source_files):
        """A release spec whose artifact and public source archive are served by the mock network."""
        recipe = source_files["source/build.py"]
        source_archive = self.root / "source.tar.gz"
        self.archive(source_archive, source_files)
        commit = "a" * 40
        artifact_url = "https://github.com/studiohaze/topaz/releases/download/v5.21.0/fixture"
        source_url = f"{bench.PUBLIC_REPOSITORY}/archive/{commit}.tar.gz"
        spec = {
            "channel": "npm",
            "artifact": {"url": artifact_url, "format": "raw",
                         "sha256": hashlib.sha256(official).hexdigest(),
                         "executable_sha256": hashlib.sha256(official).hexdigest()},
            "source": {"url": source_url, "sha256": bench.digest(source_archive),
                       "commit": commit, "recipe": "build.py",
                       "recipe_sha256": hashlib.sha256(recipe).hexdigest(),
                       "interpreter": "python", "interpreter_path": sys.executable,
                       "source_date_epoch": "0", "executable": "rebuilt/topaz"},
        }
        responses = {
            artifact_url: official,
            source_url: source_archive.read_bytes(),
            "https://registry.npmjs.org/topaz-lang/5.21.0": json.dumps({
                "name": "topaz-lang", "version": "5.21.0"}).encode(),
        }
        return spec, responses

    def test_source_archive_cannot_supply_the_declared_rebuild_output(self):
        official = b"official executable fixture"
        spec, responses = self.release_fixture(official, {"source/build.py": b"pass\n", "source/rebuilt/topaz": official})
        with patch.object(bench, "read_url", side_effect=responses.__getitem__), \
                patch.object(bench, "inspect_tool", return_value={"version": "5.21.0"}), \
                patch.object(bench, "command") as rebuild:
            with self.assertRaises(bench.SetupError):
                bench.acquire_release("5.21.0", spec, self.cache, self.work,
                                      self.env, self.logs, "benchmark", 10)
        rebuild.assert_not_called()

    def test_only_the_benchmark_method_can_verify_public_source_eligibility(self):
        # The same release and a recipe that rebuilds identical bytes: the
        # benchmark method verifies it, the baseline never runs the rebuild.
        official = b"official executable fixture"
        spec, responses = self.release_fixture(official, {"source/build.py": b"pass\n"})

        def rebuild_identical(argv, cwd, env, timeout, logs, name):
            (cwd / "rebuilt").mkdir()
            (cwd / "rebuilt" / "topaz").write_bytes(official)
            return b""

        results = {}
        for method in ("benchmark", "baseline"):
            work = self.work / method
            work.mkdir()
            with patch.object(bench, "read_url", side_effect=responses.__getitem__), \
                    patch.object(bench, "inspect_tool", return_value={"version": "5.21.0"}), \
                    patch.object(bench, "command", side_effect=rebuild_identical) as rebuild:
                _, results[method] = bench.acquire_release("5.21.0", spec, self.cache, work,
                                                           self.env, self.logs, method, 10)
            results[method + "_rebuilds"] = rebuild.call_count
        self.assertEqual(results["benchmark"]["eligibility"], "verified_public_source_same_bytes")
        self.assertEqual(results["benchmark_rebuilds"], 1)
        self.assertEqual(results["baseline"]["eligibility"], "official_download_identity_only")
        self.assertEqual(results["baseline_rebuilds"], 0)
        self.assertNotIn("byte_comparison", results["baseline"])
        self.assertNotIn("rebuilt_executable", results["baseline"])

    def test_source_archive_rejects_traversal_and_links(self):
        for index, member in enumerate(("../escaped", "source/../../escaped", "/absolute", "C:/escape")):
            with self.subTest(member=member):
                archive = self.root / "unsafe.tar.gz"
                self.archive(archive, {member: b"outside"})
                with self.assertRaises(bench.SetupError):
                    bench.extract_source(archive, self.root / f"extract-path-{index}")
        for index, kind in enumerate((tarfile.SYMTYPE, tarfile.LNKTYPE)):
            with self.subTest(link=kind):
                archive = self.root / "link.tar.gz"
                self.archive(archive, link=kind)
                with self.assertRaises(bench.SetupError):
                    bench.extract_source(archive, self.root / f"extract-link-{index}")
        self.assertFalse((self.root / "escaped").exists())

    def test_baseline_plans_topaz_native_only(self):
        tools = {"typescript": {"version": "5.9.2"}, "python": {"version": "3.12.10"},
                 "rustc": {"version": "1.96.0"}}
        lanes = {}
        for method in ("baseline", "benchmark", "smoke"):
            cases = bench.planned_cases(["fibonacci", "empty"], list(bench.LANGUAGES),
                                        ["5.21.3"], tools, method)
            lanes[method] = {(case["language"], case["mode"]) for case in cases}
        self.assertNotIn(("topaz", "run"), lanes["baseline"])
        self.assertIn(("topaz", "native"), lanes["baseline"])
        for method in ("benchmark", "smoke"):
            self.assertIn(("topaz", "run"), lanes[method])
            self.assertIn(("topaz", "native"), lanes[method])
        self.assertEqual(lanes["baseline"] | {("topaz", "run")}, lanes["benchmark"])

    def test_quick_and_duplicate_argument_validation(self):
        rejected = [
            ["--languages", "python,python"],
            ["--programs", "sieve,sieve"],
            ["--quick"],
            ["--quick", "--releases", "5.21.0,5.21.1"],
            ["--quick", "--releases", "5.21.0", "--programs", "sieve,matrix"],
            ["--baseline", "--quick", "--releases", "5.21.0", "--programs", "fibonacci"],
        ]
        for argv in rejected:
            with self.subTest(argv=argv), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    bench.parse_args(argv)
                self.assertEqual(error.exception.code, 2)
        with patch.object(bench, "read_url") as network:
            with self.assertRaises(bench.SetupError):
                bench.official_releases("5.21.0,5.21.0")
        network.assert_not_called()
        quick = bench.parse_args(["--quick", "--releases", "5.21.0", "--languages", "python"])
        self.assertEqual(quick.programs, ["fibonacci"])
        self.assertEqual((quick.repetitions, quick.warmups), (1, 0))
        baseline = bench.parse_args(["--baseline", "--releases", "5.21.3"])
        self.assertEqual(baseline.method, "baseline")
        self.assertEqual(baseline.programs, list(bench.PROGRAMS))
        self.assertEqual((baseline.repetitions, baseline.warmups), (15, 3))

    @unittest.skipUnless(shutil.which("node"), "Node is required for the CommonJS boundary check")
    def test_commonjs_product_runs_beneath_an_es_module_parent(self):
        (self.root / "package.json").write_text('{"type":"module"}\n', encoding="utf-8")
        (self.program / "main.ts").write_text('console.log("checked");\n', encoding="utf-8")
        case = {**self.case, "id": "probe-typescript", "language": "typescript"}
        directory = bench.write_workspace(case, self.work)
        product = directory / "product" / "main.js"
        product.write_text('require("node:fs"); process.stdout.write("checked\\n");\n', encoding="utf-8")
        result = subprocess.run([shutil.which("node"), str(product)], cwd=directory,
                                capture_output=True, timeout=10, check=False)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        self.assertEqual(result.stdout, b"checked\n")

    def test_environment_resets_locale_and_removes_host_overrides(self):
        inherited = {"TZ": "Asia/Seoul", "LC_ALL": "ko_KR.UTF-8", "LANG": "ko_KR.UTF-8",
                     "PYTHONHASHSEED": "random", "NODE_OPTIONS": "--inspect",
                     "RUSTC_WRAPPER": "wrapper", "TOPAZ_STORAGE_DIR": "shared",
                     "XDG_RUNTIME_DIR": "shared", "PATH": "tool-path",
                     "TOPAZ_COMPILER_MEMORY_MIB": "custom-limit",
                     "TOPAZ_COMPILER_TIME_MS": "custom-budget",
                     "TOPAZ_SELF_FRONTEND_METRICS": "metrics.json"}
        with patch.dict(os.environ, inherited, clear=True):
            env = bench.base_environment({})
        self.assertEqual((env["TZ"], env["LC_ALL"], env["LANG"]), ("UTC", "C", "C"))
        self.assertEqual(env["PATH"], "tool-path")
        for name in ("PYTHONHASHSEED", "NODE_OPTIONS", "RUSTC_WRAPPER",
                     "TOPAZ_STORAGE_DIR", "XDG_RUNTIME_DIR", "TOPAZ_COMPILER_MEMORY_MIB",
                     "TOPAZ_COMPILER_TIME_MS", "TOPAZ_SELF_FRONTEND_METRICS"):
            self.assertNotIn(name, env)

    def test_typescript_identity_includes_the_loaded_compiler_implementation(self):
        package = self.root / "typescript"
        library = package / "lib"
        library.mkdir(parents=True)
        entry = library / "tsc.js"
        implementation = library / "_tsc.js"
        entry.write_text('require("./_tsc.js");\n', encoding="utf-8")
        implementation.write_text("// first compiler implementation\n", encoding="utf-8")
        (package / "package.json").write_text('{"name":"typescript","version":"5.9.3"}\n', encoding="utf-8")
        spec = {"path": str(entry), "version": "5.9.3"}
        with patch.object(bench, "command", return_value=b"Version 5.9.3\n"):
            before = bench.inspect_tool("typescript", spec, ["--version"], self.env, self.logs)
            implementation.write_text("// changed compiler implementation\n", encoding="utf-8")
            after = bench.inspect_tool("typescript", spec, ["--version"], self.env, self.logs)
        self.assertEqual(before["sha256"], after["sha256"])
        before_files = {entry["path"]: entry["sha256"] for entry in before["package_files"]}
        after_files = {entry["path"]: entry["sha256"] for entry in after["package_files"]}
        self.assertNotEqual(before_files[str(Path("lib") / "_tsc.js")], after_files[str(Path("lib") / "_tsc.js")])


if __name__ == "__main__":
    unittest.main()
