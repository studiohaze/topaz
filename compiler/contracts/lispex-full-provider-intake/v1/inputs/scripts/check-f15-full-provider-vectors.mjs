import { createHash } from 'node:crypto';
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const manifestPath =
  'products/full-embed-evaluator/v1.15.8/vectors/vectors.v1.json';
const manifest = JSON.parse(readFileSync(path.join(root, manifestPath), 'utf8'));
const binary = path.join(root, 'target/debug/lispex');
const sha256 = (value) =>
  createHash('sha256').update(readFileSync(path.join(root, value))).digest('hex');
const fail = (message) => {
  throw new Error(`F15 full provider vectors failed: ${message}`);
};

if (
  manifest.schema !== 'lispex.full-provider-vectors/v1' ||
  manifest.status !== 'qualification-input' ||
  manifest.evaluator_sha256 !==
    'dd4cde2976d825ae99b542d308d87489ac96848e0710329dbb9173664d8c5ad8' ||
  manifest.positives.length !== 6 ||
  manifest.negatives.length !== 14
) {
  fail('manifest identity or cardinality drifted');
}

const expectedContracts = {
  semantic_profile_id: 'lispex/r7rs-rule-current-profile-bounded/1',
  model_id: 'lispex-full-vm-meter/1',
  abi_id: 'lispex.embed-wasm-abi/v1',
  value_codec_id: 'lispex.embed-value/v1',
  transcript_id: 'lispex.embed-transcript/v1',
  receipt_schema_id: 'lispex.embed-receipt-core/v1',
  component_id: 'lispex-evaluator/rust-vm-current-profile/1',
};
if (JSON.stringify(manifest.contracts) !== JSON.stringify(expectedContracts)) {
  fail('contract identity drifted');
}

const expectedPositive = new Map([
  ['prepared', ['prepare', 'prepared', false]],
  ['complete', ['evaluate', 'complete', true]],
  ['semantic-fault', ['evaluate', 'semantic-fault', true]],
  ['evaluation-exhausted', ['evaluate', 'exhausted', true]],
  ['preparation-exhausted', ['prepare', 'exhausted', true]],
  ['request-refusal', ['prepare', 'request-refusal', true]],
]);

for (const item of manifest.positives) {
  const expected = expectedPositive.get(item.name);
  if (!expected) fail(`unexpected positive ${item.name}`);
  const [operation, category, coreRequired] = expected;
  for (const reference of [item.artifact, item.portable_core].filter(Boolean)) {
    if (
      statSync(path.join(root, reference.path)).size !== reference.bytes ||
      sha256(reference.path) !== reference.sha256
    ) {
      fail(`${item.name} exact bytes drifted`);
    }
  }
  if (Boolean(item.portable_core) !== coreRequired) {
    fail(`${item.name} portable-core eligibility drifted`);
  }
  const result = spawnSync(
    binary,
    ['embed', 'full', 'inspect', '--artifact', item.artifact.path],
    { cwd: root, encoding: 'utf8', timeout: 30_000 }
  );
  if (result.status !== 0) fail(`${item.name} inspection failed: ${result.stderr}`);
  const projection = JSON.parse(result.stdout.trim());
  if (
    projection.kind !== operation ||
    projection.category !== category ||
    projection.portable_core_assigned !== coreRequired ||
    projection.vouch_eligible !== false ||
    projection.fallback_count !== 0
  ) {
    fail(`${item.name} projection drifted`);
  }
}
if (expectedPositive.size !== manifest.positives.length) {
  fail('positive category coverage drifted');
}

const expectedNegative = new Map([
  ['magic', ['magic-refusal', 'full artifact magic or minimum length is invalid']],
  ['truncated', ['truncation-refusal', 'full artifact envelope digest mismatch']],
  ['trailing', ['trailing-byte-refusal', 'full artifact has trailing bytes']],
  ['evaluator', ['identity-refusal', 'full artifact evaluator identity mismatch']],
  ['limit-count', ['limit-count-refusal', 'full artifact exact-limit count mismatch']],
  ['limit-value', ['exact-limit-value-mismatch-refusal', 'full artifact portable core mismatch']],
  ['identity-tag', ['identity-tag-refusal', 'full artifact identity tag is invalid']],
  ['identity-noncanonical-case', ['noncanonical-identity-refusal', 'full artifact identity is not lowercase SHA-256']],
  ['identity-mismatch', ['inconsistent-identity-refusal', 'full result is not bound to prepared identities']],
  ['request', ['request-binding-refusal', 'full artifact request digest mismatch']],
  ['response', ['response-refusal', 'embed artifact magic or minimum length is invalid']],
  ['portable-core', ['portable-core-refusal', 'full artifact portable core mismatch']],
  ['portable-core-duplicate-key', ['duplicate-core-key-refusal', 'full artifact portable core mismatch']],
  ['envelope-digest', ['envelope-digest-refusal', 'full artifact envelope digest mismatch']],
]);

for (const item of manifest.negatives) {
  const expected = expectedNegative.get(item.name);
  if (!expected || item.expectation !== expected[0]) {
    fail(`${item.name} negative taxonomy drifted`);
  }
  if (
    statSync(path.join(root, item.artifact.path)).size !== item.artifact.bytes ||
    sha256(item.artifact.path) !== item.artifact.sha256
  ) {
    fail(`${item.name} negative bytes drifted`);
  }
  const result = spawnSync(
    binary,
    ['embed', 'full', 'inspect', '--artifact', item.artifact.path],
    { cwd: root, encoding: 'utf8', timeout: 30_000 }
  );
  if (
    result.status !== 3 ||
    !result.stderr.includes('lispex embed full inspect:') ||
    !result.stderr.includes(expected[1])
  ) {
    fail(`${item.name} did not fail with its exact refusal class`);
  }
}
if (expectedNegative.size !== manifest.negatives.length) {
  fail('negative category coverage drifted');
}

console.log(
  'F15 full provider vectors passed (6 positive categories; 14 classified single-fault negatives; exact bytes and cores)'
);
