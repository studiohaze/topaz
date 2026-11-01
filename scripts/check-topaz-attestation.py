#!/usr/bin/env python3
"""Check a verified release attestation against the v2 contract.

Run gh attestation verify with --format json first. gh checks the signature,
the repository and the signer. This script then checks what the signed
statement says. It exits 0 only when every result matches the contract.
"""

import argparse
import importlib.util
import json
import sys
from pathlib import Path

PREDICATE_TYPE = "https://github.com/studiohaze/topaz/attestations/static-release-bytes/v2"
REPOSITORY = "https://github.com/studiohaze/topaz"
SIGNER = REPOSITORY + "/.github/workflows/verify-topaz-release.yml@refs/heads/main"


def load_release():
    path = Path(__file__).resolve().with_name("verify-topaz-release.py")
    spec = importlib.util.spec_from_file_location("verify_topaz_release", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def v2_policy(release, record):
    return {
        "predicateType": PREDICATE_TYPE,
        "cli": (record.name, record.pins[record.name]),
        "seed": (record.seed, record.pins[record.seed]),
        "compilerRust": (release.PUBLIC_FILE, record.pins["compiler.rs"]),
        "version": record.version,
        "target": record.target,
        "checks": list(release.CHECKS),
        "notEstablished": list(release.NOT_ESTABLISHED),
    }


def require(ok, message):
    if not ok:
        raise ValueError(message)


def check_result(result, release):
    certificate = result["verificationResult"]["signature"]["certificate"]
    require(certificate.get("sourceRepositoryURI") == REPOSITORY, "signed in another repository")
    require(certificate.get("buildSignerURI") == SIGNER, "signed by another workflow or ref")

    statement = result["verificationResult"]["statement"]
    require(statement.get("predicateType") == PREDICATE_TYPE,
            f"predicate type is {statement.get('predicateType')!r}")

    predicate = statement.get("predicate", {})
    key = (predicate.get("version"), predicate.get("target"))
    require(key in release.RELEASES, f"unknown release version or target: {key!r}")
    policy = v2_policy(release, release.RELEASES[key])

    subjects = [(s.get("name"), s.get("digest", {}).get("sha256")) for s in statement.get("subject", [])]
    expected = [policy["cli"], policy["seed"], policy["compilerRust"]]
    require(sorted(subjects) == sorted(expected),
            "subjects are not exactly the three verified files")

    require(predicate.get("verification") == "static-bytes-only", "verification is not static-bytes-only")
    require(predicate.get("version") == policy["version"] and predicate.get("target") == policy["target"],
            "wrong release version or target")
    require(predicate.get("checks") == policy["checks"], "unexpected checks")
    require(predicate.get("notEstablished") == policy["notEstablished"],
            "notEstablished is missing or changed")

    # The predicate must describe the same bytes the subjects name.
    compiler = predicate.get("compilerRust", {})
    require(predicate.get("subject", {}).get("sha256") == policy["cli"][1]
            and compiler.get("seedArchiveSha256") == policy["seed"][1]
            and compiler.get("sha256") == policy["compilerRust"][1],
            "predicate digests do not match the subjects")


def check(results, release=None):
    require(isinstance(results, list) and results, "no verified attestation results")
    if release is None:
        release = load_release()
    for result in results:
        check_result(result, release)
    return len(results)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("results", type=Path, help="output of gh attestation verify --format json")
    args = parser.parse_args()
    try:
        results = json.loads(args.results.read_text(encoding="utf-8"))
        count = check(results)
    except (ValueError, KeyError, TypeError, AttributeError, OSError) as error:
        sys.exit(f"attestation rejected: {error}")
    print(f"accepted {count} attestation(s) under {PREDICATE_TYPE}")
