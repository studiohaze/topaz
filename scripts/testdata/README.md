# Release test inputs

The two v1 JSON files are preserved `gh attestation verify --format json`
results for Topaz 5.21.1. The CLI-only statement was signed at `d9757ace` in
run 41212135782. The three-subject statement was signed at `d552cf62` in
run 41213079276. The preserved tests check what the v1 policy accepted then
and confirm that today's v2 policy rejects both.

`attestation-v2-v5.21.1-linux-x86_64.json` is the verified result for run
41215275227 at `1c5b59925e404e95b4d67a2274586be91ec0f967`. It was retrieved
using the published CLI and the command in VERIFY.md, including its signer
digest, source ref, runner restriction and predicate type.

`attestation-v2-v5.21.2-test-only-linux-x86_64.json` is a synthetic test
fixture. It has no signature and describes no published release. Its matching
record lives only in `release_test_support.py`. It exercises adding a second
release and changing the workflow default without changing historical policy.

The two `topaz-linux-x86_64.release-core-link*.json` files are also unsigned,
synthetic inputs for that test-only release. Repeated-digit revisions are dummy
identities, not public commits. They model `topaz.release-core-link/v1`: the
product record names the executable and generated Rust, and the seed follow-up
adds `publicKitCommit`, `seedArchiveSha256` and `linkedProductRecordSha256`.
The verifier tests replace the example hashes with hashes of small generated
files and sign the checksum lists with a fresh test key. No fixture signature
is presented as a release signature or an issued attestation.

Each release owns its `checks` and `not_established` lists. The 5.21.1 v2
fixture stays unchanged. The test-only next release adds the link-record check
and two record subjects under the same v2 predicate type. A link check requires
`official-cli-links-the-public-compiler-rust` to remain in `not_established`:
checking the project's signed statement does not independently prove linking.

To add a real release, keep the existing records in `verify-topaz-release.py`,
add its complete `Release` record to `RELEASES` and select it with
`CURRENT_RELEASE`. Add its verified v2 result here as
`attestation-v2-<version>-<target>.json`. The acceptance tests enumerate the
records automatically. The verifier tests create small inputs with a fresh
Ed25519 key for each record so they reach signature, size and seed comparison
checks without downloading or executing a release CLI.

For a release with link records, also set `link_target` to the record's target
triple. Pin and bound both link files, `release-core-link.seed.checksums.txt`
and its `.sig`. The original product record must be in `checksums.txt`; the
seed follow-up must be in the separately signed seed-link list. The verifier
checks both signatures, both entries, the executable name, size and digest,
the follow-up's product-record and seed-archive digests, and the public commit
and generated Rust digest against the pinned public repository file. The
workflow attests both link files along with the original three subjects.
