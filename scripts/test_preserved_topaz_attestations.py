"""Keep the issued 5.21.1 statements under their historical policies.

The v1 CLI-only fixture was signed at d9757ace, the three-subject v1 at
d552cf62, and the v2 at 1c5b599 in run 41215275227. Only v2 is accepted today.
"""

import unittest

from release_test_support import PRESERVED_RELEASE, attestation, fixture, hypothetical_release, load, statement

V1 = "https://github.com/studiohaze/topaz/attestations/static-release-bytes/v1"


def v1_policy(results, checker, record):
    """What a v1 consumer could check then, using that release's CLI pin."""
    certificate = results[0]["verificationResult"]["signature"]["certificate"]
    subjects = {s["name"]: s["digest"]["sha256"] for s in statement(results)["subject"]}
    return (certificate["sourceRepositoryURI"] == checker.REPOSITORY
            and certificate["buildSignerURI"] == checker.SIGNER
            and statement(results)["predicateType"] == V1
            and subjects.get(record.name) == record.pins[record.name])


class Preserved5211Attestations(unittest.TestCase):
    def setUp(self):
        self.checker = load("check-topaz-attestation.py")
        self.release = load("verify-topaz-release.py")
        self.record = self.release.RELEASES[PRESERVED_RELEASE]
        next_release = hypothetical_release(self.release)
        self.release.CURRENT_RELEASE = (next_release.version, next_release.target)
        self.release.RELEASES[self.release.CURRENT_RELEASE] = next_release

    def test_v1_fixtures_pass_historical_policy_and_fail_v2(self):
        for filename in ["attestation-v1-cli-only.json", "attestation-v1-three-subjects.json"]:
            with self.subTest(fixture=filename):
                results = fixture(filename)
                self.assertTrue(v1_policy(results, self.checker, self.record))
                with self.assertRaisesRegex(ValueError, "predicate type"):
                    self.checker.check(results, self.release)

    def test_cli_only_v1_cannot_be_relabelled_v2(self):
        results = fixture("attestation-v1-cli-only.json")
        statement(results)["predicateType"] = self.checker.PREDICATE_TYPE
        with self.assertRaisesRegex(ValueError, "subjects are not exactly"):
            self.checker.check(results, self.release)

    def test_issued_v2_still_passes_after_current_release_changes(self):
        self.assertEqual(self.checker.check(attestation(self.record), self.release), 1)


if __name__ == "__main__":
    unittest.main()
