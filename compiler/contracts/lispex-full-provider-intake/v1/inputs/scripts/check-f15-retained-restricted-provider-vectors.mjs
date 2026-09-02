#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const manifestPath =
  'products/embed-evaluator/handoffs/lda-c1/v1/vectors/vectors.v1.json';
const manifest = JSON.parse(readFileSync(path.join(root, manifestPath), 'utf8'));
const binary = path.join(root, 'target/debug/lispex');
const sha256 = (relative) =>
  createHash('sha256')
    .update(readFileSync(path.join(root, relative)))
    .digest('hex');
const fail = (message) => {
  throw new Error(`F15 retained restricted LDA-C1 vectors failed: ${message}`);
};

if (
  manifest.schema !== 'lispex.retained-restricted-lda-c1-vectors/v1' ||
  manifest.status !== 'exact-provider-handoff-input' ||
  manifest.provider_wire_revision !==
    '451027beb8daff90c09364a1fd38a2a9f3790e55' ||
  manifest.evaluator_sha256 !==
    'fa6e52559e1f5a43e50a3b7ac0cc5add6930cff0aed8aaff462cff4609362870' ||
  manifest.positives.length !== 6 ||
  manifest.negatives.length !== 13
) {
  fail('manifest identity or cardinality drifted');
}

const expectedContracts = {
  semantic_profile_id: 'lispex/r7rs-rule-embedded-core/1',
  model_id: 'lispex-vm-meter/1',
  abi_id: 'lispex.embed-wasm-abi/v1',
  value_codec_id: 'lispex.embed-value/v1',
  transcript_id: 'lispex.embed-transcript/v1',
  receipt_schema_id: 'lispex.embed-receipt-core/v1',
  component_id: 'lispex-embed-evaluator/1.12.4',
};
if (JSON.stringify(manifest.contracts) !== JSON.stringify(expectedContracts)) {
  fail('retained contract identity drifted');
}

const expectedPositive = new Map([
  ['prepared', ['prepare', 'prepared', false]],
  ['complete', ['evaluate', 'semantic-complete', true]],
  ['semantic-fault', ['evaluate', 'semantic-fault', true]],
  ['evaluation-exhausted', ['evaluate', 'semantic-exhausted', true]],
  ['preparation-exhausted', ['prepare', 'semantic-exhausted', true]],
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
    ['embed', 'inspect', '--artifact', item.artifact.path],
    { cwd: root, encoding: 'utf8', timeout: 30_000 }
  );
  if (result.status !== 0) {
    fail(`${item.name} inspection failed: ${result.stderr}`);
  }
  const projection = JSON.parse(result.stdout.trim());
  if (
    projection.operation !== operation ||
    projection.category !== category ||
    projection.portable !== coreRequired ||
    projection.vouch_eligible !== false
  ) {
    fail(`${item.name} projection drifted`);
  }
}

const expectedNegative = new Map([
  ['magic', ['magic-refusal', 'embed artifact envelope magic or minimum length is invalid']],
  ['truncated', ['truncation-refusal', 'embed artifact field is truncated']],
  ['trailing', ['trailing-byte-refusal', 'embed artifact envelope has trailing bytes']],
  ['evaluator', ['evaluator-refusal', 'embed artifact evaluator digest mismatch']],
  ['limit-count', ['limit-count-refusal', 'embed artifact exact-limit count mismatch']],
  ['limit-value', ['exact-limit-value-mismatch-refusal', 'embed artifact portable core mismatch']],
  ['identity-tag', ['identity-tag-refusal', 'embed artifact identity tag is invalid']],
  ['identity-noncanonical-case', ['noncanonical-identity-refusal', 'embed artifact identity is not lowercase SHA-256']],
  ['identity-mismatch', ['inconsistent-identity-refusal', 'result artifact is not bound to its prepared identities']],
  ['category-binding', ['category-binding-refusal', 'embed artifact envelope and response disagree']],
  ['response', ['response-refusal', 'embed artifact magic or minimum length is invalid']],
  ['portable-core', ['portable-core-refusal', 'embed artifact portable core mismatch']],
  ['portable-core-duplicate-key', ['duplicate-core-key-refusal', 'embed artifact portable core mismatch']],
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
    ['embed', 'inspect', '--artifact', item.artifact.path],
    { cwd: root, encoding: 'utf8', timeout: 30_000 }
  );
  if (
    result.status !== 3 ||
    !result.stderr.includes('lispex embed inspect:') ||
    !result.stderr.includes(expected[1])
  ) {
    fail(`${item.name} did not fail with its exact refusal class`);
  }
}
if (expectedNegative.size !== manifest.negatives.length) {
  fail('negative category coverage drifted');
}

console.log(
  'F15 retained restricted LDA-C1 vectors passed (6 positive categories; 13 classified single-fault negatives; exact artifact and core bytes)'
);
