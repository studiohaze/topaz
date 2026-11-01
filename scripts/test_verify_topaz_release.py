"""Offline tests that reach the verifier's signature and byte comparison steps.

Every release input is pinned, so tampering with a real input stops at the pin
check before openssl or the seed comparison runs. These tests build small
synthetic inputs with their own key and pins instead. Only the test copy of
the module is changed. The release pins in the script stay as they are.
"""

import gzip
import hashlib
import io
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import types
import unittest
from dataclasses import replace
from pathlib import Path
from unittest import mock

from release_test_support import fixture, load, release_cases


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def openssl(*args):
    subprocess.run(["openssl", *args], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def flip_byte(data, index=0):
    return data[:index] + bytes([data[index] ^ 0x01]) + data[index + 1:]


class VerifierReachesRealChecks(unittest.TestCase):
    def __str__(self):
        return super().__str__() + f" [{self.release_key[0]} {self.release_key[1]}]"

    def setUp(self):
        self.verifier = load("verify-topaz-release.py")
        self.record = next(r for r in release_cases(self.verifier)
                           if (r.version, r.target) == self.release_key)
        self.work = Path(tempfile.mkdtemp(prefix="topaz-verify-test-"))
        self.addCleanup(shutil.rmtree, self.work, ignore_errors=True)

        v = self.verifier
        # Small signed inputs exercise every record without downloading or
        # running release executables. Different releases have different sizes.
        cli = os.urandom(4096 + len(self.record.version))
        compiler = b"// synthetic compiler.rs\n" + os.urandom(2048 + len(self.record.version))
        name = self.record.name
        (self.work / name).write_bytes(cli)
        (self.work / (name + ".gz")).write_bytes(gzip.compress(cli, mtime=0))
        (self.work / "compiler.rs").write_bytes(compiler)
        self.write_seed(compiler)

        key = self.work / "signing-key.pem"
        openssl("genpkey", "-algorithm", "ed25519", "-out", str(key))
        openssl("pkey", "-in", str(key), "-pubout", "-out", str(self.work / "topaz-release.pub"))
        self.signing_key = key
        if self.record.has_link_check:
            self.write_link_records(cli, compiler)
        checksums = "0" * 64 + "  topaz-other-target.gz\n" + "".join(
            f"{sha256(self.work / filename)}  {filename}\n" for filename in [name + ".gz", name])
        if self.record.has_link_check:
            checksums += f"{sha256(self.work / self.record.product_link)}  {self.record.product_link}\n"
        (self.work / "checksums.txt").write_text(checksums, encoding="ascii", newline="\n")
        openssl("pkeyutl", "-sign", "-inkey", str(key), "-rawin",
                "-in", str(self.work / "checksums.txt"), "-out", str(self.work / "checksums.txt.sig"))
        self.signing_key = key
        # The raw CLI is checked through the gzip stream, not read from disk.
        (self.work / name).unlink()

        pins = {filename: sha256(self.work / filename) for filename in self.record.inputs}
        pins[name] = hashlib.sha256(cli).hexdigest()
        limits = {filename: (self.work / filename).stat().st_size for filename in self.record.limits}
        self.record = replace(self.record, raw_size=len(cli), core_size=len(compiler), pins=pins, limits=limits)

        self.openssl_runs = []
        real_run = subprocess.run

        # OpenSSL 3.0 reports a failed verification on stdout and 3.5 on
        # stderr, so both streams are kept for the assertion.
        def recording_run(command, **kwargs):
            kwargs["stdout"] = subprocess.PIPE
            kwargs["stderr"] = subprocess.PIPE
            try:
                completed = real_run(command, **kwargs)
            except subprocess.CalledProcessError as error:
                self.openssl_runs.append((command, error.returncode, (error.output or b"") + (error.stderr or b"")))
                raise
            self.openssl_runs.append((command, completed.returncode, (completed.stdout or b"") + (completed.stderr or b"")))
            return completed

        v.subprocess = types.SimpleNamespace(run=recording_run, DEVNULL=subprocess.DEVNULL,
                                             CalledProcessError=subprocess.CalledProcessError)

    def write_seed(self, compiler):
        with tarfile.open(self.work / self.record.seed, "w:gz") as seed:
            member = tarfile.TarInfo(self.verifier.MEMBER)
            member.size = len(compiler)
            seed.addfile(member, io.BytesIO(compiler))

    def repin(self, name):
        self.record.pins[name] = sha256(self.work / name)
        if name in self.record.limits:
            self.record.limits[name] = (self.work / name).stat().st_size

    def write_link_records(self, cli, compiler):
        product = fixture(self.record.product_link)
        product["subject"] = {"name": self.record.name, "sha256": hashlib.sha256(cli).hexdigest(), "bytes": len(cli)}
        product["generatedRustSha256"] = hashlib.sha256(compiler).hexdigest()
        self.write_json(self.record.product_link, product)
        followup = product | {
            "seedSourceRevision": product["productSourceRevision"],
            "seedArchiveSha256": sha256(self.work / self.record.seed),
            "publicKitCommit": self.record.commit,
            "linkedProductRecordSha256": sha256(self.work / self.record.product_link),
        }
        self.write_json(self.record.seed_link, followup)
        filename = self.verifier.SEED_LINK_CHECKSUMS
        (self.work / filename).write_text(
            f"{'0' * 64}  topaz-other-target.release-core-link.seed.json\n"
            f"{sha256(self.work / self.record.seed_link)}  {self.record.seed_link}\n", encoding="ascii", newline="\n")
        self.sign_list(filename)

    def write_json(self, filename, value):
        (self.work / filename).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8", newline="\n")

    def sign_list(self, filename):
        openssl("pkeyutl", "-sign", "-inkey", str(self.signing_key), "-rawin",
                "-in", str(self.work / filename), "-out", str(self.work / (filename + ".sig")))

    def assert_signature_checks(self, returncode_ok, count=None):
        expected = count if count is not None else (2 if returncode_ok and self.record.has_link_check else 1)
        self.assertEqual(len(self.openssl_runs), expected)
        for command, returncode, _ in self.openssl_runs:
            self.assertEqual(command[:3], ["openssl", "pkeyutl", "-verify"])
            self.assertEqual(returncode == 0, returncode_ok)

    def test_intact_inputs_pass(self):
        evidence = self.verifier.verify(self.work, download=False, release=self.record)
        self.assert_signature_checks(returncode_ok=True)
        self.assertEqual(evidence["subject"]["sha256"], self.record.pins[self.record.name])
        self.assertEqual(evidence["subject"]["bytes"], self.record.raw_size)
        self.assertEqual(evidence["version"], self.record.version)
        self.assertEqual(evidence["target"], self.record.target)
        self.assertEqual(evidence["compilerRust"]["commit"], self.record.commit)
        self.assertEqual(evidence["compilerRust"]["bytes"], self.record.core_size)
        self.assertEqual(evidence["compilerRust"]["publicUrl"],
                         f"https://raw.githubusercontent.com/studiohaze/topaz/{self.record.commit}/{self.verifier.PUBLIC_FILE}")
        self.assertEqual(evidence["compilerRust"]["seedUrl"],
                         f"https://topaz.ooo/dl/{self.record.version}/{self.record.seed}")
        self.assertEqual(evidence["checks"], list(self.record.checks))
        self.assertEqual(evidence["notEstablished"], list(self.record.not_established))
        if self.record.has_link_check:
            self.assertEqual(evidence["releaseCoreLink"], self.record.link_evidence())

    def test_changed_signature_fails_in_ed25519_check(self):
        signature = self.work / "checksums.txt.sig"
        signature.write_bytes(flip_byte(signature.read_bytes()))
        self.repin("checksums.txt.sig")
        with self.assertRaises(subprocess.CalledProcessError):
            self.verifier.verify(self.work, download=False, release=self.record)
        self.assert_signature_checks(returncode_ok=False)
        self.assertIn(b"signature", self.openssl_runs[0][2].lower())

    def test_changed_seed_compiler_fails_byte_comparison(self):
        compiler = (self.work / "compiler.rs").read_bytes()
        self.write_seed(flip_byte(compiler, len(compiler) // 2))
        self.repin(self.record.seed)
        with self.assertRaisesRegex(ValueError, "seed and public compiler Rust differ"):
            self.verifier.verify(self.work, download=False, release=self.record)
        self.assert_signature_checks(returncode_ok=True, count=1)

    def test_unpinned_change_fails_before_openssl(self):
        compiler = self.work / "compiler.rs"
        compiler.write_bytes(flip_byte(compiler.read_bytes()))
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch: compiler.rs"):
            self.verifier.verify(self.work, download=False, release=self.record)
        self.assertEqual(self.openssl_runs, [])

    def test_workflow_default_selects_current_record(self):
        self.verifier.RELEASES[self.release_key] = self.record
        self.verifier.CURRENT_RELEASE = self.release_key
        evidence = self.verifier.verify(self.work, download=False)
        self.assertEqual(evidence["version"], self.record.version)
        self.assertEqual(evidence["subject"]["sha256"], self.record.pins[self.record.name])
        self.assert_signature_checks(returncode_ok=True)

    def test_each_input_limit_is_enforced_before_openssl(self):
        for filename, (_, size) in self.record.inputs.items():
            with self.subTest(input=filename):
                if filename == "compiler.rs":
                    record = replace(self.record, core_size=size - 1)
                else:
                    record = replace(self.record, limits=self.record.limits | {filename: size - 1})
                with self.assertRaisesRegex(ValueError, f"input exceeds {size - 1} bytes"):
                    self.verifier.verify(self.work, download=False, release=record)
        self.assertEqual(self.openssl_runs, [])

    def test_raw_cli_size_and_digest_are_enforced(self):
        for change, reason in [({"raw_size": self.record.raw_size - 1}, "input exceeds"),
                               ({"raw_size": self.record.raw_size + 1}, "decompressed CLI mismatch"),
                               ({"pins": self.record.pins | {self.record.name: "0" * 64}}, "wrong signed record")]:
            with self.subTest(change=list(change)):
                with self.assertRaisesRegex(ValueError, reason):
                    self.verifier.verify(self.work, download=False, release=replace(self.record, **change))

    def test_changed_decompressed_cli_fails_digest_check(self):
        name = self.record.name
        archive = self.work / (name + ".gz")
        cli = flip_byte(gzip.decompress(archive.read_bytes()))
        self.assertEqual(len(cli), self.record.raw_size)
        self.assertNotEqual(hashlib.sha256(cli).hexdigest(), self.record.pins[name])
        archive.write_bytes(gzip.compress(cli, mtime=0))
        self.repin(name + ".gz")

        # Keep the signed raw digest and its pin: only the decompressed bytes
        # disagree. The archive, checksum list and signature must all pass.
        checksums = self.work / "checksums.txt"
        checksums.write_text(
            f"{self.record.pins[name + '.gz']}  {name}.gz\n"
            f"{self.record.pins[name]}  {name}\n" +
            (f"{self.record.pins[self.record.product_link]}  {self.record.product_link}\n"
             if self.record.has_link_check else ""), encoding="ascii", newline="\n")
        openssl("pkeyutl", "-sign", "-inkey", str(self.signing_key), "-rawin",
                "-in", str(checksums), "-out", str(self.work / "checksums.txt.sig"))
        self.repin("checksums.txt")
        self.repin("checksums.txt.sig")

        with self.assertRaisesRegex(ValueError, "decompressed CLI mismatch"):
            self.verifier.verify(self.work, download=False, release=self.record)
        self.assertEqual(len(self.openssl_runs), 1)
        self.assertEqual(self.openssl_runs[0][1], 0)

    def test_wrong_compiler_member_size_is_rejected(self):
        record = replace(self.record, core_size=self.record.core_size + 1)
        with self.assertRaisesRegex(ValueError, "invalid compiler member"):
            self.verifier.verify(self.work, download=False, release=record)
        self.assertEqual(len(self.openssl_runs), 1)
        self.assertEqual(self.openssl_runs[0][1], 0)

    def test_downloads_use_record_urls(self):
        urls = {f"https://topaz.ooo/dl/{self.record.version}/{name}": self.work / name
                for name in self.record.limits}
        urls["https://topaz.ooo/dl/topaz-release.pub"] = urls.pop(
            f"https://topaz.ooo/dl/{self.record.version}/topaz-release.pub")
        urls[f"https://raw.githubusercontent.com/studiohaze/topaz/{self.record.commit}/{self.verifier.PUBLIC_FILE}"] = (
            self.work / "compiler.rs")
        with tempfile.TemporaryDirectory(prefix="topaz-download-test-") as output:
            def source(url, timeout):
                self.assertEqual(timeout, 60)
                return urls.pop(url).open("rb")
            with mock.patch.object(self.verifier.urllib.request, "urlopen", side_effect=source):
                evidence = self.verifier.verify(Path(output), download=True, release=self.record)
        self.assertFalse(urls)
        self.assertEqual(evidence["version"], self.record.version)
        self.assert_signature_checks(returncode_ok=True)


def load_tests(loader, tests, pattern):
    """Adding a production record automatically runs the same byte checks."""
    suite = unittest.TestSuite()
    for record in release_cases(load("verify-topaz-release.py")):
        for name in loader.getTestCaseNames(VerifierReachesRealChecks):
            case = VerifierReachesRealChecks(name)
            case.release_key = (record.version, record.target)
            suite.addTest(case)
    return suite


if __name__ == "__main__":
    unittest.main()
