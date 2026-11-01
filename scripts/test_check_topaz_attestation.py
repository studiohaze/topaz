"""Acceptance tests shared by every release, including a test-only second one."""

import copy
import unittest
from dataclasses import replace

from release_test_support import attestation, load, release_cases, statement


class AttestationAcceptance(unittest.TestCase):
    def setUp(self):
        self.checker = load("check-topaz-attestation.py")
        self.release = load("verify-topaz-release.py")
        self.records = release_cases(self.release)
        self.release.RELEASES = {(r.version, r.target): r for r in self.records}
        # Model the next release becoming current. Historical results must
        # still select their own record, regardless of this workflow default.
        last = self.records[-1]
        self.release.CURRENT_RELEASE = (last.version, last.target)

    def rejects(self, results, reason):
        with self.assertRaisesRegex(ValueError, reason):
            self.checker.check(results, self.release)

    def test_v2_attestations_pass_for_each_release(self):
        for record in self.records:
            with self.subTest(version=record.version, target=record.target):
                self.assertEqual(self.checker.check(attestation(record), self.release), 1)

    def test_each_result_selects_its_own_release(self):
        results = [result for record in self.records for result in attestation(record)]
        self.assertEqual(self.checker.check(results, self.release), len(results))

    def test_v2_rejects_changed_contract_for_each_release(self):
        cases = {
            "subjects are not exactly": lambda s: s["subject"].pop(),
            "verification is not": lambda s: s["predicate"].update(verification="runtime"),
            "unexpected checks": lambda s: s["predicate"]["checks"].pop(),
            "notEstablished is missing": lambda s: s["predicate"].pop("notEstablished"),
            "predicate digests": lambda s: s["predicate"]["compilerRust"].update(sha256="0" * 64),
            "unknown release": lambda s: s["predicate"].update(version="v0.0.0-unknown"),
            "unknown release version or target": lambda s: s["predicate"].update(target="unknown"),
            "predicate type": lambda s: s.update(predicateType=self.checker.PREDICATE_TYPE.replace("/v2", "/v1")),
        }
        for record in self.records:
            for reason, change in cases.items():
                with self.subTest(version=record.version, reason=reason):
                    results = attestation(record)
                    change(statement(results))
                    self.rejects(results, reason)

    def test_v2_rejects_other_signer_and_repository_for_each_release(self):
        for record in self.records:
            for field, reason in [("buildSignerURI", "another workflow"),
                                  ("sourceRepositoryURI", "another repository")]:
                with self.subTest(version=record.version, field=field):
                    results = attestation(record)
                    results[0]["verificationResult"]["signature"]["certificate"][field] += "-other"
                    self.rejects(results, reason)
        self.rejects([], "no verified attestation")

    def test_v2_rejects_cross_release_subjects(self):
        for record in self.records:
            for other in self.records:
                if record == other:
                    continue
                with self.subTest(version=record.version, subjects_from=other.version):
                    results = attestation(record)
                    statement(results)["subject"] = copy.deepcopy(statement(attestation(other))["subject"])
                    self.rejects(results, "subjects are not exactly")

    def test_link_check_does_not_change_historical_policy(self):
        old, new = self.records[0], self.records[-1]
        self.assertNotIn(self.release.LINK_RECORD_CHECK, old.checks)
        self.assertIn(self.release.LINK_RECORD_CHECK, new.checks)
        self.assertEqual(self.checker.check(attestation(old), self.release), 1)
        results = attestation(old)
        statement(results)["predicate"]["checks"] = list(new.checks)
        self.rejects(results, "unexpected checks")
        results = attestation(new)
        statement(results)["predicate"]["checks"] = list(old.checks)
        self.rejects(results, "unexpected checks")

    def test_link_record_requires_the_linking_caveat(self):
        for record in self.records:
            if not record.has_link_check:
                continue
            self.assertIn("official-cli-links-the-public-compiler-rust", record.not_established)
            changed = replace(record, not_established=tuple(
                item for item in record.not_established if item != "official-cli-links-the-public-compiler-rust"))
            self.release.RELEASES[(record.version, record.target)] = changed
            # Even a predicate agreeing with the weakened record must fail.
            results = attestation(record)
            statement(results)["predicate"]["notEstablished"] = list(changed.not_established)
            self.rejects(results, "link-record check must keep official-cli-links-the-public-compiler-rust")

    def test_link_attestation_rejects_changed_evidence(self):
        record = self.records[-1]
        for field in ["productRecord", "seedRecord", "publicCommit", "compilerRustSha256", "executableSha256"]:
            with self.subTest(field=field):
                results = attestation(record)
                statement(results)["predicate"]["releaseCoreLink"][field] = "changed"
                self.rejects(results, "releaseCoreLink does not match")
        results = attestation(record)
        statement(results)["predicate"].pop("releaseCoreLink")
        self.rejects(results, "releaseCoreLink does not match")
        results = attestation(record)
        statement(results)["predicate"]["compilerRust"]["commit"] = "0" * 40
        self.rejects(results, "public compiler identity mismatch")


if __name__ == "__main__":
    unittest.main()
