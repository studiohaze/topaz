"""Exercise v2 content identities through signatures, verifier and attestation."""
import hashlib
import json
import unittest
from dataclasses import replace
import test_verify_topaz_release as byte_tests
from release_test_support import attestation, load, statement


class ContentIdentityChecks(unittest.TestCase):
    release_key = ("v5.21.2", "linux-x86_64")
    write_seed = byte_tests.VerifierReachesRealChecks.write_seed
    write_link_records = byte_tests.VerifierReachesRealChecks.write_link_records
    write_json = byte_tests.VerifierReachesRealChecks.write_json
    repin = byte_tests.VerifierReachesRealChecks.repin
    sign_list = byte_tests.VerifierReachesRealChecks.sign_list

    def setUp(self):
        byte_tests.VerifierReachesRealChecks.setUp(self)
        old_seed = self.record.seed
        self.record = replace(self.record, version="v5.21.3", source_identity="sha256:" + "c" * 64,
                              pins=dict(self.record.pins), limits=dict(self.record.limits))
        (self.work / old_seed).rename(self.work / self.record.seed)
        self.record.pins[self.record.seed] = self.record.pins.pop(old_seed)
        self.record.limits[self.record.seed] = self.record.limits.pop(old_seed)
        for filename in [self.record.product_link, self.record.seed_link]:
            record = json.loads((self.work / filename).read_text(encoding="utf-8"))
            record.update(schema="topaz.release-core-link/v2", version=self.record.version,
                          productSourceRevision=self.record.source_identity)
            if filename == self.record.seed_link:
                record.update(seedSourceRevision=self.record.source_identity,
                              linkedProductRecordSha256=self.record.pins[self.record.product_link])
            self.write_json(filename, record)
            self.repin(filename)
        self.resign(self.record.product_link, "checksums.txt")
        self.resign(self.record.seed_link, self.verifier.SEED_LINK_CHECKSUMS)
        seed = {"schema": "topaz.bootstrap-seed/v2", "version": self.record.version,
                "sourceRevision": self.record.source_identity,
                "archive": {"name": self.record.seed, "size": (self.work / self.record.seed).stat().st_size,
                            "sha256": self.record.pins[self.record.seed]}}
        self.write_json("bootstrap-seed.json", seed)
        self.repin("bootstrap-seed.json")
        member = lambda name: {"name": name, "size": (self.work / name).stat().st_size, "sha256": self.record.pins[name]}
        links = [member(self.record.seed_link)]
        for target in ["linux-aarch64", "macos-x86_64", "macos-aarch64", "windows-x86_64.exe"]:
            links.append({"name": "topaz-" + target + ".release-core-link.seed.json", "size": 1, "sha256": "a" * 64})
        self.write_json("manifest.json", {"schema": "topaz.release-manifest/v6", "version": self.record.version,
            "sourceSha": self.record.source_identity, "bootstrapSeed": {"schema": seed["schema"],
            "sourceArchive": member(self.record.seed), "manifest": member("bootstrap-seed.json"), "links": links}})
        self.repin("manifest.json")
        for name in [self.record.seed, "bootstrap-seed.json", "manifest.json", self.record.seed_link]:
            self.record.limits[name] = (self.work / name).stat().st_size
            self.resign(name, "checksums.txt")
        with (self.work / "checksums.txt").open("a", encoding="ascii") as out:
            for link in links[1:]:
                out.write(f'{link["sha256"]}  {link["name"]}\n')
        self.sign_list("checksums.txt")
        self.repin("checksums.txt")
        self.repin("checksums.txt.sig")

    def resign(self, filename, list_name):
        file = self.work / list_name
        lines = [line for line in file.read_text(encoding="ascii").splitlines()
                 if line.split()[-1:] != [filename]]
        file.write_text("\n".join(lines) + f"\n{self.record.pins[filename]}  {filename}\n", encoding="ascii")
        self.sign_list(list_name)
        self.repin(list_name)
        self.repin(list_name + ".sig")

    def evidence(self):
        return self.verifier.verify(self.work, download=False, release=self.record)

    def test_signed_content_identity_and_attestation(self):
        evidence = self.evidence()
        self.assertEqual(evidence["releaseCoreLink"]["sourceIdentity"], self.record.source_identity)
        results = attestation(replace(self.record, version="v5.21.2"))
        payload = statement(results)
        payload["predicate"] = evidence
        payload["subject"] = [{"name": name, "digest": {"sha256": self.record.pins[name]}}
                              for name in [self.record.name, self.record.seed, self.record.product_link, self.record.seed_link]]
        payload["subject"].append({"name": self.verifier.PUBLIC_FILE,
                                   "digest": {"sha256": self.record.pins["compiler.rs"]}})
        self.verifier.RELEASES[(self.record.version, self.record.target)] = self.record
        checker = load("check-topaz-attestation.py")
        self.assertEqual(checker.check(results, self.verifier), 1)
        payload["predicate"]["releaseCoreLink"]["sourceIdentity"] = "sha256:" + "d" * 64
        with self.assertRaisesRegex(ValueError, "releaseCoreLink"):
            checker.check(results, self.verifier)

    def test_signed_private_revision_is_refused(self):
        filename = self.record.seed_link
        record = json.loads((self.work / filename).read_text(encoding="utf-8"))
        record["seedSourceRevision"] = "d" * 40
        self.write_json(filename, record)
        self.repin(filename)
        self.resign(filename, "checksums.txt")
        manifest = json.loads((self.work / "manifest.json").read_text(encoding="utf-8"))
        manifest["bootstrapSeed"]["links"][0].update(sha256=self.record.pins[filename], size=(self.work / filename).stat().st_size)
        self.write_json("manifest.json", manifest)
        self.repin("manifest.json")
        self.resign("manifest.json", "checksums.txt")
        with self.assertRaisesRegex(ValueError, "content identity mismatch"):
            self.evidence()

    def test_seed_repin_cannot_bypass_signed_release_list(self):
        archive = self.work / self.record.seed
        archive.write_bytes(archive.read_bytes() + b"tampered")
        self.repin(self.record.seed)
        self.record.limits[self.record.seed] = archive.stat().st_size
        with self.assertRaisesRegex(ValueError, "wrong signed record"):
            self.evidence()

    def test_missing_seed_in_signed_release_list_is_refused(self):
        sums = self.work / "checksums.txt"
        sums.write_text("\n".join(line for line in sums.read_text(encoding="ascii").splitlines()
                                 if line.split()[-1:] != [self.record.seed]) + "\n", encoding="ascii")
        self.sign_list("checksums.txt")
        self.repin("checksums.txt")
        self.repin("checksums.txt.sig")
        with self.assertRaisesRegex(ValueError, "wrong signed record"):
            self.evidence()
