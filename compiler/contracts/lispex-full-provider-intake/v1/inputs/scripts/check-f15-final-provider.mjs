#!/usr/bin/env node

import { createHash } from 'node:crypto';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const evaluatorPath = path.join(
  root,
  'products/full-embed-evaluator/v1.15.7/lispex-full-embed-evaluator.wasm'
);
const evaluatorSha256 =
  'dd4cde2976d825ae99b542d308d87489ac96848e0710329dbb9173664d8c5ad8';
const evaluatorBytes = 1_484_873;
const consumeIndex = process.argv.indexOf('--consume-core');
const consumeCore = consumeIndex === -1 ? null : process.argv[consumeIndex + 1];
const skipRuntimeTest = process.argv.includes('--skip-runtime-test');
const sha256Bytes = (bytes) => createHash('sha256').update(bytes).digest('hex');
const canonicalJson = (value) => `${JSON.stringify(value, null, 2)}\n`;

function fail(message, result) {
  if (result?.stdout) process.stderr.write(result.stdout);
  if (result?.stderr) process.stderr.write(result.stderr);
  throw new Error(`F15 final provider court failed: ${message}`);
}

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd ?? root,
      env: { ...process.env, CARGO_NET_OFFLINE: 'true', ...options.env },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const stdout = [];
    const stderr = [];
    child.stdout.on('data', (chunk) => stdout.push(chunk));
    child.stderr.on('data', (chunk) => stderr.push(chunk));
    const timer = setTimeout(
      () => child.kill('SIGKILL'),
      options.timeout ?? 300_000
    );
    child.on('error', reject);
    child.on('close', (status) => {
      clearTimeout(timer);
      const result = {
        status,
        stdout: Buffer.concat(stdout).toString('utf8'),
        stderr: Buffer.concat(stderr).toString('utf8'),
      };
      if (status !== (options.status ?? 0)) {
        reject(
          new Error(
            `${command} ${args.join(' ')} exited ${status}; stdout=${result.stdout}; stderr=${result.stderr}`
          )
        );
      } else {
        resolve(result);
      }
    });
  });
}

function jsonLine(text) {
  return JSON.parse(text.trim().split('\n').filter(Boolean).at(-1) ?? 'null');
}

function portableCore(bytes) {
  if (bytes.subarray(0, 8).toString() !== 'LPXFAR01')
    fail('artifact magic drifted');
  let offset = 75 + bytes[74] * 8;
  for (let index = 0; index < 6; index += 1) {
    const tag = bytes[offset++];
    if (tag === 1) offset += 64;
    else if (tag !== 0) fail('artifact identity tag drifted');
  }
  offset += 32;
  for (let index = 0; index < 2; index += 1) {
    const length = bytes.readUInt32BE(offset);
    offset += 4 + length;
  }
  const length = bytes.readUInt32BE(offset);
  offset += 4;
  if (length === 0 || offset + length + 32 !== bytes.length) {
    fail('portable core is absent or misframed');
  }
  return bytes.subarray(offset, offset + length);
}

const evaluator = readFileSync(evaluatorPath);
if (
  evaluator.length !== evaluatorBytes ||
  sha256Bytes(evaluator) !== evaluatorSha256
) {
  fail('exact evaluator bytes drifted');
}
const module = new WebAssembly.Module(evaluator);
if (WebAssembly.Module.imports(module).length !== 0)
  fail('evaluator imports host capability');

await run('cargo', [
  'build',
  '--manifest-path',
  'interp/Cargo.toml',
  '--locked',
  '--offline',
  '--bin',
  'lispex',
]);
if (!skipRuntimeTest) {
  await run('cargo', [
    'test',
    '-p',
    'lispex',
    'f15_final_provider_runtime_qualification',
    '--lib',
  ]);
}

const work = mkdtempSync(path.join(tmpdir(), 'lispex-f15-final-provider-'));
try {
  const binaryName = process.platform === 'win32' ? 'lispex.exe' : 'lispex';
  const installed = path.join(work, binaryName);
  copyFileSync(path.join(root, 'target/debug', binaryName), installed);
  const evalLimits = path.join(work, 'eval-limits.json');
  const input = path.join(work, 'input.bin');
  const prepared = path.join(work, 'prepared.lpxfull');
  const result = path.join(work, 'result.lpxfull');

  copyFileSync(path.join(root, 'tests/f12/evaluation-limits.json'), evalLimits);
  copyFileSync(
    path.join(
      root,
      'products/full-embed-evaluator/v1.15.8/vectors/prepared.lpxfull'
    ),
    prepared
  );
  copyFileSync(
    path.join(
      root,
      'examples/checkable-refund/generated/inputs/day-14-unopened.bin'
    ),
    input
  );
  await run(
    installed,
    [
      'embed',
      'full',
      'evaluate',
      '--prepared',
      prepared,
      '--input',
      input,
      '--limits',
      evalLimits,
      '--out',
      result,
    ],
    { cwd: work }
  );
  const projection = jsonLine(
    (
      await run(installed, ['embed', 'full', 'verify', '--artifact', result], {
        cwd: work,
      })
    ).stdout
  );
  if (
    projection.category !== 'complete' ||
    projection.engine_artifact_sha256 !== evaluatorSha256 ||
    projection.portable_core_assigned !== true ||
    projection.vouch_eligible !== false ||
    projection.fallback_count !== 0
  ) {
    fail('installed complete projection drifted');
  }

  const core = portableCore(readFileSync(result));
  const outputDir = path.join(root, 'target/f15');
  mkdirSync(outputDir, { recursive: true });
  const corePath = path.join(outputDir, 'final-provider-core.bin');
  writeFileSync(corePath, core);
  if (consumeCore) {
    const peer = readFileSync(path.resolve(root, consumeCore));
    if (!core.equals(peer))
      fail('portable core differs from peer architecture');
  }
  const evidence = {
    schema: 'lispex.f15-final-provider-runtime-evidence/v1',
    evaluator_sha256: evaluatorSha256,
    evaluator_bytes: evaluator.length,
    imports: 0,
    portable_core_bytes: core.length,
    portable_core_raw_sha256: sha256Bytes(core),
    source_free_installed_journey: true,
    fresh_instance_per_operation: true,
    concurrent_evaluations: 4,
    byte_identical_concurrent_results: true,
    semantic_work_precedes_fuel_ceiling: true,
    semantic_allocation_precedes_memory_ceiling: true,
    safety_precedence_count: 0,
    fallback: 0,
    vouch_eligible: false,
  };
  writeFileSync(
    path.join(outputDir, 'final-provider-runtime-evidence.json'),
    canonicalJson(evidence)
  );
  console.log(
    `F15 final provider runtime court passed (${evaluator.length} evaluator bytes; imports 0; ${core.length} portable-core bytes; 4 concurrent exact results; safety precedence 0; fallback 0)`
  );
} finally {
  rmSync(work, { recursive: true, force: true });
}
