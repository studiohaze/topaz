#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const generatedPath = path.join(
  root,
  'interp/src/full_primitive_meter_generated.rs'
);
const classification = JSON.parse(
  readFileSync(
    path.join(
      root,
      'docs/bounded-execution/full-profile-cost-classification.v0.json'
    ),
    'utf8'
  )
);
const baseline = readFileSync(generatedPath, 'utf8');

function fail(message) {
  throw new Error(`F15 full provider mutation court failed: ${message}`);
}

function validateRuntimeSites(source) {
  const sites = [...source.matchAll(/meter_site_id: "([^"]+)",/gu)].map(
    (match) => match[1]
  );
  if (
    sites.length !== classification.rows.length ||
    sites.some(
      (site, index) => site !== classification.rows[index].meter_site_id
    )
  ) {
    throw new Error('E_F15_RUNTIME_SITE_IDENTITY');
  }
}

validateRuntimeSites(baseline);
let detectedRowMutations = 0;
for (const row of classification.rows) {
  const needle = `meter_site_id: "${row.meter_site_id}",`;
  const replacement = `meter_site_id: "primitive-mutant/${String(row.id).padStart(3, '0')}",`;
  const mutated = baseline.replace(needle, replacement);
  if (mutated === baseline) fail(`row ${row.id} has no generated meter site`);
  try {
    validateRuntimeSites(mutated);
  } catch (error) {
    if (error.message === 'E_F15_RUNTIME_SITE_IDENTITY') {
      detectedRowMutations += 1;
      continue;
    }
    throw error;
  }
  fail(`row mutation survived: ${row.id}`);
}

const structuralCourts = [
  ['scripts/check-f15-full-profile-surface.mjs', 8],
  ['scripts/check-f15-full-primitive-registry.mjs', 9],
  ['scripts/check-f15-full-profile-cost.mjs', 13],
  ['scripts/check-f15-full-guest-control.mjs', 18],
];
let detectedStructuralMutations = 0;
for (const [script, expected] of structuralCourts) {
  const result = spawnSync(process.execPath, [script], {
    cwd: root,
    encoding: 'utf8',
    timeout: 60_000,
  });
  if (result.status !== 0 || !result.stdout.includes(`${expected} `)) {
    fail(`${script} did not detect its ${expected} declared mutations`);
  }
  detectedStructuralMutations += expected;
}

const generated = detectedRowMutations + detectedStructuralMutations;
if (detectedRowMutations !== 205 || generated !== 253) {
  fail('mutation denominator drifted');
}

console.log(
  `F15 full provider mutation court passed (${detectedRowMutations}/205 runtime-row identity mutations; ${detectedStructuralMutations}/48 structural mutations; ${generated}/${generated} detected; survivors 0)`
);
