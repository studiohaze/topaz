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


@dataclass(frozen=True)
class Release:
    version: str
    target: str
    commit: str
    raw_size: int
    core_size: int
    pins: dict
    limits: dict

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
# The two checks relate published bytes to each other. They say nothing about
# what the CLI executes, so the attestation says so explicitly.
CHECKS = ["official-binary-compressed-and-uncompressed-match-signed-list",
          "public-seed-compiler-rust-equals-pinned-public-repository-file"]
NOT_ESTABLISHED = ["official-cli-links-the-public-compiler-rust",
                   "official-cli-behaves-like-the-public-core",
                   "official-cli-rebuilds-from-this-repository"]


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


def verify(directory, download, release=None):
    if release is None:
        release = RELEASES[CURRENT_RELEASE]
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

    subprocess.run(
        ["openssl", "pkeyutl", "-verify", "-pubin", "-inkey", str(directory / "topaz-release.pub"),
         "-rawin", "-in", str(directory / "checksums.txt"), "-sigfile", str(directory / "checksums.txt.sig")],
        check=True, stdout=subprocess.DEVNULL,
    )
    signed_lines = (directory / "checksums.txt").read_text(encoding="ascii").splitlines()
    for filename in [name + ".gz", name]:
        records = [line.split() for line in signed_lines if line.split()[-1:] == [filename]]
        require(records == [[pins[filename], filename]], f"missing, duplicate or wrong signed record: {filename}")

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

    return {
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
        "checks": list(CHECKS),
        "notEstablished": list(NOT_ESTABLISHED),
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, help="read the six inputs from this directory offline")
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
