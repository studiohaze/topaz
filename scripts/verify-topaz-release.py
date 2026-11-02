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


"""Public content identities for release records from Topaz 5.21.3."""
import re


def content_identity_release(version):
    match = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)", version)
    return bool(match and tuple(map(int, match.groups())) >= (5, 21, 3))


def verify_public_link_sources(product, followup, release):
    if not content_identity_release(release.version):
        return
    expected = release.source_identity
    if not isinstance(expected, str) or not re.fullmatch(r"sha256:[a-f0-9]{64}", expected):
        raise ValueError("release requires its pinned public content identity")
    if product.get("productSourceRevision") != expected or followup.get("seedSourceRevision") != expected:
        raise ValueError("link record public content identity mismatch")
    def visit(item):
        if isinstance(item, str):
            for commit in re.findall(r"(?<![a-f0-9])[a-f0-9]{40}(?![a-f0-9])", item):
                if commit != release.commit:
                    raise ValueError("link record contains a non-public commit identifier")
        elif isinstance(item, dict):
            for value in item.values():
                visit(value)
        elif isinstance(item, list):
            for value in item:
                visit(value)
    visit(product)
    visit(followup)


def verify_release_seed(directory, release, signed_lines):
    if not content_identity_release(release.version):
        return
    import hashlib
    import json
    names = [release.seed, "bootstrap-seed.json", "manifest.json", release.seed_link]
    for name in names:
        digest = hashlib.sha256((directory / name).read_bytes()).hexdigest()
        records = [line.split() for line in signed_lines if line.split()[-1:] == [name]]
        if records != [[digest, name]]:
            raise ValueError(f"missing, duplicate or wrong signed record: {name}")
    manifest = json.loads((directory / "manifest.json").read_text(encoding="utf-8"))
    seed = json.loads((directory / "bootstrap-seed.json").read_text(encoding="utf-8"))
    descriptor = manifest.get("bootstrapSeed", {})
    members = [descriptor.get("sourceArchive", {}), descriptor.get("manifest", {}), *descriptor.get("links", [])]
    expected_links = {"topaz-" + target + ".release-core-link.seed.json" for target in
                      ["linux-x86_64", "linux-aarch64", "macos-x86_64", "macos-aarch64", "windows-x86_64.exe"]}
    if (manifest.get("schema") != "topaz.release-manifest/v6" or manifest.get("version") != release.version
            or manifest.get("sourceSha") != release.source_identity
            or descriptor.get("schema") != "topaz.bootstrap-seed/v2"
            or len(members) != 7 or {m.get("name") for m in members[2:]} != expected_links
            or members[0].get("name") != release.seed or members[1].get("name") != "bootstrap-seed.json"):
        raise ValueError("release manifest seed inventory differs")
    for member in members:
        records = [line.split() for line in signed_lines if line.split()[-1:] == [member.get("name")]]
        if records != [[member.get("sha256"), member.get("name")]]:
            raise ValueError("manifest seed member absent from signed list")
        if member["name"] in names:
            data = (directory / member["name"]).read_bytes()
            if len(data) != member.get("size") or hashlib.sha256(data).hexdigest() != member.get("sha256"):
                raise ValueError("manifest seed member bytes differ")
    if (seed.get("schema") != "topaz.bootstrap-seed/v2" or seed.get("version") != release.version
            or seed.get("sourceRevision") != release.source_identity
            or seed.get("archive", {}).get("name") != release.seed
            or seed["archive"].get("sha256") != members[0]["sha256"]
            or seed["archive"].get("size") != members[0]["size"]):
        raise ValueError("seed provenance differs from signed release manifest")


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
    source_identity: str | None = None

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
            **({"sourceIdentity": self.source_identity} if content_identity_release(self.version) else {}),
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
        urls = {name: f"{self.site}/{name}" for name in self.limits if not (content_identity_release(self.version) and name in {SEED_LINK_CHECKSUMS, SEED_LINK_CHECKSUMS + ".sig"})}
        urls["topaz-release.pub"] = "https://topaz.ooo/dl/topaz-release.pub"
        return {name: (url, self.limits[name]) for name, url in urls.items()} | {
            "compiler.rs": (self.raw_url, self.core_size),
        }


# Issued release identities and public-key fingerprints are release data.
def load_release_data():
    data = json.loads((Path(__file__).parent / "topaz-releases.json").read_text(encoding="utf-8"))
    if data.get("schema") != "topaz.public-release-verifier-data/v1":
        raise ValueError("unsupported release verifier data")
    releases = {}
    for fields in data["releases"]:
        fields = dict(fields)
        fields["checks"] = tuple(fields["checks"])
        fields["not_established"] = tuple(fields["not_established"])
        release = Release(**fields)
        key = (release.version, release.target)
        if key in releases or release.pins["topaz-release.pub"] not in data["publicKeys"]:
            raise ValueError("duplicate release or unregistered release public key")
        releases[key] = release
    current = tuple(data["currentRelease"])
    if current not in releases:
        raise ValueError("current release is absent from release data")
    return releases, current


RELEASES, CURRENT_RELEASE = load_release_data()


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
    seed_lines = signed_lines if content_identity_release(release.version) else verify_signed_list(directory, SEED_LINK_CHECKSUMS)
    verify_list_entry(seed_lines, release.seed_link, release.pins[release.seed_link])
    product = json.loads((directory / release.product_link).read_text(encoding="utf-8"))
    followup = json.loads((directory / release.seed_link).read_text(encoding="utf-8"))
    for record in [product, followup]:
        require(record.get("schema") == ("topaz.release-core-link/v2" if content_identity_release(release.version) else "topaz.release-core-link/v1"), "link record schema mismatch")
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
    verify_public_link_sources(product, followup, release)
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

    verify_release_seed(directory, release, signed_lines)

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
                         "seedTrust": ("signed release checksum list" if content_identity_release(release.version) else "release-specific digest pinned in verifier"),
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
