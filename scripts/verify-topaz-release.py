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
from pathlib import Path

VERSION = "v5.21.1"
TARGET = "linux-x86_64"
COMMIT = "05965dfc878c1fd581a203d38abf16c729d23abc"
NAME = "topaz-" + TARGET
SEED = f"topaz-bootstrap-seed-{VERSION}.tar.gz"
MEMBER = "topaz-public/generated/compiler.rs"
RAW_SIZE = 109085408
CORE_SIZE = 13077120
SITE = f"https://topaz.ooo/dl/{VERSION}"
PUBLIC_FILE = "compiler/generated/native/compiler.rs"
RAW_URL = f"https://raw.githubusercontent.com/studiohaze/topaz/{COMMIT}/{PUBLIC_FILE}"

# PEM hash is over the exact published PEM bytes. The seed pin is separate
# from the publisher-signed binary checksum list. All pins refer to this release.
PINS = {
    "topaz-release.pub": "796cd56440798d10d74f8bb98c4d89302bd20b7258469bd397fbdde5c6ccd0a4",
    "checksums.txt": "e0837f8a7301243062e9fed559438232d6ab5672d77bc551641892542e0343f9",
    "checksums.txt.sig": "768d1e01e06dcbe5d771b7079038e88f63d9a5f17b327e379fc54f3ea15ad64c",
    NAME + ".gz": "632df3fa2dbc0a390d2401c9d429b51d92ae3094e7265d370dcf6420f52cbb2c",
    NAME: "1f0611b39ce9b0ce044b1b8d0d13c888b74409d547bdae824fd47b4ab3a5468d",
    SEED: "cf6a09b66fa5197472808fc01388e799b394686fb705b8e977cc39962583e974",
    "compiler.rs": "5f32cee07a1adea1c67ba8ac8fbd4f3ee4051f019a7856fcdb228740a1d4b8b1",
}
INPUTS = {
    "topaz-release.pub": ("https://topaz.ooo/dl/topaz-release.pub", 4096),
    "checksums.txt": (f"{SITE}/checksums.txt", 65536),
    "checksums.txt.sig": (f"{SITE}/checksums.txt.sig", 1024),
    NAME + ".gz": (f"{SITE}/{NAME}.gz", 41839790),
    SEED: (f"{SITE}/{SEED}", 1926265),
    "compiler.rs": (RAW_URL, CORE_SIZE),
}


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


def verify(directory, download):
    for name, (url, limit) in INPUTS.items():
        path = directory / name
        if download:
            with urllib.request.urlopen(url, timeout=60) as source, path.open("xb") as output:
                digest, _ = digest_stream(source, limit, output)
        else:
            with path.open("rb") as source:
                digest, _ = digest_stream(source, limit)
        require(digest == PINS[name], f"SHA-256 mismatch: {name}")

    subprocess.run(
        ["openssl", "pkeyutl", "-verify", "-pubin", "-inkey", str(directory / "topaz-release.pub"),
         "-rawin", "-in", str(directory / "checksums.txt"), "-sigfile", str(directory / "checksums.txt.sig")],
        check=True, stdout=subprocess.DEVNULL,
    )
    signed_lines = (directory / "checksums.txt").read_text(encoding="ascii").splitlines()
    for name in [NAME + ".gz", NAME]:
        records = [line.split() for line in signed_lines if line.split()[-1:] == [name]]
        require(records == [[PINS[name], name]], f"missing, duplicate or wrong signed record: {name}")

    with gzip.open(directory / (NAME + ".gz"), "rb") as cli:
        digest, size = digest_stream(cli, RAW_SIZE)
    require(digest == PINS[NAME] and size == RAW_SIZE, "decompressed CLI mismatch")

    # Read a single exact regular-file member. No archive path is written to disk.
    with tarfile.open(directory / SEED, "r:gz") as seed:
        members = [m for m in seed if m.name == MEMBER]
        require(len(members) == 1 and members[0].isfile() and members[0].size == CORE_SIZE,
                "missing, duplicated or invalid compiler member")
        with seed.extractfile(members[0]) as from_seed, (directory / "compiler.rs").open("rb") as from_git:
            while chunk := from_seed.read(1024 * 1024):
                require(chunk == from_git.read(len(chunk)), "seed and public compiler Rust differ")
            require(not from_git.read(1), "public compiler Rust has extra bytes")

    return {
        "version": VERSION, "target": TARGET, "verification": "static-bytes-only",
        "subject": {"name": NAME, "sha256": digest, "bytes": size},
        "officialBinary": {"url": f"{SITE}/{NAME}", "compressedUrl": INPUTS[NAME + ".gz"][0],
                           "compressedSha256": PINS[NAME + ".gz"],
                           "signedChecksumFile": INPUTS["checksums.txt"][0],
                           "checksumsSha256": PINS["checksums.txt"],
                           "signatureUrl": INPUTS["checksums.txt.sig"][0],
                           "signatureSha256": PINS["checksums.txt.sig"],
                           "publicKeyUrl": INPUTS["topaz-release.pub"][0],
                           "publicKeyPemSha256": PINS["topaz-release.pub"], "signatureAlgorithm": "Ed25519"},
        "compilerRust": {"seedUrl": INPUTS[SEED][0], "seedArchiveSha256": PINS[SEED],
                         "seedTrust": "release-specific digest pinned in verifier",
                         "member": MEMBER, "publicRepository": "studiohaze/topaz",
                         "commit": COMMIT, "publicPath": PUBLIC_FILE, "publicUrl": RAW_URL,
                         "bytes": CORE_SIZE, "sha256": PINS["compiler.rs"], "byteComparison": "equal"},
        "checks": ["official-binary-compressed-and-uncompressed-match-signed-list",
                   "public-seed-compiler-rust-equals-pinned-public-repository-file"],
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
