"""Release data and loaders used only by the offline acceptance tests."""

import hashlib
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
PRESERVED_RELEASE = ("v5.21.1", "linux-x86_64")


def load(filename):
    spec = importlib.util.spec_from_file_location(filename.replace("-", "_"), HERE / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fixture(name):
    return json.loads((HERE / "testdata" / name).read_text(encoding="utf-8"))


def hypothetical_release(verifier):
    """A test fixture, not a real release or proposed 5.21.2 pins."""
    version = "v5.21.2-test-only"
    name = "topaz-linux-x86_64"
    seed = f"topaz-bootstrap-seed-{version}.tar.gz"
    names = ["topaz-release.pub", "checksums.txt", "checksums.txt.sig",
             name + ".gz", name, seed, "compiler.rs"]
    return verifier.Release(
        version=version, target="linux-x86_64", commit="2" * 40,
        raw_size=8192, core_size=3072,
        pins={filename: hashlib.sha256(f"test-only {filename}".encode()).hexdigest()
              for filename in names},
        limits={"topaz-release.pub": 512, "checksums.txt": 2048,
                "checksums.txt.sig": 128, name + ".gz": 9000, seed: 4000},
    )


def release_cases(verifier):
    return [*verifier.RELEASES.values(), hypothetical_release(verifier)]


def attestation(record):
    return fixture(f"attestation-v2-{record.version}-{record.target}.json")


def statement(results):
    return results[0]["verificationResult"]["statement"]
