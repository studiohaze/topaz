# Checking the release attestation

The workflow in `.github/workflows/verify-topaz-release.yml` checks the published
Topaz 5.21.1 Linux x86_64 release and then attests three files. They are the
uncompressed CLI, the bootstrap seed archive and
`compiler/generated/native/compiler.rs`. The predicate type is
`https://github.com/studiohaze/topaz/attestations/static-release-bytes/v2`.

The attestation covers static bytes only. It doesn't show that the CLI links the
public compiler Rust, that it behaves like the public core, or that it rebuilds
from this repository. The predicate says so in its `notEstablished` list.

## 1. Verify the signature

Pick a commit of this repository whose workflow and scripts you have reviewed,
and pin it with `--signer-digest`. Then run gh on the file you downloaded.

```sh
gh attestation verify topaz-linux-x86_64 \
  --repo studiohaze/topaz \
  --signer-workflow studiohaze/topaz/.github/workflows/verify-topaz-release.yml \
  --signer-digest <reviewed commit> \
  --source-ref refs/heads/main \
  --deny-self-hosted-runners \
  --predicate-type https://github.com/studiohaze/topaz/attestations/static-release-bytes/v2 \
  --format json > attestation.json
```

You can pass the seed archive or `compiler/generated/native/compiler.rs` in
place of the CLI. All three files are subjects of the same attestation.

## 2. Check what the statement says

gh checks who signed the statement and that your file is one of its subjects.
It doesn't check the rest of the statement. Check out the same reviewed commit
and run the policy check from it.

```sh
python3 -I scripts/check-topaz-attestation.py attestation.json
```

It accepts a result only when the type is v2, the subjects are exactly the
three expected files and digests, the verification is `static-bytes-only`, the
checks are the expected ones and the `notEstablished` list is present.

Earlier attestations used the v1 type. The first of them named only the CLI, so
the policy check rejects every v1 result.
