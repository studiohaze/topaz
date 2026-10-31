"""Offline tests for the v2 attestation policy on real gh attestation verify output.

testdata/attestation-v1-cli-only.json is the first attestation, signed at
d9757ace with only the CLI as subject. testdata/attestation-v1-three-subjects.json
was signed at d552cf62 with all three files but still under the v1 type.
"""

import copy
import importlib.util
import json
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
V1 = "https://github.com/studiohaze/topaz/attestations/static-release-bytes/v1"


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fixture(name):
    return json.loads((HERE / "testdata" / name).read_text(encoding="utf-8"))


def statement(results):
    return results[0]["verificationResult"]["statement"]


def v1_policy(results):
    """What a consumer of v1 could check: signer, type and the CLI digest."""
    certificate = results[0]["verificationResult"]["signature"]["certificate"]
    subjects = {s["name"]: s["digest"]["sha256"] for s in statement(results)["subject"]}
    return (certificate["buildSignerURI"] == checker.SIGNER
            and statement(results)["predicateType"] == V1
            and subjects.get(release.NAME) == release.PINS[release.NAME])


checker = load("check_under_test", "check-topaz-attestation.py")
release = load("release_under_test", "verify-topaz-release.py")
POLICY = checker.v2_policy(release)


class AttestationPolicy(unittest.TestCase):
    def setUp(self):
        self.old = fixture("attestation-v1-cli-only.json")
        self.new_v1 = fixture("attestation-v1-three-subjects.json")
        # The v2 workflow emits the same statement with the new type.
        self.v2 = copy.deepcopy(self.new_v1)
        statement(self.v2)["predicateType"] = checker.PREDICATE_TYPE

    def rejects(self, results, reason):
        with self.assertRaisesRegex(ValueError, reason):
            checker.check(results, POLICY)

    def test_old_attestation_passes_v1_and_is_rejected_by_v2(self):
        self.assertTrue(v1_policy(self.old))
        self.rejects(self.old, "predicate type")
        statement(self.old)["predicateType"] = checker.PREDICATE_TYPE
        self.rejects(self.old, "subjects are not exactly")

    def test_three_subject_v1_attestation_is_rejected_by_type(self):
        self.assertTrue(v1_policy(self.new_v1))
        self.rejects(self.new_v1, "predicate type")

    def test_v2_attestation_passes(self):
        self.assertEqual(checker.check(self.v2, POLICY), 1)

    def test_v2_rejects_changed_contract(self):
        cases = {
            "subjects are not exactly": lambda s: s["subject"].pop(),
            "verification is not": lambda s: s["predicate"].update(verification="runtime"),
            "unexpected checks": lambda s: s["predicate"]["checks"].pop(),
            "notEstablished is missing": lambda s: s["predicate"].pop("notEstablished"),
            "predicate digests": lambda s: s["predicate"]["compilerRust"].update(sha256="0" * 64),
        }
        for reason, change in cases.items():
            with self.subTest(reason):
                results = copy.deepcopy(self.v2)
                change(statement(results))
                self.rejects(results, reason)

    def test_v2_rejects_other_signer_and_empty_output(self):
        certificate = self.v2[0]["verificationResult"]["signature"]["certificate"]
        certificate["buildSignerURI"] = certificate["buildSignerURI"].replace("refs/heads/main", "refs/heads/x")
        self.rejects(self.v2, "another workflow")
        self.rejects([], "no verified attestation")


if __name__ == "__main__":
    unittest.main()
