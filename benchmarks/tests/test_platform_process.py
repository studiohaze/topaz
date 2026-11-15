"""Behavior checks for process capture, timeouts and OS counter availability."""

import os
from pathlib import Path
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from platform_process import capture, machine_metadata, measure, validate_windows_environment


class ProcessMeasurementTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def run_python(self, source, *, timeout=10, memory=True):
        return measure([sys.executable, "-c", source], self.root, dict(os.environ),
                       timeout, self.root, memory)

    def test_captures_streams_exit_code_and_kernel_counter(self):
        result = self.run_python(
            "import sys; print('stdout'); print('stderr', file=sys.stderr); sys.exit(7)")
        self.assertEqual(result["stdout"].replace(b"\r\n", b"\n"), b"stdout\n")
        self.assertEqual(result["stderr"].replace(b"\r\n", b"\n"), b"stderr\n")
        self.assertEqual(result["exit_code"], 7)
        self.assertFalse(result["timed_out"])
        self.assertIsInstance(result["elapsed_ns"], int)
        self.assertGreaterEqual(result["elapsed_ns"], 0)
        self.assertIsInstance(result["peak_rss_bytes"], int)
        self.assertGreater(result["peak_rss_bytes"], 0)
        self.assertIn(result["memory_scope"], (
            "root-process-peak-working-set",
            "process-and-waited-descendants-maximum-rss-not-sum"))

    def test_build_can_disable_memory_collection(self):
        result = self.run_python("pass", memory=False)
        self.assertIsNone(result["peak_rss_bytes"])
        self.assertEqual(result["memory_scope"], "not-measured")

    def test_large_output_does_not_deadlock(self):
        result = self.run_python(
            "import sys; sys.stdout.write('x' * 131072); sys.stderr.write('y' * 131072)")
        self.assertEqual(result["stdout"], b"x" * 131072)
        self.assertEqual(result["stderr"], b"y" * 131072)
        self.assertEqual(result["exit_code"], 0)

    def test_timeout_terminates_descendants(self):
        child = "import time; from pathlib import Path; time.sleep(0.6); Path('leaked').write_text('bad')"
        source = (
            "import subprocess, sys, time; "
            f"subprocess.Popen([sys.executable, '-c', {child!r}]); "
            "print('started', flush=True); time.sleep(30)"
        )
        for operation in (measure, capture):
            with self.subTest(operation=operation.__name__):
                result = operation([sys.executable, "-c", source], self.root,
                                   dict(os.environ), 0.3, self.root)
                self.assertTrue(result["timed_out"])
                self.assertNotEqual(result["exit_code"], 0)
                self.assertIn(b"started", result["stdout"])
                time.sleep(0.7)
                self.assertFalse((self.root / "leaked").exists())

    def test_untimed_capture_retains_no_measurement_fields(self):
        source = "import sys; print('checked'); print('detail', file=sys.stderr); sys.exit(3)"
        result = capture([sys.executable, "-c", source], self.root,
                         dict(os.environ), 10, self.root)
        self.assertEqual(set(result), {"exit_code", "timed_out", "stdout", "stderr"})
        self.assertEqual(result["exit_code"], 3)
        self.assertFalse(result["timed_out"])
        self.assertEqual(result["stdout"].replace(b"\r\n", b"\n"), b"checked\n")
        self.assertEqual(result["stderr"].replace(b"\r\n", b"\n"), b"detail\n")

    def test_machine_facts_mark_missing_values_explicitly(self):
        facts = machine_metadata()
        self.assertTrue(facts["os"])
        self.assertTrue(facts["architecture"])
        self.assertTrue(facts["measurement_clock"]["monotonic"])
        for field in ("cpu_model", "physical_cpu_cores", "physical_memory_bytes", "logical_cpus"):
            self.assertEqual(facts[field] is None, field in facts["unavailable"])

    def test_invalid_timeout_fails_before_launch(self):
        for timeout in (0, -1, float("inf"), float("nan")):
            with self.assertRaises(ValueError):
                self.run_python("raise SystemExit('must not run')", timeout=timeout)

    def test_windows_environment_rejects_case_insensitive_duplicates(self):
        with self.assertRaises(ValueError):
            validate_windows_environment({"Path": "first", "PATH": "second"})
        validate_windows_environment({"Path": "first", "TEMP": "temporary"})


if __name__ == "__main__":
    unittest.main()
