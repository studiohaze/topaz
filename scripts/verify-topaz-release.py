#!/usr/bin/env python3
"""Verify public release bytes without running Topaz. Only a successful run prints JSON."""

import argparse
import gzip
import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path

MEMBER = "topaz-public/generated/compiler.rs"
PUBLIC_FILE = "compiler/generated/native/compiler.rs"
LINK_RECORD_CHECK = "signed-release-core-links-match-executable-and-public-compiler"
LINK_NOT_ESTABLISHED = "official-cli-links-the-public-compiler-rust"
SEED_LINK_CHECKSUMS = "release-core-link.seed.checksums.txt"


@dataclass(frozen=True)
class Release:
    version: str
    target: str
    commit: str
    raw_size: int
    core_size: int
    pins: dict
    limits: dict
    checks: tuple
    not_established: tuple
    link_target: str | None = None

    @property
    def has_link_check(self):
        return LINK_RECORD_CHECK in self.checks

    @property
    def product_link(self):
        return self.name + ".release-core-link.json"

    @property
    def seed_link(self):
        return self.name + ".release-core-link.seed.json"

    def validate_policy(self):
        if self.has_link_check:
            require(LINK_NOT_ESTABLISHED in self.not_established,
                    "link-record check must keep official-cli-links-the-public-compiler-rust in notEstablished")
            require(bool(self.link_target), "link-record check requires a link target")
        else:
            require(self.link_target is None, "link target requires a link-record check")

    def link_evidence(self):
        return {
            "productRecord": {"name": self.product_link, "sha256": self.pins[self.product_link]},
            "seedRecord": {"name": self.seed_link, "sha256": self.pins[self.seed_link]},
            "publicCommit": self.commit,
            "compilerRustSha256": self.pins["compiler.rs"],
            "executableSha256": self.pins[self.name],
        }

    @property
    def name(self):
        return "topaz-" + self.target

    @property
    def seed(self):
        return f"topaz-bootstrap-seed-{self.version}.tar.gz"

    @property
    def site(self):
        return f"https://topaz.ooo/dl/{self.version}"

    @property
    def raw_url(self):
        return f"https://raw.githubusercontent.com/studiohaze/topaz/{self.commit}/{PUBLIC_FILE}"

    @property
    def inputs(self):
        urls = {name: f"{self.site}/{name}" for name in self.limits}
        urls["topaz-release.pub"] = "https://topaz.ooo/dl/topaz-release.pub"
        return {name: (url, self.limits[name]) for name, url in urls.items()} | {
            "compiler.rs": (self.raw_url, self.core_size),
        }


# Keep issued releases here when CURRENT_RELEASE advances. PEM hashes cover
# exact published PEM bytes; seed pins are separate from signed CLI checksums.
RELEASES = {
    ("v5.21.1", "linux-x86_64"): Release(
        version="v5.21.1",
        target="linux-x86_64",
        commit="05965dfc878c1fd581a203d38abf16c729d23abc",
        raw_size=109085408,
        core_size=13077120,
        checks=("official-binary-compressed-and-uncompressed-match-signed-list",
                "public-seed-compiler-rust-equals-pinned-public-repository-file"),
        not_established=("official-cli-links-the-public-compiler-rust",
                         "official-cli-behaves-like-the-public-core",
                         "official-cli-rebuilds-from-this-repository"),
        pins={
            "topaz-release.pub": "796cd56440798d10d74f8bb98c4d89302bd20b7258469bd397fbdde5c6ccd0a4",
            "checksums.txt": "e0837f8a7301243062e9fed559438232d6ab5672d77bc551641892542e0343f9",
            "checksums.txt.sig": "768d1e01e06dcbe5d771b7079038e88f63d9a5f17b327e379fc54f3ea15ad64c",
            "topaz-linux-x86_64.gz": "632df3fa2dbc0a390d2401c9d429b51d92ae3094e7265d370dcf6420f52cbb2c",
            "topaz-linux-x86_64": "1f0611b39ce9b0ce044b1b8d0d13c888b74409d547bdae824fd47b4ab3a5468d",
            "topaz-bootstrap-seed-v5.21.1.tar.gz": "cf6a09b66fa5197472808fc01388e799b394686fb705b8e977cc39962583e974",
            "compiler.rs": "5f32cee07a1adea1c67ba8ac8fbd4f3ee4051f019a7856fcdb228740a1d4b8b1",
        },
        limits={
            "topaz-release.pub": 4096,
            "checksums.txt": 65536,
            "checksums.txt.sig": 1024,
            "topaz-linux-x86_64.gz": 41839790,
            "topaz-bootstrap-seed-v5.21.1.tar.gz": 1926265,
        },
    ),
}
CURRENT_RELEASE = ("v5.21.1", "linux-x86_64")


def require(ok, message):
    if not ok:
        raise ValueError(message)


def digest_stream(stream, limit, destination=None):
    digest = hashlib.sha256()
    size = 0
    while block := stream.read(1024 * 1024):
        size += len(block)
        require(size <= limit, f"input exceeds {limit} bytes")
        digest.update(block)
        if destination is not None:
            destination.write(block)
    return digest.hexdigest(), size


def verify_signed_list(directory, filename):
    subprocess.run(
        ["openssl", "pkeyutl", "-verify", "-pubin", "-inkey", str(directory / "topaz-release.pub"),
         "-rawin", "-in", str(directory / filename), "-sigfile", str(directory / (filename + ".sig"))],
        check=True, stdout=subprocess.DEVNULL,
    )
    return (directory / filename).read_text(encoding="ascii").splitlines()


def verify_list_entry(lines, filename, digest):
    records = [line.split() for line in lines if line.split()[-1:] == [filename]]
    require(records == [[digest, filename]], f"missing, duplicate or wrong signed record: {filename}")


def verify_links(directory, release, signed_lines, cli_digest, cli_size):
    verify_list_entry(signed_lines, release.product_link, release.pins[release.product_link])
    seed_lines = verify_signed_list(directory, SEED_LINK_CHECKSUMS)
    verify_list_entry(seed_lines, release.seed_link, release.pins[release.seed_link])
    product = json.loads((directory / release.product_link).read_text(encoding="utf-8"))
    followup = json.loads((directory / release.seed_link).read_text(encoding="utf-8"))
    for record in [product, followup]:
        require(record.get("schema") == "topaz.release-core-link/v1", "link record schema mismatch")
        require(record.get("version") == release.version and record.get("target") == release.link_target,
                "link record release or target mismatch")
        require(record.get("subject") == {"name": release.name, "sha256": cli_digest, "bytes": cli_size},
                "link record executable bytes or SHA-256 mismatch")
        require(record.get("generatedRustSha256") == release.pins["compiler.rs"],
                "link record compiler.rs SHA-256 mismatch with public repository")
    require(product.get("publicKitCommit") is None and product.get("seedArchiveSha256") is None,
            "product link must precede seed publication")
    require(followup.get("publicKitCommit") == release.commit,
            "link record public commit mismatch with pinned public repository commit")
    require(followup.get("seedArchiveSha256") == release.pins[release.seed],
            "link record seed archive SHA-256 mismatch")
    require(followup.get("linkedProductRecordSha256") == release.pins[release.product_link],
            "seed link does not match signed product link")
    # The follow-up extends the signed product record; it cannot rewrite it.
    for key, value in product.items():
        if key not in {"seedSourceRevision", "seedArchiveSha256", "publicKitCommit"}:
            require(followup.get(key) == value, f"seed link changed product field: {key}")
    return release.link_evidence()


def verify(directory, download, release=None):
    if release is None:
        release = RELEASES[CURRENT_RELEASE]
    release.validate_policy()
    pins, inputs = release.pins, release.inputs
    name, seed_name = release.name, release.seed
    for filename, (url, limit) in inputs.items():
        path = directory / filename
        if download:
            with urllib.request.urlopen(url, timeout=60) as source, path.open("xb") as output:
                digest, _ = digest_stream(source, limit, output)
        else:
            with path.open("rb") as source:
                digest, _ = digest_stream(source, limit)
        require(digest == pins[filename], f"SHA-256 mismatch: {filename}")

    signed_lines = verify_signed_list(directory, "checksums.txt")
    for filename in [name + ".gz", name]:
        verify_list_entry(signed_lines, filename, pins[filename])

    with gzip.open(directory / (name + ".gz"), "rb") as cli:
        digest, size = digest_stream(cli, release.raw_size)
    require(digest == pins[name] and size == release.raw_size, "decompressed CLI mismatch")

    # Read a single exact regular-file member. No archive path is written to disk.
    with tarfile.open(directory / seed_name, "r:gz") as seed:
        members = [m for m in seed if m.name == MEMBER]
        require(len(members) == 1 and members[0].isfile() and members[0].size == release.core_size,
                "missing, duplicated or invalid compiler member")
        with seed.extractfile(members[0]) as from_seed, (directory / "compiler.rs").open("rb") as from_git:
            while chunk := from_seed.read(1024 * 1024):
                require(chunk == from_git.read(len(chunk)), "seed and public compiler Rust differ")
            require(not from_git.read(1), "public compiler Rust has extra bytes")

    evidence = {
        "version": release.version, "target": release.target, "verification": "static-bytes-only",
        "subject": {"name": name, "sha256": digest, "bytes": size},
        "officialBinary": {"url": f"{release.site}/{name}", "compressedUrl": inputs[name + ".gz"][0],
                           "compressedSha256": pins[name + ".gz"],
                           "signedChecksumFile": inputs["checksums.txt"][0],
                           "checksumsSha256": pins["checksums.txt"],
                           "signatureUrl": inputs["checksums.txt.sig"][0],
                           "signatureSha256": pins["checksums.txt.sig"],
                           "publicKeyUrl": inputs["topaz-release.pub"][0],
                           "publicKeyPemSha256": pins["topaz-release.pub"], "signatureAlgorithm": "Ed25519"},
        "compilerRust": {"seedUrl": inputs[seed_name][0], "seedArchiveSha256": pins[seed_name],
                         "seedTrust": "release-specific digest pinned in verifier",
                         "member": MEMBER, "publicRepository": "studiohaze/topaz",
                         "commit": release.commit, "publicPath": PUBLIC_FILE, "publicUrl": release.raw_url,
                         "bytes": release.core_size, "sha256": pins["compiler.rs"], "byteComparison": "equal"},
        "checks": list(release.checks),
        "notEstablished": list(release.not_established),
    }
    if release.has_link_check:
        evidence["releaseCoreLink"] = verify_links(directory, release, signed_lines, digest, size)
    return evidence


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, help="read the selected release's inputs from this directory offline")
    args = parser.parse_args()
    try:
        if args.inputs is not None:
            evidence = verify(args.inputs, download=False)
        else:
            with tempfile.TemporaryDirectory(prefix="topaz-release-") as work:
                evidence = verify(Path(work), download=True)
        print(json.dumps(evidence, sort_keys=True, separators=(",", ":")))
    except (ValueError, OSError, EOFError, tarfile.TarError, subprocess.CalledProcessError) as error:
        sys.exit(f"verification failed: {error}")
