"""Offline tests that reach the verifier's signature and byte comparison steps.

Every release input is pinned, so tampering with a real input stops at the pin
check before openssl or the seed comparison runs. These tests build small
synthetic inputs with their own key and pins instead. Only the test copy of
the module is changed. The release pins in the script stay as they are.
"""

import gzip
import hashlib
import importlib.util
import io
import os
import shutil
import subprocess
import tarfile
import tempfile
import types
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().with_name("verify-topaz-release.py")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def openssl(*args):
    subprocess.run(["openssl", *args], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def flip_byte(data, index=0):
    return data[:index] + bytes([data[index] ^ 0x01]) + data[index + 1:]


class VerifierReachesRealChecks(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("verify_under_test", SCRIPT)
        self.verifier = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.verifier)
        self.work = Path(tempfile.mkdtemp(prefix="topaz-verify-test-"))
        self.addCleanup(shutil.rmtree, self.work, ignore_errors=True)

        v = self.verifier
        cli = os.urandom(4096)
        compiler = b"// synthetic compiler.rs\n" + os.urandom(2048)
        (self.work / v.NAME).write_bytes(cli)
        (self.work / (v.NAME + ".gz")).write_bytes(gzip.compress(cli, mtime=0))
        (self.work / "compiler.rs").write_bytes(compiler)
        self.write_seed(compiler)

        key = self.work / "signing-key.pem"
        openssl("genpkey", "-algorithm", "ed25519", "-out", str(key))
        openssl("pkey", "-in", str(key), "-pubout", "-out", str(self.work / "topaz-release.pub"))
        checksums = "0" * 64 + "  topaz-other-target.gz\n" + "".join(
            f"{sha256(self.work / name)}  {name}\n" for name in [v.NAME + ".gz", v.NAME])
        (self.work / "checksums.txt").write_text(checksums, encoding="ascii", newline="\n")
        openssl("pkeyutl", "-sign", "-inkey", str(key), "-rawin",
                "-in", str(self.work / "checksums.txt"), "-out", str(self.work / "checksums.txt.sig"))
        key.unlink()
        # The raw CLI is checked through the gzip stream, not read from disk.
        (self.work / v.NAME).unlink()

        v.RAW_SIZE = len(cli)
        v.CORE_SIZE = len(compiler)
        v.PINS = {name: sha256(self.work / name) for name in v.INPUTS}
        v.PINS[v.NAME] = hashlib.sha256(cli).hexdigest()

        self.openssl_runs = []
        real_run = subprocess.run

        def recording_run(command, **kwargs):
            kwargs["stderr"] = subprocess.PIPE
            try:
                completed = real_run(command, **kwargs)
            except subprocess.CalledProcessError as error:
                self.openssl_runs.append((command, error.returncode, error.stderr))
                raise
            self.openssl_runs.append((command, completed.returncode, completed.stderr))
            return completed

        v.subprocess = types.SimpleNamespace(run=recording_run, DEVNULL=subprocess.DEVNULL,
                                             CalledProcessError=subprocess.CalledProcessError)

    def write_seed(self, compiler):
        with tarfile.open(self.work / self.verifier.SEED, "w:gz") as seed:
            member = tarfile.TarInfo(self.verifier.MEMBER)
            member.size = len(compiler)
            seed.addfile(member, io.BytesIO(compiler))

    def repin(self, name):
        self.verifier.PINS[name] = sha256(self.work / name)

    def assert_one_signature_check(self, returncode_ok):
        self.assertEqual(len(self.openssl_runs), 1)
        command, returncode, _ = self.openssl_runs[0]
        self.assertEqual(command[:3], ["openssl", "pkeyutl", "-verify"])
        self.assertEqual(returncode == 0, returncode_ok)

    def test_intact_inputs_pass(self):
        evidence = self.verifier.verify(self.work, download=False)
        self.assert_one_signature_check(returncode_ok=True)
        self.assertEqual(evidence["subject"]["sha256"], self.verifier.PINS[self.verifier.NAME])
        self.assertEqual(evidence["notEstablished"], self.verifier.NOT_ESTABLISHED)

    def test_changed_signature_fails_in_ed25519_check(self):
        signature = self.work / "checksums.txt.sig"
        signature.write_bytes(flip_byte(signature.read_bytes()))
        self.repin("checksums.txt.sig")
        with self.assertRaises(subprocess.CalledProcessError):
            self.verifier.verify(self.work, download=False)
        self.assert_one_signature_check(returncode_ok=False)
        self.assertIn(b"signature", self.openssl_runs[0][2].lower())

    def test_changed_seed_compiler_fails_byte_comparison(self):
        compiler = (self.work / "compiler.rs").read_bytes()
        self.write_seed(flip_byte(compiler, len(compiler) // 2))
        self.repin(self.verifier.SEED)
        with self.assertRaisesRegex(ValueError, "seed and public compiler Rust differ"):
            self.verifier.verify(self.work, download=False)
        self.assert_one_signature_check(returncode_ok=True)

    def test_unpinned_change_fails_before_openssl(self):
        compiler = self.work / "compiler.rs"
        compiler.write_bytes(flip_byte(compiler.read_bytes()))
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch: compiler.rs"):
            self.verifier.verify(self.work, download=False)
        self.assertEqual(self.openssl_runs, [])


if __name__ == "__main__":
    unittest.main()
