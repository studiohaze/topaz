# Checking the release attestation

The workflow in `.github/workflows/verify-topaz-release.yml` checks the published
Topaz 5.21.2 Linux x86_64 release and then attests five files. They are the
uncompressed CLI, the bootstrap seed archive,
`compiler/generated/native/compiler.rs`, and the executable's product and seed
link records. The verifier keeps a separate record for each release, including
the three-file 5.21.1 contract. The predicate type is
`https://github.com/studiohaze/topaz/attestations/static-release-bytes/v2`.

The attestation checks static bytes and the signed link records. It doesn't show that the CLI links the
public compiler Rust, that it behaves like the public core, or that it rebuilds
from this repository. The predicate says so in its `notEstablished` list.

## 1. Verify the signature

`--signer-digest` is the commit the workflow ran at, so pin a commit that issued
an attestation for the version you downloaded. The issued 5.21.1 v2 attestation came from run
[41215275227](https://github.com/studiohaze/topaz/actions/runs/41215275227) at
commit `1c5b59925e404e95b4d67a2274586be91ec0f967`. Review the workflow and
scripts at that commit. Compare a later reviewed commit to see which checks
changed. The current policy checker retains the 5.21.1 contract.

```sh
git diff 1c5b59925e404e95b4d67a2274586be91ec0f967 <reviewed commit> -- \
  .github/workflows/verify-topaz-release.yml scripts/
```

An empty diff means both commits run the same checks. The following example
verifies a file from 5.21.1 using its issuing commit.

```sh
gh attestation verify topaz-linux-x86_64 \
  --repo studiohaze/topaz \
  --signer-workflow studiohaze/topaz/.github/workflows/verify-topaz-release.yml \
  --signer-digest 1c5b59925e404e95b4d67a2274586be91ec0f967 \
  --source-ref refs/heads/main \
  --deny-self-hosted-runners \
  --predicate-type https://github.com/studiohaze/topaz/attestations/static-release-bytes/v2 \
  --format json > attestation.json
```

You can pass the 5.21.1 seed archive or `compiler/generated/native/compiler.rs`
in place of the CLI. All three files are subjects of that attestation.

## 2. Check what the statement says

gh checks who signed the statement and that your file is one of its subjects.
It doesn't check the rest of the statement. Run the policy check from the commit
you reviewed.

```sh
python3 -I scripts/check-topaz-attestation.py attestation.json
```

It accepts a result only when the type is v2, the subjects are exactly the
expected files and digests for that release, the verification is `static-bytes-only`, the
checks are the expected ones and the `notEstablished` list is present.

For 5.21.2, the expected subjects also include
`topaz-linux-x86_64.release-core-link.json` and
`topaz-linux-x86_64.release-core-link.seed.json`. The verifier checks both
Ed25519-signed checksum lists, the executable's name, size and digest, the
public source commit, and the generated compiler and seed archive digests.
It preserves `official-cli-links-the-public-compiler-rust` in `notEstablished`.

You can run these checks directly before using an attestation.

```sh
python3 -I scripts/verify-topaz-release.py
```

The command downloads the pinned 5.21.2 public inputs and prints the verified
record as JSON. It does not execute the downloaded compiler.

Earlier attestations used the v1 type. The first of them named only the CLI, so
the policy check rejects every v1 result.

## Issued attestations

| Release | Run | Commit | Type | Subjects |
| --- | --- | --- | --- | --- |
| 5.21.1 | [41215275227](https://github.com/studiohaze/topaz/actions/runs/41215275227) | `1c5b599` | v2 | CLI, seed archive, `compiler.rs` |
| 5.21.1 | [41213079276](https://github.com/studiohaze/topaz/actions/runs/41213079276) | `d552cf6` | v1 | CLI, seed archive, `compiler.rs` |
| 5.21.1 | [41212135782](https://github.com/studiohaze/topaz/actions/runs/41212135782) | `d9757ac` | v1 | CLI |
