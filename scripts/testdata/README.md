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

To add a real release, keep the existing records in `verify-topaz-release.py`,
add its complete `Release` record to `RELEASES` and select it with
`CURRENT_RELEASE`. Add its verified v2 result here as
`attestation-v2-<version>-<target>.json`. The acceptance tests enumerate the
records automatically. The verifier tests create small inputs with a fresh
Ed25519 key for each record so they reach signature, size and seed comparison
checks without downloading or executing a release CLI.
