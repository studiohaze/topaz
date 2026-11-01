"""Reject signed but inconsistent link records using synthetic release bytes."""

import json
import subprocess
import unittest
from dataclasses import replace

import test_verify_topaz_release as byte_tests
from release_test_support import attestation, hypothetical_release, load, release_cases, statement


class LinkRecordChecks(byte_tests.VerifierReachesRealChecks):
    def change_link(self, filename, change):
        record = json.loads((self.work / filename).read_text(encoding="utf-8"))
        change(record)
        self.write_json(filename, record)
        self.repin(filename)
        # Re-sign the enclosing list so failures reach the semantic check.
        list_name = ("checksums.txt" if filename == self.record.product_link
                     else self.verifier.SEED_LINK_CHECKSUMS)
        path = self.work / list_name
        lines = [line for line in path.read_text(encoding="ascii").splitlines()
                 if line.split()[-1:] != [filename]]
        lines.append(f"{self.record.pins[filename]}  {filename}")
        path.write_text("\n".join(lines) + "\n", encoding="ascii", newline="\n")
        self.sign_list(list_name)
        self.repin(list_name)
        self.repin(list_name + ".sig")

    def rejects_link(self, filename, change, reason):
        self.change_link(filename, change)
        with self.assertRaisesRegex(ValueError, reason):
            self.verifier.verify(self.work, download=False, release=self.record)
        self.assert_signature_checks(returncode_ok=True)

    def test_link_wrong_executable_hash(self):
        for filename in [self.record.product_link, self.record.seed_link]:
            with self.subTest(record=filename):
                self.openssl_runs.clear()
                self.rejects_link(filename, lambda r: r["subject"].update(sha256="0" * 64),
                                  "link record executable bytes or SHA-256 mismatch")
                self.change_link(filename, lambda r: r["subject"].update(sha256=self.record.pins[self.record.name]))

    def test_link_wrong_public_commit(self):
        self.rejects_link(self.record.seed_link, lambda r: r.update(publicKitCommit="0" * 40),
                          "link record public commit mismatch with pinned public repository commit")

    def test_link_wrong_compiler_hash(self):
        self.rejects_link(self.record.seed_link, lambda r: r.update(generatedRustSha256="0" * 64),
                          "link record compiler.rs SHA-256 mismatch with public repository")

    def test_link_wrong_executable_name_and_size(self):
        for change in [{"name": "topaz-other"}, {"bytes": self.record.raw_size + 1}]:
            with self.subTest(change=change):
                self.openssl_runs.clear()
                self.rejects_link(self.record.seed_link, lambda r: r["subject"].update(change),
                                  "link record executable bytes or SHA-256 mismatch")
                self.change_link(self.record.seed_link, lambda r: r.update(subject={
                    "name": self.record.name, "bytes": self.record.raw_size,
                    "sha256": self.record.pins[self.record.name]}))

    def test_link_wrong_schema_version_and_target(self):
        for field, reason in [("schema", "link record schema mismatch"),
                              ("version", "link record release or target mismatch"),
                              ("target", "link record release or target mismatch")]:
            original = json.loads((self.work / self.record.seed_link).read_text(encoding="utf-8"))[field]
            with self.subTest(field=field):
                self.openssl_runs.clear()
                self.rejects_link(self.record.seed_link, lambda r: r.update({field: "wrong"}), reason)
                self.change_link(self.record.seed_link, lambda r: r.update({field: original}))

    def test_link_missing_duplicate_and_wrong_signed_entries(self):
        for filename, list_name in [(self.record.product_link, "checksums.txt"),
                                    (self.record.seed_link, self.verifier.SEED_LINK_CHECKSUMS)]:
            path = self.work / list_name
            original = path.read_text(encoding="ascii")
            entry = f"{self.record.pins[filename]}  {filename}\n"
            for changed in [original.replace(entry, ""), original + entry,
                            original.replace(entry, f"{'0' * 64}  {filename}\n")]:
                with self.subTest(record=filename, list_bytes=changed):
                    path.write_text(changed, encoding="ascii", newline="\n")
                    self.sign_list(list_name)
                    self.repin(list_name)
                    self.repin(list_name + ".sig")
                    with self.assertRaisesRegex(ValueError, "missing, duplicate or wrong signed record"):
                        self.verifier.verify(self.work, download=False, release=self.record)
            path.write_text(original, encoding="ascii", newline="\n")
            self.sign_list(list_name)
            self.repin(list_name)
            self.repin(list_name + ".sig")

    def test_link_changed_seed_signature_fails_in_ed25519(self):
        filename = self.verifier.SEED_LINK_CHECKSUMS + ".sig"
        path = self.work / filename
        path.write_bytes(byte_tests.flip_byte(path.read_bytes()))
        self.repin(filename)
        with self.assertRaises(subprocess.CalledProcessError):
            self.verifier.verify(self.work, download=False, release=self.record)
        self.assertEqual([result[1] == 0 for result in self.openssl_runs], [True, False])

    def test_link_wrong_product_record_hash(self):
        self.rejects_link(self.record.seed_link, lambda r: r.update(linkedProductRecordSha256="0" * 64),
                          "seed link does not match signed product link")

    def test_link_wrong_seed_archive_hash(self):
        self.rejects_link(self.record.seed_link, lambda r: r.update(seedArchiveSha256="0" * 64),
                          "link record seed archive SHA-256 mismatch")

    def test_link_not_established_guard_precedes_io(self):
        record = replace(self.record, not_established=tuple(
            item for item in self.record.not_established if item != "official-cli-links-the-public-compiler-rust"))
        with self.assertRaisesRegex(ValueError, "link-record check must keep official-cli-links-the-public-compiler-rust"):
            self.verifier.verify(self.work, download=False, release=record)
        self.assertEqual(self.openssl_runs, [])

    def test_link_verifier_output_passes_attestation_policy(self):
        evidence = self.verifier.verify(self.work, download=False, release=self.record)
        results = attestation(self.record)
        s = statement(results)
        s["predicate"] = evidence
        s["subject"] = [{"name": name, "digest": {"sha256": self.record.pins[name]}}
                        for name in [self.record.name, self.record.seed,
                                     self.record.product_link, self.record.seed_link]]
        s["subject"].append({"name": self.verifier.PUBLIC_FILE,
                             "digest": {"sha256": self.record.pins["compiler.rs"]}})
        self.verifier.RELEASES[(self.record.version, self.record.target)] = self.record
        self.assertEqual(load("check-topaz-attestation.py").check(results, self.verifier), 1)


def load_tests(loader, tests, pattern):
    suite = unittest.TestSuite()
    for record in release_cases(load("verify-topaz-release.py")):
        if not record.has_link_check:
            continue
        for name in sorted(LinkRecordChecks.__dict__):
            if name.startswith("test_link_"):
                case = LinkRecordChecks(name)
                case.release_key = (record.version, record.target)
                suite.addTest(case)
    return suite


if __name__ == "__main__":
    unittest.main()
