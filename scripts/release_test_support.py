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
             name + ".gz", name, seed, "compiler.rs",
             name + ".release-core-link.json", name + ".release-core-link.seed.json",
             verifier.SEED_LINK_CHECKSUMS, verifier.SEED_LINK_CHECKSUMS + ".sig"]
    pins = {filename: hashlib.sha256(f"test-only {filename}".encode()).hexdigest()
            for filename in names}
    for suffix in [".release-core-link.json", ".release-core-link.seed.json"]:
        pins[name + suffix] = hashlib.sha256((HERE / "testdata" / (name + suffix)).read_bytes()).hexdigest()
    return verifier.Release(
        version=version, target="linux-x86_64", commit="2" * 40,
        raw_size=8192, core_size=3072,
        pins=pins,
        limits={"topaz-release.pub": 512, "checksums.txt": 2048,
                "checksums.txt.sig": 128, name + ".gz": 9000, seed: 4000,
                name + ".release-core-link.json": 4096,
                name + ".release-core-link.seed.json": 4096,
                verifier.SEED_LINK_CHECKSUMS: 2048, verifier.SEED_LINK_CHECKSUMS + ".sig": 128},
        checks=("official-binary-compressed-and-uncompressed-match-signed-list",
                "public-seed-compiler-rust-equals-pinned-public-repository-file",
                verifier.LINK_RECORD_CHECK),
        not_established=("official-cli-links-the-public-compiler-rust",
                         "official-cli-behaves-like-the-public-core",
                         "official-cli-rebuilds-from-this-repository"),
        link_target="x86_64-unknown-linux-gnu",
    )


def release_cases(verifier):
    return [*verifier.RELEASES.values(), hypothetical_release(verifier)]


def attestation(record):
    return fixture(f"attestation-v2-{record.version}-{record.target}.json")


def statement(results):
    return results[0]["verificationResult"]["statement"]
