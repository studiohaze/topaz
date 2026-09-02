import { createHash } from 'node:crypto';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const outRoot = path.join(
  root,
  'products/full-embed-evaluator/v1.15.8/vectors'
);
const binary = path.join(root, 'target/debug/lispex');
const evaluatorSha256 =
  'dd4cde2976d825ae99b542d308d87489ac96848e0710329dbb9173664d8c5ad8';
const magic = Buffer.from('LPXFAR01');

if (!process.argv.includes('--write')) {
  throw new Error('use --write to regenerate exact F15 provider vectors');
}

const sha256Bytes = (value) =>
  createHash('sha256').update(value).digest('hex');
const sha256File = (value) => sha256Bytes(readFileSync(value));
const canonicalJson = (value) => `${JSON.stringify(value, null, 2)}\n`;
const run = (args, expected = 0) => {
  const result = spawnSync(binary, args, {
    cwd: root,
    encoding: 'utf8',
    timeout: 240_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.status !== expected) {
    throw new Error(
      `${args.join(' ')} exited ${result.status}; stdout=${result.stdout}; stderr=${result.stderr}`
    );
  }
  return result;
};
const jsonLine = (text) =>
  JSON.parse(text.trim().split('\n').filter(Boolean).at(-1) ?? 'null');

function u32(bytes, offset) {
  return bytes.readUInt32BE(offset);
}

function parseEnvelope(bytes) {
  if (!bytes.subarray(0, 8).equals(magic)) throw new Error('vector magic drift');
  if (bytes.subarray(10, 74).toString() !== evaluatorSha256) {
    throw new Error('vector evaluator drift');
  }
  const count = bytes[74];
  let offset = 75 + count * 8;
  const identityTagOffsets = [];
  const identityValueOffsets = [];
  for (let index = 0; index < 6; index += 1) {
    identityTagOffsets.push(offset);
    const tag = bytes[offset];
    offset += 1;
    if (tag === 1) {
      identityValueOffsets.push(offset);
      offset += 64;
    } else if (tag === 0) {
      identityValueOffsets.push(null);
    } else {
      throw new Error('vector identity tag drift');
    }
  }
  const requestOffset = offset;
  offset += 32;
  const fields = [];
  for (const name of ['replay_request', 'response', 'portable_core']) {
    const lengthOffset = offset;
    const length = u32(bytes, offset);
    offset += 4;
    fields.push({ name, lengthOffset, offset, length, end: offset + length });
    offset += length;
  }
  if (offset !== bytes.length - 32) throw new Error('vector framing drift');
  return {
    kind: bytes[8],
    category: bytes[9],
    limitCountOffset: 74,
    limitValuesOffset: 75,
    identityTagOffsets,
    identityValueOffsets,
    requestOffset,
    fields: Object.fromEntries(fields.map((field) => [field.name, field])),
    digestOffset: offset,
  };
}

function canonicalValueEnd(bytes, start) {
  const tag = bytes[start];
  let offset = start + 1;
  const length = () => {
    const value = Number(bytes.readBigUInt64BE(offset));
    offset += 8;
    return value;
  };
  if ([0, 1, 2].includes(tag)) return offset;
  if ([3, 7, 8, 12].includes(tag)) return offset + length();
  if (tag === 4) {
    offset += length();
    offset += length();
    return offset;
  }
  if (tag === 5) return offset + 8;
  if (tag === 6) return offset + 4;
  if ([9, 11].includes(tag)) {
    const count = length();
    for (let index = 0; index < count; index += 1) {
      offset = canonicalValueEnd(bytes, offset);
    }
    return offset;
  }
  if (tag === 10) {
    const count = length();
    for (let index = 0; index < count + 1; index += 1) {
      offset = canonicalValueEnd(bytes, offset);
    }
    return offset;
  }
  if (tag === 13) {
    const count = length();
    for (let index = 0; index < count; index += 1) {
      offset += Number(bytes.readBigUInt64BE(offset)) + 8;
      offset = canonicalValueEnd(bytes, offset);
    }
    return offset;
  }
  throw new Error(`unsupported canonical value tag ${tag}`);
}

function duplicateFirstRecordEntry(core) {
  if (core[0] !== 13) throw new Error('portable core is not a canonical record');
  const count = core.readBigUInt64BE(1);
  const firstStart = 9;
  const keyBytes = Number(core.readBigUInt64BE(firstStart));
  const firstEnd = canonicalValueEnd(core, firstStart + 8 + keyBytes);
  const countBytes = Buffer.alloc(8);
  countBytes.writeBigUInt64BE(count + 1n);
  return Buffer.concat([
    core.subarray(0, 1),
    countBytes,
    core.subarray(firstStart, firstEnd),
    core.subarray(firstStart),
  ]);
}

function resign(bytes) {
  const body = bytes.subarray(0, bytes.length - 32);
  return Buffer.concat([body, createHash('sha256').update(body).digest()]);
}

function signBody(body) {
  return Buffer.concat([body, createHash('sha256').update(body).digest()]);
}

const work = mkdtempSync(path.join(tmpdir(), 'lispex-f15-vectors-'));
try {
  const build = spawnSync('cargo', ['build', '-p', 'lispex', '--bin', 'lispex'], {
    cwd: root,
    encoding: 'utf8',
    timeout: 240_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (build.status !== 0) throw new Error(`Native build failed: ${build.stderr}`);

  mkdirSync(outRoot, { recursive: true });

  const prepareLimits = path.join(root, 'tests/f12/prepare-limits.json');
  const evalLimits = path.join(root, 'tests/f12/evaluation-limits.json');
  const input = path.join(
    root,
    'examples/checkable-refund/generated/inputs/day-14-unopened.bin'
  );
  const lowPrepare = path.join(work, 'low-prepare.json');
  const lowEval = path.join(work, 'low-eval.json');
  writeFileSync(
    lowPrepare,
    canonicalJson({
      raw_source_bytes: 4096,
      prepare_work: 0,
      logical_allocation: 1000000,
      syntax_depth: 64,
    })
  );
  writeFileSync(
    lowEval,
    canonicalJson({
      canonical_input_bytes: 4096,
      eval_work: 0,
      logical_allocation: 1000000,
      semantic_frames: 1000,
      traversal_depth: 256,
      output_bytes: 1000000,
      diagnostic_bytes: 1000000,
      transcript_bytes: 1000000,
      transcript_events: 100,
      result_bytes: 1000000,
    })
  );
  const invalidSource = path.join(work, 'invalid.lspx');
  const faultSource = path.join(work, 'fault.lspx');
  writeFileSync(invalidSource, '(');
  writeFileSync(faultSource, '(car 1)\n');

  const tempPrepared = (name) => path.join(work, `${name}.prepared.lpxfull`);
  const tempResult = (name) => path.join(work, `${name}.lpxfull`);
  const prepare = (name, source, limits = prepareLimits, expected = 0) => {
    const output = tempPrepared(name);
    run(
      [
        'embed',
        'full',
        'prepare',
        '--source',
        source,
        '--limits',
        limits,
        '--out',
        output,
      ],
      expected
    );
    return output;
  };
  const evaluate = (name, prepared, limits = evalLimits, expected = 0) => {
    const output = tempResult(name);
    run([
      'embed',
      'full',
      'evaluate',
      '--prepared',
      prepared,
      '--input',
      input,
      '--limits',
      limits,
      '--out',
      output,
    ], expected);
    return output;
  };

  const vectorPrepared = prepare(
    'prepared',
    path.join(root, 'tests/f15/full-vector.lspx')
  );
  const positives = [
    ['prepared', vectorPrepared],
    ['complete', evaluate('complete', vectorPrepared)],
    ['semantic-fault', evaluate('semantic-fault', prepare('fault', faultSource))],
    [
      'evaluation-exhausted',
      evaluate('evaluation-exhausted', vectorPrepared, lowEval),
    ],
    [
      'preparation-exhausted',
      prepare(
        'preparation-exhausted',
        path.join(root, 'tests/f15/full-vector.lspx'),
        lowPrepare,
        3
      ),
    ],
    ['request-refusal', prepare('request-refusal', invalidSource, prepareLimits, 3)],
  ];

  const positiveRecords = [];
  for (const [name, sourcePath] of positives) {
    const artifactPath = path.join(outRoot, `${name}.lpxfull`);
    copyFileSync(sourcePath, artifactPath);
    const bytes = readFileSync(artifactPath);
    const envelope = parseEnvelope(bytes);
    const projection = jsonLine(
      run(['embed', 'full', 'inspect', '--artifact', artifactPath]).stdout
    );
    let core = null;
    if (envelope.fields.portable_core.length > 0) {
      const corePath = path.join(outRoot, `${name}.core`);
      writeFileSync(
        corePath,
        bytes.subarray(
          envelope.fields.portable_core.offset,
          envelope.fields.portable_core.end
        )
      );
      core = {
        path: path.relative(root, corePath),
        bytes: statSync(corePath).size,
        sha256: sha256File(corePath),
      };
    }
    positiveRecords.push({
      name,
      artifact: {
        path: path.relative(root, artifactPath),
        bytes: bytes.length,
        sha256: sha256Bytes(bytes),
      },
      operation: projection.kind,
      category: projection.category,
      portable_core: core,
    });
  }

  const completePath = positiveRecords.find((item) => item.name === 'complete')
    .artifact.path;
  const complete = readFileSync(path.join(root, completePath));
  const parsed = parseEnvelope(complete);
  const mutations = [];
  const addMutation = (name, bytes, expectation) => {
    const output = path.join(outRoot, `negative-${name}.lpxfull`);
    writeFileSync(output, bytes);
    mutations.push({
      name,
      artifact: {
        path: path.relative(root, output),
        bytes: bytes.length,
        sha256: sha256Bytes(bytes),
      },
      expectation,
    });
  };
  const mutateAndResign = (offset, value) => {
    const bytes = Buffer.from(complete);
    bytes[offset] = value;
    return resign(bytes);
  };
  const replacePortableCore = (core) => {
    const field = parsed.fields.portable_core;
    const length = Buffer.alloc(4);
    length.writeUInt32BE(core.length);
    return signBody(
      Buffer.concat([
        complete.subarray(0, field.lengthOffset),
        length,
        core,
        complete.subarray(field.end, parsed.digestOffset),
      ])
    );
  };
  const removeLastLimit = () => {
    const count = complete[parsed.limitCountOffset];
    const lastStart = parsed.limitValuesOffset + (count - 1) * 8;
    const body = Buffer.concat([
      complete.subarray(0, parsed.limitCountOffset),
      Buffer.from([count - 1]),
      complete.subarray(parsed.limitValuesOffset, lastStart),
      complete.subarray(lastStart + 8, parsed.digestOffset),
    ]);
    return signBody(body);
  };
  addMutation('magic', mutateAndResign(0, complete[0] ^ 1), 'magic-refusal');
  addMutation('truncated', complete.subarray(0, complete.length - 1), 'truncation-refusal');
  addMutation(
    'trailing',
    resign(Buffer.concat([complete.subarray(0, -32), Buffer.from([0]), complete.subarray(-32)])),
    'trailing-byte-refusal'
  );
  addMutation('evaluator', mutateAndResign(10, complete[10] === 97 ? 98 : 97), 'identity-refusal');
  addMutation(
    'limit-count',
    removeLastLimit(),
    'limit-count-refusal'
  );
  addMutation(
    'limit-value',
    mutateAndResign(parsed.limitValuesOffset + 7, complete[parsed.limitValuesOffset + 7] ^ 1),
    'exact-limit-value-mismatch-refusal'
  );
  addMutation(
    'identity-tag',
    mutateAndResign(parsed.identityTagOffsets[0], 2),
    'identity-tag-refusal'
  );
  addMutation(
    'identity-noncanonical-case',
    mutateAndResign(
      parsed.identityValueOffsets.find((value) => value !== null),
      65
    ),
    'noncanonical-identity-refusal'
  );
  addMutation(
    'identity-mismatch',
    mutateAndResign(
      parsed.identityValueOffsets[3],
      complete[parsed.identityValueOffsets[3]] === 97 ? 98 : 97
    ),
    'inconsistent-identity-refusal'
  );
  addMutation(
    'request',
    mutateAndResign(parsed.requestOffset, complete[parsed.requestOffset] ^ 1),
    'request-binding-refusal'
  );
  addMutation(
    'response',
    mutateAndResign(parsed.fields.response.offset, complete[parsed.fields.response.offset] ^ 1),
    'response-refusal'
  );
  addMutation(
    'portable-core',
    mutateAndResign(
      parsed.fields.portable_core.offset,
      complete[parsed.fields.portable_core.offset] ^ 1
    ),
    'portable-core-refusal'
  );
  const completeCore = complete.subarray(
    parsed.fields.portable_core.offset,
    parsed.fields.portable_core.end
  );
  addMutation(
    'portable-core-duplicate-key',
    replacePortableCore(duplicateFirstRecordEntry(completeCore)),
    'duplicate-core-key-refusal'
  );
  const digestMutation = Buffer.from(complete);
  digestMutation[digestMutation.length - 1] ^= 1;
  addMutation('envelope-digest', digestMutation, 'envelope-digest-refusal');

  const manifest = {
    schema: 'lispex.full-provider-vectors/v1',
    status: 'qualification-input',
    evaluator_sha256: evaluatorSha256,
    contracts: {
      semantic_profile_id: 'lispex/r7rs-rule-current-profile-bounded/1',
      model_id: 'lispex-full-vm-meter/1',
      abi_id: 'lispex.embed-wasm-abi/v1',
      value_codec_id: 'lispex.embed-value/v1',
      transcript_id: 'lispex.embed-transcript/v1',
      receipt_schema_id: 'lispex.embed-receipt-core/v1',
      component_id: 'lispex-evaluator/rust-vm-current-profile/1',
    },
    positives: positiveRecords,
    negatives: mutations,
  };
  writeFileSync(
    path.join(outRoot, 'vectors.v1.json'),
    canonicalJson(manifest)
  );
  console.log(
    `wrote ${positiveRecords.length} positive and ${mutations.length} negative full-provider vectors`
  );
} finally {
  rmSync(work, { recursive: true, force: true });
}
