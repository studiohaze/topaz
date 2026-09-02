#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  canonicalStoredZip,
  dependencies,
} from './f13-evaluator-redistribution-lib.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const mode = process.argv[2];
if (!['--write', '--check'].includes(mode)) {
  throw new Error('usage: gen-f15-final-provider.mjs --write|--check');
}

const sourceRevision = 'e3909e56491b53c61a63efaff51b6093019d8b27';
const wireRevision = '451027beb8daff90c09364a1fd38a2a9f3790e55';
const qualificationRevision = '8e7d4bf2a1bc550b5e1315449002cf2938952ec4';
const crossArchitectureRevision =
  'baf0907c229283e6706a6ceb91dcaef41e225a37';
const crossArchitectureRunId = 30646266047;
const evaluatorSha =
  'dd4cde2976d825ae99b542d308d87489ac96848e0710329dbb9173664d8c5ad8';
const evaluatorBytes = 1_484_873;
const aotCompilerSha =
  '3c5742ee9a34836db49acf47930a127fffc5eb00fa417e5240f91a33ad5d9123';
const contracts = Object.freeze({
  language_profile_id: 'lispex-profile-1.5',
  semantic_profile_id: 'lispex/r7rs-rule-current-profile-bounded/1',
  feature_set_sha256:
    '6ff16159ab9c6758c1485b67c928a9b3b4a896e4f2ecf96e4cf3cbaf5ac22fae',
  model_id: 'lispex-full-vm-meter/1',
  abi_id: 'lispex.embed-wasm-abi/v1',
  value_codec_id: 'lispex.embed-value/v1',
  transcript_id: 'lispex.embed-transcript/v1',
  receipt_schema_id: 'lispex.embed-receipt-core/v1',
  component_id: 'lispex-evaluator/rust-vm-current-profile/1',
});
const restricted = Object.freeze({
  source_revision: '5423eb32a1f456163de1a4898bef4eebd7f3c91e',
  redistribution_revision: '573943d8053cefa0a4dcdf8469abf33e773ff706',
  evaluator_path: 'products/embed-evaluator/v1.12.4/lispex-embed-evaluator.wasm',
  evaluator_sha256:
    'fa6e52559e1f5a43e50a3b7ac0cc5add6930cff0aed8aaff462cff4609362870',
  evaluator_bytes: 1_387_762,
  redistribution_path:
    'products/embed-evaluator/v1.12.4/lispex-embed-evaluator-redistribution.zip',
  redistribution_sha256:
    'fd8c2ba441c977c207147690272f09df3ce3b74c1bd3223c295593a3c5dd1b92',
  component_id: 'lispex-embed-evaluator/1.12.4',
  semantic_profile_id: 'lispex/r7rs-rule-embedded-core/1',
  model_id: 'lispex-vm-meter/1',
  abi_id: 'lispex.embed-wasm-abi/v1',
  value_codec_id: 'lispex.embed-value/v1',
  transcript_id: 'lispex.embed-transcript/v1',
  receipt_schema_id: 'lispex.embed-receipt-core/v1',
});
const evaluatorPath =
  'products/full-embed-evaluator/v1.15.7/lispex-full-embed-evaluator.wasm';
const machinePath =
  'docs/bounded-execution/f15-full-provider-machine-contract.v1.json';
const vectorsPath =
  'products/full-embed-evaluator/v1.15.8/vectors/vectors.v1.json';
const executionInputsPath =
  'docs/bounded-execution/f15-final-provider-execution-inputs.v1.json';
const semanticClosurePath =
  'docs/bounded-execution/f15-final-semantic-surface-closure.v1.json';
const restrictedMachinePath =
  'docs/bounded-execution/f15-retained-restricted-lda-c1-machine-contract.v1.json';
const restrictedVectorsPath =
  'products/embed-evaluator/handoffs/lda-c1/v1/vectors/vectors.v1.json';
const qualificationPath =
  'docs/bounded-execution/f15-final-qualification.v1.json';
const componentPath =
  'embedding-readiness/release-dag/components/lispex-full-embed-evaluator-1.v1.json';
const courtPath =
  'docs/bounded-execution/f15-final-provider-court.v1.json';
const rootPath = 'docs/bounded-execution/f15-final-provider-root.v1.json';
const evidenceDirectory = 'docs/bounded-execution/f15-provider-evidence';
const distributionDirectory =
  'products/full-embed-evaluator/v1.15.8/redistribution';
const archivePath =
  'products/full-embed-evaluator/v1.15.8/lispex-full-embed-evaluator-redistribution.zip';
const attestationPath =
  'embedding-readiness/release-dag/attestations/lispex-full-embed-evaluator-1.v1.json';
const distributionAttestationPath =
  'embedding-readiness/release-dag/attestations/lispex-full-embed-evaluator-redistribution-1.v1.json';
const dispositionsPath =
  'embedding-readiness/release-dag/dispositions/lispex-1.15.8-full-provider.v1.json';
const historyPath =
  'embedding-readiness/release-dag/history/lispex-attestation-history.v7.json';
const retentionPath =
  'embedding-readiness/release-dag/retention/lispex-v1.15.8-full-embed-evaluator.v1.json';
const handoffRetentionPath =
  'embedding-readiness/release-dag/retention/lispex-v1.15.8-topaz-component-handoff.v1.json';
const snapshotPath =
  'embedding-readiness/release-dag/snapshots/lispex-1.15.8-full-provider.v1.json';
const configPath =
  'embedding-readiness/release-dag/full-provider-release-config.v1.json';
const viewPath =
  'embedding-readiness/release-dag/generated/current-full-evaluator-provider.v1.json';
const handoffPath =
  'embedding-readiness/release-dag/handoffs/lispex-f15-topaz-component-handoff.v1.json';

const exactJson = (value) => Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
const read = (relative) => readFileSync(path.join(root, relative));
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const reference = (relative, bytes = read(relative)) => ({
  path: relative,
  sha256: sha(bytes),
});
const retained = (kind, relative, bytes = read(relative)) => ({
  kind,
  path: relative,
  sha256: sha(bytes),
  bytes: bytes.length,
});
const outputs = new Map();
const putJson = (relative, value) => {
  const bytes = exactJson(value);
  outputs.set(relative, bytes);
  return bytes;
};
const outputReference = (relative) => {
  const bytes = outputs.get(relative);
  if (!bytes) throw new Error(`missing generated output ${relative}`);
  return reference(relative, bytes);
};
const generatedOrInputReference = (relative) =>
  outputs.has(relative) ? outputReference(relative) : reference(relative);

const evaluator = read(evaluatorPath);
if (evaluator.length !== evaluatorBytes || sha(evaluator) !== evaluatorSha) {
  throw new Error('exact full evaluator bytes drifted');
}
if (WebAssembly.Module.imports(new WebAssembly.Module(evaluator)).length !== 0) {
  throw new Error('exact full evaluator imports host capability');
}
const restrictedEvaluator = read(restricted.evaluator_path);
if (
  restrictedEvaluator.length !== restricted.evaluator_bytes ||
  sha(restrictedEvaluator) !== restricted.evaluator_sha256 ||
  WebAssembly.Module.imports(new WebAssembly.Module(restrictedEvaluator)).length !== 0
) {
  throw new Error('retained restricted evaluator bytes drifted');
}
const restrictedRedistribution = read(restricted.redistribution_path);
if (sha(restrictedRedistribution) !== restricted.redistribution_sha256) {
  throw new Error('retained restricted redistribution bytes drifted');
}
const executionInputs = JSON.parse(read(executionInputsPath).toString('utf8'));
const runtimeEvidence = executionInputs.cross_architecture;
if (
  executionInputs.subject.evaluator_sha256 !== evaluatorSha ||
  executionInputs.revisions.qualification !== qualificationRevision ||
  executionInputs.revisions.cross_architecture_workflow !==
    crossArchitectureRevision ||
  runtimeEvidence.run_id !== crossArchitectureRunId ||
  runtimeEvidence.artifact.portable_core_raw_sha256 !==
    '3cbd38dc840902d4e606431545bf9cadb584fafe6c023add26745a49a9da7dd0' ||
  runtimeEvidence.result.safety_precedence_count !== 0 ||
  runtimeEvidence.result.fallback_count !== 0
) {
  throw new Error('tracked final provider execution evidence drifted');
}

const surface = JSON.parse(
  read('docs/bounded-execution/full-profile-surface.v0.json').toString('utf8')
);
const costClassification = JSON.parse(
  read('docs/bounded-execution/full-profile-cost-classification.v0.json').toString(
    'utf8'
  )
);
const guestControl = JSON.parse(
  read('docs/bounded-execution/full-guest-control-classification.v0.json').toString(
    'utf8'
  )
);
if (
  surface.semantic_profile !== contracts.language_profile_id ||
  surface.denominator.primitive_rows !== 205 ||
  surface.denominator.bytecode_opcodes !== 22 ||
  surface.denominator.guest_calling_rows !== 18 ||
  costClassification.rows.length !== 205 ||
  costClassification.summary.full_design_deferred_rows !== 0 ||
  guestControl.summary.runtime_admitted_rows !== 18 ||
  guestControl.summary.deferred_rows !== 0 ||
  guestControl.summary.non_control_runtime_rows_pending !== 0
) {
  throw new Error('final semantic-surface authority drifted');
}
const fullClassCounts = Object.fromEntries(
  ['Fixed', 'Precharged', 'Incremental', 'GuestCalling', 'Deferred'].map(
    (classification) => [
      classification,
      costClassification.rows.filter(
        (row) => row.full_cost_class === classification
      ).length,
    ]
  )
);
const semanticAuthorityPaths = [
  'product/contracts/lispex-profile-1.5.json',
  'product/contracts/source-frontend.v1.json',
  'product/contracts/primitive-capabilities.v2.json',
  'product/contracts/diagnostic-catalog.v1.json',
  'docs/bounded-execution/full-profile-surface-authority.v0.json',
  'docs/bounded-execution/full-profile-surface.v0.json',
  'docs/bounded-execution/full-primitive-registry-authority.v0.json',
  'docs/bounded-execution/full-primitive-registry.v0.json',
  'docs/bounded-execution/full-profile-cost-authority.v0.json',
  'docs/bounded-execution/full-profile-cost-classification.v0.json',
  'docs/bounded-execution/full-guest-control-authority.v0.json',
  'docs/bounded-execution/full-guest-control-classification.v0.json',
  'docs/bounded-execution/guest-kernel-roots.v0.json',
  'docs/bounded-execution/evaluation-allocation-mir.v0.json',
  'docs/bounded-execution/recursive-components.v0.json',
  'docs/bounded-execution/guest-kernel-dependency-allowlist.v0.json',
];
for (const relative of semanticAuthorityPaths) read(relative);
const semanticClosure = {
  schema: 'lispex.f15-final-semantic-surface-closure/v1',
  status: 'qualified-final-provider-surface',
  evaluator_source_revision: sourceRevision,
  language_profile_authority: {
    id: contracts.language_profile_id,
    ...reference('product/contracts/lispex-profile-1.5.json'),
  },
  executable_profile: {
    id: contracts.semantic_profile_id,
    feature_set_sha256: contracts.feature_set_sha256,
  },
  source_authorities: semanticAuthorityPaths.map((relative) =>
    reference(relative)
  ),
  denominator: surface.denominator,
  construction_classification: {
    allowed: ['Fixed', 'Precharged', 'Incremental', 'GuestCalling', 'Deferred'],
    counts: fullClassCounts,
    unclassified_rows: 0,
    deferred_rows: 0,
  },
  runtime_closure: {
    primitive_rows: 205,
    reached_primitive_rows: 205,
    bytecode_opcodes: 22,
    guest_calling_rows: 18,
    guest_calling_runtime_admitted_rows: 18,
    non_control_runtime_rows_pending: 0,
    deferred_executable_rows: 0,
  },
  boundary: {
    component_qualified_by_this_record: false,
    public_language_profile_changed: false,
    restricted_profile_changed: false,
  },
};
putJson(semanticClosurePath, semanticClosure);

const restrictedVectors = JSON.parse(
  read(restrictedVectorsPath).toString('utf8')
);
if (
  restrictedVectors.evaluator_sha256 !== restricted.evaluator_sha256 ||
  restrictedVectors.positives.length !== 6 ||
  restrictedVectors.negatives.length < 13
) {
  throw new Error('retained restricted LDA-C1 vector authority drifted');
}
const restrictedContract = {
  schema: 'lispex.retained-restricted-lda-c1-machine-contract/v1',
  status: 'qualified-exact-construction-handoff',
  component: {
    id: restricted.component_id,
    source_revision: restricted.source_revision,
    artifact: {
      path: restricted.evaluator_path,
      bytes: restricted.evaluator_bytes,
      sha256: restricted.evaluator_sha256,
      imports: 0,
    },
    changed: false,
  },
  contracts: {
    language_profile_id: contracts.language_profile_id,
    semantic_profile_id: restricted.semantic_profile_id,
    feature_set_sha256:
      'c7ac2d3037b43dd90889467aabdcd2d3c061559bde12bb8330d878886c5ab429',
    model_id: restricted.model_id,
    abi_id: restricted.abi_id,
    value_codec_id: restricted.value_codec_id,
    transcript_id: restricted.transcript_id,
    receipt_schema_id: restricted.receipt_schema_id,
  },
  wasm_abi: {
    version: 65536,
    imports: [],
    exports: [
      'lispex_embed_abi_version: () -> u32',
      'lispex_embed_alloc: (u32) -> u32',
      'lispex_embed_dealloc: (u32,u32) -> u32',
      'lispex_embed_prepare: (u32,u32) -> packed-u64',
      'lispex_embed_evaluate: (u32,u32) -> packed-u64',
      'memory',
    ],
    packed_response: 'high-u32 pointer followed by low-u32 length',
    integer_encoding: 'little-endian WebAssembly i32/i64 at the call boundary',
  },
  request_grammar: {
    integer_encoding: 'unsigned big-endian',
    prepare: [
      '8 bytes ASCII LPXPRP01',
      'u32 source length and source bytes',
      'u64 raw_source_bytes',
      'u64 prepare_work',
      'u64 prepare_logical_allocation',
      'u64 syntax_depth',
      'end of input',
    ],
    evaluate: [
      '8 bytes ASCII LPXEVA01',
      'u32 prepared length and prepared bytes',
      'u32 canonical input length and canonical input bytes',
      'ten u64 evaluation limits in contract order',
      'end of input',
    ],
  },
  response_grammar: {
    magic: 'LPXRSP01',
    integer_encoding: 'unsigned big-endian',
    categories: {
      prepared: 0,
      complete: 1,
      semantic_fault: 2,
      exhausted: 3,
      request_refusal: 4,
      engine_fault: 5,
    },
    fields: [
      'u8 operation',
      'u8 category',
      'u16 code length and ASCII code bytes',
      'u32 payload length and payload bytes',
      'six optional SHA-256 fields with closed tag 0 or 1',
      'optional nine-counter nonportable usage block',
      'end of input',
    ],
  },
  native_artifact_grammar: {
    magic: 'LPXART01',
    integer_encoding: 'unsigned big-endian',
    fields: [
      'u8 kind',
      'u8 category',
      '64 lowercase hexadecimal evaluator digest bytes',
      'u8 exact-limit count followed by that many u64 values',
      'six optional SHA-256 identity fields with closed tag 0 or 1',
      'u32 response length and exact raw response bytes',
      'u32 portable-core length and exact portable-core bytes',
      'end of input',
    ],
    prepare_limit_count: 4,
    evaluate_limit_count: 14,
    replay_request_present: false,
    request_digest_present: false,
    whole_envelope_digest_present: false,
    source_retained: false,
    trailing_bytes: 'forbidden',
  },
  portable_core_construction: {
    encoding: restricted.value_codec_id,
    record_keys: 'strict UTF-8 byte order and unique',
    assigned: [
      'prepare exhausted',
      'prepare request-refusal',
      'evaluate complete',
      'evaluate semantic-fault',
      'evaluate exhausted',
      'evaluate request-refusal',
    ],
    absent: ['prepare prepared', 'prepare engine-fault', 'evaluate engine-fault'],
    usage_counters_included: false,
    vouch_eligible: false,
    provider_issued: false,
    consumer_output: 'unauthenticated deterministic serialization only',
  },
  hash_domains: [
    'lispex.embedding.submission/v1',
    'lispex.embedding.resource-contract/v1',
    'lispex.embedding.request/v1',
    'lispex.embedding.evaluation/v1',
    'lispex.embedding.transcript/v1',
    'lispex.embedding.result/v1',
    'lispex.embedding.receipt-core/v1',
  ],
  vectors: reference(restrictedVectorsPath),
  generator: reference(
    'scripts/gen-f15-retained-restricted-provider-vectors.mjs'
  ),
  checker: reference(
    'scripts/check-f15-retained-restricted-provider-vectors.mjs'
  ),
  retained_provider: {
    component_manifest: reference(
      'embedding-readiness/release-dag/components/lispex-embed-evaluator-1.12.4.v1.json'
    ),
    provider_court: reference(
      'docs/bounded-execution/f12-final-provider-court.v1.json'
    ),
    redistribution: {
      path: restricted.redistribution_path,
      sha256: restricted.redistribution_sha256,
      bytes: restrictedRedistribution.length,
    },
  },
  forbidden: {
    abi_change: true,
    wrapper_component: true,
    fallback: true,
    discovery: true,
    host_capability: true,
    topaz_admission: true,
    public_compatibility_claim: true,
  },
};
putJson(restrictedMachinePath, restrictedContract);

const common = {
  schema: 'lispex.f15-provider-evidence/v1',
  subjects: {
    evaluator_source_revision: sourceRevision,
    provider_wire_revision: wireRevision,
    qualification_revision: qualificationRevision,
    artifact_sha256: evaluatorSha,
    executable_profile_id: contracts.semantic_profile_id,
    feature_set_sha256: contracts.feature_set_sha256,
  },
  execution_input: reference(executionInputsPath),
};
const evidenceDefinitions = [
  [
    'post-b7-structural-closure',
    [
      'docs/bounded-execution/guest-kernel-roots.v0.json',
      'docs/bounded-execution/evaluation-allocation-mir.v0.json',
      'docs/bounded-execution/recursive-components.v0.json',
      'docs/bounded-execution/guest-kernel-dependency-allowlist.v0.json',
      'docs/bounded-execution/full-profile-cost-classification.v0.json',
      'docs/bounded-execution/full-guest-control-classification.v0.json',
      semanticClosurePath,
    ],
    {
      status: 'passed',
      closedWorldRoots: true,
      backedgeDominance: true,
      recursionGuards: true,
      allocationSinkGates: true,
      dependencyCapabilityAllowlist: true,
      wasmImports: 0,
      sourceRevision,
    },
  ],
  [
    'post-closure-meter-site-reachability',
    [
      'docs/bounded-execution/full-profile-cost-classification.v0.json',
      'interp/src/full_primitive_meter_generated.rs',
      'interp/src/bounded_execution.rs',
      semanticClosurePath,
    ],
    {
      status: 'passed',
      registeredPrimitiveSites: 205,
      reachedPrimitiveSites: 205,
      opcodes: 22,
      guestCallingRows: 18,
      unreachableSites: 0,
      deferredRows: 0,
    },
  ],
  [
    'post-closure-mutation',
    [
      'scripts/check-f15-full-provider-mutations.mjs',
      'scripts/check-f15-full-provider-vectors.mjs',
      vectorsPath,
      executionInputsPath,
    ],
    {
      status: 'passed',
      generatedMutations: 253,
      detectedMutations: 253,
      malformedVectorCases: JSON.parse(read(vectorsPath).toString('utf8'))
        .negatives.length,
      survivors: 0,
      executionRevision: qualificationRevision,
    },
  ],
  [
    'post-closure-property-fuzz',
    [
      'interp/src/full_profile_cost.rs',
      'interp/src/bounded_execution.rs',
      'interp/src/full_embed_host.rs',
      executionInputsPath,
    ],
    {
      status: 'passed',
      fixedSizes: [0, 1, 2, 3, 7, 8, 15, 16, 31, 32],
      fullFamilyTests: 28,
      concurrentEvaluations: 4,
      byteIdenticalConcurrentResults: true,
      zeroFallback: true,
      executionRevision: qualificationRevision,
    },
  ],
  [
    'post-closure-safety-dominance',
    [
      'scripts/check-f15-final-provider.mjs',
      'interp/src/full_embed_host.rs',
      executionInputsPath,
    ],
    {
      status: 'passed',
      safetyPrecedenceCount: 0,
      semanticWorkPrecedesFuelCeiling: true,
      semanticAllocationPrecedesMemoryCeiling: true,
      freshInstancePassed: true,
      executionRevision: qualificationRevision,
    },
  ],
  [
    'cross-architecture-portable-core',
    [
      '.github/workflows/f15-final-provider-cross-architecture.yml',
      executionInputsPath,
    ],
    {
      status: 'passed',
      runId: crossArchitectureRunId,
      workflowRevision: crossArchitectureRevision,
      qualificationRevision,
      macosArm64JobId: 91208284061,
      linuxX64JobId: 91210111599,
      portableCoreByteEqual: true,
      portableCoreRawSha256:
        runtimeEvidence.artifact.portable_core_raw_sha256,
    },
  ],
  [
    'installed-source-free-product',
    ['scripts/check-f15-final-provider.mjs', vectorsPath, executionInputsPath],
    {
      status: 'passed',
      sourceFreeInstalledJourney: true,
      freshInstancePassed: true,
      fallback: 0,
      vouchEligible: false,
      executionRevision: qualificationRevision,
    },
  ],
];
const evidence = new Map();
for (const [kind, paths, result] of evidenceDefinitions) {
  const relative = `${evidenceDirectory}/${kind}.v1.json`;
  const value = {
    ...common,
    kind,
    primaryEvidence: paths.map((item) => generatedOrInputReference(item)),
    result,
  };
  evidence.set(kind, { path: relative, value });
  putJson(relative, value);
}

const qualification = {
  schema: 'lispex.f15-final-evaluator-qualification/v1',
  status: 'qualified-and-frozen',
  provider_contract_frozen: true,
  public_contract: false,
  release_authority: false,
  source_revision: sourceRevision,
  provider_wire_revision: wireRevision,
  qualification_candidate_revision: qualificationRevision,
  cross_architecture_revision: crossArchitectureRevision,
  evaluator: {
    path: evaluatorPath,
    bytes: evaluatorBytes,
    sha256: evaluatorSha,
    imports: 0,
    memory_initial_pages: 19,
    memory_maximum_pages: 256,
  },
  contracts,
  semantic_surface_closure: outputReference(semanticClosurePath),
  denominator: {
    primitive_rows: 205,
    deferred_rows: 0,
    opcodes: 22,
    guest_calling_rows: 18,
  },
  mutation: { generated: 253, detected: 253, survivors: 0 },
  portable_core: {
    bytes: runtimeEvidence.artifact.portable_core_bytes,
    raw_sha256: runtimeEvidence.artifact.portable_core_raw_sha256,
    vector: 'products/full-embed-evaluator/v1.15.8/vectors/complete.core',
  },
  cross_architecture: {
    workflow: '.github/workflows/f15-final-provider-cross-architecture.yml',
    run_id: crossArchitectureRunId,
    workflow_revision: crossArchitectureRevision,
    qualification_revision: qualificationRevision,
    portable_core_byte_equal: true,
  },
  forbidden: {
    discovery: true,
    fallback: true,
    host_capability: true,
    remote_transport: true,
    instance_pooling: true,
    vouch_promotion: true,
    topaz_consumption_implementation: true,
  },
};
putJson(qualificationPath, qualification);

const component = {
  schema: 'studio-haze.lispex-evaluator-component/v1',
  id: contracts.component_id,
  status: 'qualified-develop-provider-component',
  release_authority: false,
  source_revision: sourceRevision,
  provider_wire_revision: wireRevision,
  qualification_candidate_revision: qualificationRevision,
  cross_architecture_revision: crossArchitectureRevision,
  artifact: {
    path: evaluatorPath,
    bytes: evaluatorBytes,
    sha256: evaluatorSha,
    imports: 0,
  },
  contracts,
  language_profile_authority: {
    id: contracts.language_profile_id,
    ...reference('product/contracts/lispex-profile-1.5.json'),
  },
  semantic_surface_closure: outputReference(semanticClosurePath),
  machine_contract: reference(machinePath),
  vectors: reference(vectorsPath),
  qualification: outputReference(qualificationPath),
  closed_world: { primitive_rows: 205, deferred_rows: 0 },
  dependencies: {
    topaz_build_dependency: null,
    aot_component_dependency: null,
  },
  boundaries: {
    whole_lispex_release_pin: false,
    topaz_admission: false,
    topaz_implementation_authority: false,
    vouch_authority: false,
  },
};
putJson(componentPath, component);

const vectorManifest = JSON.parse(read(vectorsPath).toString('utf8'));
const vectorFiles = [
  ...vectorManifest.positives.flatMap((item) =>
    [item.artifact, item.portable_core].filter(Boolean)
  ),
  ...vectorManifest.negatives.map((item) => item.artifact),
];
const restrictedVectorFiles = [
  ...restrictedVectors.positives.flatMap((item) =>
    [item.artifact, item.portable_core].filter(Boolean)
  ),
  ...restrictedVectors.negatives.map((item) => item.artifact),
];
const priorThirdParty = read(
  'products/embed-evaluator/v1.12.4/redistribution/THIRD-PARTY-NOTICES.txt'
)
  .toString('utf8')
  .replace('Lispex exact evaluator third-party notices', 'Lispex exact full-profile evaluator third-party notices')
  .replace('lispex-embed-evaluator 1.12.4', 'lispex-full-embed-evaluator 1');
const sbom = exactJson({
  schema: 'lispex.evaluator-sbom/v1',
  component: contracts.component_id,
  artifact_sha256: evaluatorSha,
  dependencies,
});
const payloads = new Map([
  ['LICENSE', read('products/embed-evaluator/v1.12.4/redistribution/LICENSE')],
  ['NOTICE', read('products/embed-evaluator/v1.12.4/redistribution/NOTICE')],
  ['THIRD-PARTY-NOTICES.txt', Buffer.from(priorThirdParty)],
  ['component-manifest.v1.json', outputs.get(componentPath)],
  ['lispex-full-embed-evaluator.wasm', evaluator],
  ['machine-contract.v1.json', read(machinePath)],
  ['provider-vectors.v1.json', read(vectorsPath)],
  ['semantic-surface-closure.v1.json', outputs.get(semanticClosurePath)],
  [
    'language-profile-authority.json',
    read('product/contracts/lispex-profile-1.5.json'),
  ],
  ['sbom.v1.json', sbom],
]);
for (const item of vectorFiles) {
  payloads.set(`vectors/${path.basename(item.path)}`, read(item.path));
}
const memberRecord = ([name, bytes]) => ({
  path: name,
  bytes: bytes.length,
  sha256: sha(bytes),
  executable: false,
});
const distributionManifest = exactJson({
  schema: 'lispex.full-embed-evaluator-redistribution/v1',
  status: 'exact-qualified-redistribution-material',
  authority: 'redistribution-material-only',
  component: contracts.component_id,
  evaluator_source_revision: sourceRevision,
  evaluator: { path: 'lispex-full-embed-evaluator.wasm', bytes: evaluatorBytes, sha256: evaluatorSha, imports: 0 },
  contracts,
  license: 'Apache-2.0',
  notice: { runtime_dependency_count: dependencies.filter((item) => item.closure_role === 'runtime').length, build_tool_dependency_count: dependencies.filter((item) => item.closure_role === 'build-tool').length },
  payload: [...payloads].sort(([a], [b]) => a.localeCompare(b)).map(memberRecord),
  envelope_members: ['redistribution-manifest.v1.json', 'SHA256SUMS'],
  archive_profile: 'canonical-stored-zip/v1',
  capabilities: { host_imports: false, discovery: false, fallback: false, remote_transport: false, vouch_promotion: false },
  topaz: { source_dependency: false, admission_claimed: false, implementation_claimed: false },
  release_authority: false,
});
payloads.set('redistribution-manifest.v1.json', distributionManifest);
const checksums = Buffer.from(
  `${[...payloads].sort(([a], [b]) => a.localeCompare(b)).map(([name, bytes]) => `${sha(bytes)}  ${name}`).join('\n')}\n`
);
payloads.set('SHA256SUMS', checksums);
for (const [name, bytes] of payloads) outputs.set(`${distributionDirectory}/${name}`, bytes);
const archive = canonicalStoredZip(payloads);
outputs.set(archivePath, archive);

const evidenceRefs = [...evidence].map(([kind, { path: relative }]) => ({
  kind,
  ...outputReference(relative),
}));
const court = {
  schema: 'lispex.f15-final-provider-court/v1',
  subjects: {
    evaluator_source_revision: sourceRevision,
    provider_wire_revision: wireRevision,
    qualification_revision: qualificationRevision,
    cross_architecture_revision: crossArchitectureRevision,
    artifact_sha256: evaluatorSha,
    component_manifest_sha256: sha(outputs.get(componentPath)),
    redistribution_sha256: sha(archive),
  },
  inputs: {
    execution_record: reference(executionInputsPath),
    semantic_surface_closure: outputReference(semanticClosurePath),
    full_machine_contract: reference(machinePath),
    full_vectors: reference(vectorsPath),
    restricted_machine_contract: outputReference(restrictedMachinePath),
    restricted_vectors: reference(restrictedVectorsPath),
  },
  evidence: evidenceRefs,
  evidence_digests: evidenceRefs.map((item) => item.sha256),
  result: {
    status: 'passed',
    missing_inputs: 0,
    failed_inputs: 0,
    stale_inputs: 0,
    full_positive_vectors: vectorManifest.positives.length,
    full_negative_vectors: vectorManifest.negatives.length,
    restricted_positive_vectors: restrictedVectors.positives.length,
    restricted_negative_vectors: restrictedVectors.negatives.length,
    safety_precedence_count: 0,
    fallback_count: 0,
    fresh_instance_passed: true,
    deferred_executable_rows: 0,
  },
  boundaries: {
    whole_release_pin: false,
    component_admission: false,
    topaz_implementation_authority: false,
    topaz_release_authority: false,
    public_compatibility_claim: false,
    public_component_release: false,
    external_witness: false,
  },
};
putJson(courtPath, court);
const providerRoot = {
  schema: 'lispex.f15-final-provider-root/v1',
  status: 'passed-component-provider-root',
  evaluator_source_revision: sourceRevision,
  provider_wire_revision: wireRevision,
  qualification_revision: qualificationRevision,
  cross_architecture_revision: crossArchitectureRevision,
  generator_identity: {
    generator: reference('scripts/gen-f15-final-provider.mjs'),
    checker: {
      ...reference('scripts/gen-f15-final-provider.mjs'),
      invocation: '--check',
    },
    runtime_court: reference('scripts/check-f15-final-provider.mjs'),
    full_vector_generator: reference(
      'scripts/gen-f15-full-provider-vectors.mjs'
    ),
    full_vector_checker: reference(
      'scripts/check-f15-full-provider-vectors.mjs'
    ),
    restricted_vector_generator: reference(
      'scripts/gen-f15-retained-restricted-provider-vectors.mjs'
    ),
    restricted_vector_checker: reference(
      'scripts/check-f15-retained-restricted-provider-vectors.mjs'
    ),
    mutation_checker: reference(
      'scripts/check-f15-full-provider-mutations.mjs'
    ),
    canonical_serialization: 'UTF-8 JSON with two-space indentation and one LF',
  },
  execution_record: reference(executionInputsPath),
  semantic_surface_closure: outputReference(semanticClosurePath),
  full_component: {
    evaluator: { sha256: evaluatorSha, bytes: evaluatorBytes, imports: 0 },
    contracts,
    component: outputReference(componentPath),
    qualification: outputReference(qualificationPath),
    machine_contract: reference(machinePath),
    vectors: reference(vectorsPath),
    vector_artifacts: vectorFiles.map((item) => reference(item.path)),
    redistribution: {
      path: archivePath,
      sha256: sha(archive),
      bytes: archive.length,
    },
  },
  retained_restricted_component: {
    component_id: restricted.component_id,
    evaluator: {
      path: restricted.evaluator_path,
      sha256: restricted.evaluator_sha256,
      bytes: restricted.evaluator_bytes,
      imports: 0,
    },
    component_manifest: reference(
      'embedding-readiness/release-dag/components/lispex-embed-evaluator-1.12.4.v1.json'
    ),
    provider_court: reference(
      'docs/bounded-execution/f12-final-provider-court.v1.json'
    ),
    machine_contract: outputReference(restrictedMachinePath),
    vectors: reference(restrictedVectorsPath),
    vector_artifacts: restrictedVectorFiles.map((item) => reference(item.path)),
    redistribution: {
      path: restricted.redistribution_path,
      sha256: restricted.redistribution_sha256,
      bytes: restrictedRedistribution.length,
    },
    changed: false,
  },
  evidence: evidenceRefs,
  final_court: outputReference(courtPath),
  boundaries: {
    root_references_handoff: false,
    whole_release_pin: false,
    mutual_final_manifest_reference: false,
    topaz_admission: false,
    topaz_release_authority: false,
    public_release_authority: false,
  },
};
putJson(rootPath, providerRoot);

const providerAttestation = {
  schema: 'studio-haze.relationship-attestation/v1',
  id: 'lispex-full-embed-evaluator-1/v1',
  relationship: 'lispex.provides-full-embed-evaluator/v1',
  state: 'qualified',
  subject: { component: contracts.component_id, ...outputReference(componentPath) },
  provider: { evaluator_artifact_sha256: evaluatorSha, ...contracts },
  evidence: { provider_root: outputReference(rootPath), final_provider_court: outputReference(courtPath), records: evidenceRefs },
  build_dependency: false,
  limitations: ['provider-attestation-not-topaz-admission', 'single-rust-vm-engine', 'no-topaz-target-admission', 'no-vouch-authority', 'not-publicly-released'],
  issuer: { owner: 'lispex-release', signature_status: 'unsigned-develop-provider-attestation' },
};
putJson(attestationPath, providerAttestation);
const distributionAttestation = {
  schema: 'lispex.evaluator-redistribution-attestation/v1',
  id: 'lispex-full-embed-evaluator-redistribution-1/v1',
  relationship: 'lispex.provides-full-embed-evaluator-redistribution/v1',
  state: 'exact-redistribution-material',
  subject: { component: contracts.component_id, component_manifest: outputReference(componentPath), evaluator: { sha256: evaluatorSha, bytes: evaluatorBytes } },
  distribution: { path: archivePath, sha256: sha(archive), bytes: archive.length, profile: 'canonical-stored-zip/v1' },
  provider_evidence: { final_court: outputReference(courtPath), provider_attestation: outputReference(attestationPath) },
  boundaries: { topaz_admission: false, topaz_implementation_authority: false, whole_lispex_release_pin: false, evaluator_rebuild: false, network_resolution: false, latest_resolution: false, vouch_promotion: false, release_authority: false },
};
putJson(distributionAttestationPath, distributionAttestation);

const priorDispositions = JSON.parse(read('embedding-readiness/release-dag/dispositions/lispex-1.15.0.v1.json'));
const latestTopazVersion = '5.17';
const latestTopazMacosArm64Sha =
  '4c61f3df1782033b87019a70c8bb7bfcd3da23c3e3711a33730ff4d1fe626f6f';
const retainedDispositions = priorDispositions.records.map((record) => {
  if (record.relationship === 'lispex-aot.uses-topaz-compiler/v1') {
    return {
      ...record,
      latest_candidate_seen: latestTopazVersion,
      latest_candidate_digest: latestTopazMacosArm64Sha,
      latest_candidate_status: 'publicly-released-exact-macos-aarch64-product',
      reason_code: 'exact-public-candidate-reviewed-no-aot-court',
      reason:
        'Acknowledge the exact publicly released latest macOS ARM64 Topaz product while retaining the published 5.11 compiler component because F15 ran no AOT advancement court.',
      revisit_trigger: 'next-exact-topaz-aot-advancement-court',
    };
  }
  if (record.relationship === 'lispex-in-topaz.tested-with-topaz/v1') {
    return {
      ...record,
      latest_candidate_seen: latestTopazVersion,
      latest_candidate_digest: latestTopazMacosArm64Sha,
      latest_candidate_status: 'publicly-released-exact-macos-aarch64-product',
      reason_code: 'new-public-runner-reviewed-no-conformance-court',
      reason:
        'Acknowledge the exact publicly released latest macOS ARM64 Topaz product while retaining the 5.11 runner evidence because F15 ran no Lispex-in-Topaz conformance court.',
      revisit_trigger: 'next-lispex-in-topaz-conformance-court',
    };
  }
  return record;
});
const dispositions = {
  ...priorDispositions,
  release_id: 'lispex-checkpoint/1.15.8',
  status: 'qualified-develop-provider-review',
  records: [
    ...retainedDispositions,
    { relationship: 'lispex.provides-full-embed-evaluator/v1', decision: 'advance', selected_version: '1', selected_digest: evaluatorSha, latest_candidate_seen: '1', latest_candidate_digest: evaluatorSha, latest_candidate_status: 'qualified-develop-provider-component', reason_code: 'full-current-profile-qualified', reason: 'Add the separately identified full current-profile evaluator without changing the retained restricted component.', owner: 'lispex-release', evidence_digest: sha(outputs.get(courtPath)), revisit_trigger: 'next-full-evaluator-artifact-or-contract-change' },
    { relationship: 'lispex.provides-full-embed-evaluator-redistribution/v1', decision: 'advance', selected_version: '1', selected_digest: sha(archive), latest_candidate_seen: '1', latest_candidate_digest: sha(archive), latest_candidate_status: 'qualified-exact-redistribution-material', reason_code: 'full-provider-redistribution-qualified', reason: 'Retain the exact licensed full evaluator redistribution bytes separately from product release state.', owner: 'lispex-release', evidence_digest: sha(outputs.get(distributionAttestationPath)), revisit_trigger: 'next-full-evaluator-redistribution-change' },
  ],
};
putJson(dispositionsPath, dispositions);

const previousHistoryPath = 'embedding-readiness/release-dag/history/lispex-attestation-history.v6.json';
const previousHistory = JSON.parse(read(previousHistoryPath));
const history = {
  schema: previousHistory.schema,
  id: 'lispex-attestation-history/v1.15.8-v7',
  status: 'append-only-successor-not-release-build-input',
  product: 'lispex',
  source_release_id: 'lispex-checkpoint/1.15.8',
  previous_history: reference(previousHistoryPath),
  entries: [
    ...previousHistory.entries.map((entry) => ({ ...entry, sequence: entry.sequence + 1, transition: 'retain', predecessor_attestation_sha256: entry.attestation.sha256 })),
    { relationship: 'lispex.provides-full-embed-evaluator/v1', sequence: 0, transition: 'genesis', predecessor_attestation_sha256: null, attestation: outputReference(attestationPath) },
    { relationship: 'lispex.provides-full-embed-evaluator-redistribution/v1', sequence: 0, transition: 'genesis', predecessor_attestation_sha256: null, attestation: outputReference(distributionAttestationPath) },
  ],
  append_only_rules: previousHistory.append_only_rules,
  build_input: false,
};
putJson(historyPath, history);

const retentionArtifacts = [];
const addRetention = (kind, relative, bytes = outputs.get(relative) ?? read(relative)) => {
  if (retentionArtifacts.some((item) => item.path === relative)) return;
  retentionArtifacts.push(retained(kind, relative, bytes));
};
addRetention('evaluator-wasm', evaluatorPath, evaluator);
addRetention(
  'evaluator-source-manifest',
  'products/full-embed-evaluator/v1.15.7/manifest.v0.json'
);
addRetention('language-profile-authority', 'product/contracts/lispex-profile-1.5.json');
for (const relative of semanticAuthorityPaths) {
  addRetention('semantic-surface-authority', relative);
}
addRetention('semantic-surface-closure', semanticClosurePath);
addRetention('execution-input', executionInputsPath);
addRetention('machine-contract', machinePath);
addRetention('vectors-manifest', vectorsPath);
for (const item of vectorFiles) addRetention('full-provider-vector', item.path);
for (const relative of [
  'scripts/gen-f15-final-provider.mjs',
  'scripts/check-f15-final-provider.mjs',
  'scripts/gen-f15-full-provider-vectors.mjs',
  'scripts/check-f15-full-provider-vectors.mjs',
  'scripts/check-f15-full-provider-mutations.mjs',
  'scripts/gen-f15-retained-restricted-provider-vectors.mjs',
  'scripts/check-f15-retained-restricted-provider-vectors.mjs',
]) {
  addRetention('provider-tool', relative);
}
addRetention('component-manifest', componentPath);
addRetention('qualification', qualificationPath);
for (const item of evidenceRefs) {
  addRetention(`provider-evidence:${item.kind}`, item.path);
}
addRetention('final-provider-court', courtPath);
addRetention('final-provider-root', rootPath);
for (const [name] of payloads) {
  addRetention(
    'redistribution-member',
    `${distributionDirectory}/${name}`
  );
}
addRetention('redistribution-archive', archivePath, archive);
addRetention('restricted-evaluator', restricted.evaluator_path, restrictedEvaluator);
addRetention(
  'restricted-component-manifest',
  'embedding-readiness/release-dag/components/lispex-embed-evaluator-1.12.4.v1.json'
);
addRetention(
  'restricted-provider-court',
  'docs/bounded-execution/f12-final-provider-court.v1.json'
);
addRetention('restricted-machine-contract', restrictedMachinePath);
addRetention('restricted-vectors-manifest', restrictedVectorsPath);
for (const item of restrictedVectorFiles) {
  addRetention('restricted-provider-vector', item.path);
}
addRetention(
  'restricted-redistribution',
  restricted.redistribution_path,
  restrictedRedistribution
);
addRetention(
  'restricted-component-retention',
  'embedding-readiness/release-dag/retention/lispex-v1.12.4-embed-evaluator.v1.json'
);
addRetention(
  'restricted-redistribution-retention',
  'embedding-readiness/release-dag/retention/lispex-v1.12.4-embed-evaluator-redistribution.v1.json'
);
addRetention('provider-attestation', attestationPath);
addRetention('distribution-attestation', distributionAttestationPath);
addRetention('disposition', dispositionsPath);
addRetention('attestation-history', historyPath);
const retention = {
  schema: 'studio-haze.exact-artifact-retention/v1',
  id: 'lispex-v1.15.8-full-embed-evaluator/v1',
  status: 'project-lifetime-repository-retained',
  owner: 'lispex-release',
  subject: { evaluator_sha256: evaluatorSha, redistribution_archive_sha256: sha(archive) },
  artifacts: retentionArtifacts,
  retention_rules: { addressing: 'exact-sha256', network_required_for_consumption: false, latest_resolution: false, rebuild_substitution: 'forbidden', historical_deletion_or_replacement: 'forbidden', security_revocation: 'append-only-new-use-status-only' },
};
putJson(retentionPath, retention);
const snapshot = {
  schema: 'studio-haze.coordination-snapshot/v1',
  id: 'lispex-1.15.8-full-provider/v1',
  status: 'vendored-exact-provider-material-not-release-build-input',
  policy: reference('embedding-readiness/release-coordination-policy.v1.json'),
  component_manifest: outputReference(componentPath),
  dispositions: outputReference(dispositionsPath),
  attestation_history: outputReference(historyPath),
  retention_record: outputReference(retentionPath),
  final_provider_court: outputReference(courtPath),
  provider_root: outputReference(rootPath),
  relationship_records: [
    outputReference(attestationPath),
    outputReference(distributionAttestationPath),
    reference(
      'embedding-readiness/release-dag/attestations/lispex-embed-evaluator-1.12.4.v1.json'
    ),
  ],
  resolved_artifacts: [
    { id: contracts.component_id, sha256: evaluatorSha },
    {
      id: 'lispex-full-embed-evaluator-redistribution/1',
      sha256: sha(archive),
    },
    { id: restricted.component_id, sha256: restricted.evaluator_sha256 },
    {
      id: 'lispex-embed-evaluator-redistribution/1.12.4',
      sha256: restricted.redistribution_sha256,
    },
    { id: 'topaz-compiler/5.11', sha256: aotCompilerSha },
  ],
  network_required: false,
  uses_latest: false,
  build_input: false,
};
putJson(snapshotPath, snapshot);
const handoffRetentionArtifacts = [
  retained('provider-root', rootPath, outputs.get(rootPath)),
  retained('provider-court', courtPath, outputs.get(courtPath)),
  retained('component-retention', retentionPath, outputs.get(retentionPath)),
  retained('coordination-snapshot', snapshotPath, outputs.get(snapshotPath)),
  retained('provider-attestation', attestationPath, outputs.get(attestationPath)),
  retained(
    'distribution-attestation',
    distributionAttestationPath,
    outputs.get(distributionAttestationPath)
  ),
  retained('disposition', dispositionsPath, outputs.get(dispositionsPath)),
  retained('attestation-history', historyPath, outputs.get(historyPath)),
];
const handoffRetention = {
  schema: 'studio-haze.exact-artifact-retention/v1',
  id: 'lispex-v1.15.8-topaz-component-handoff/v1',
  status: 'project-lifetime-repository-retained',
  owner: 'lispex-release',
  subject: {
    provider_root_sha256: sha(outputs.get(rootPath)),
    coordination_snapshot_sha256: sha(outputs.get(snapshotPath)),
  },
  artifacts: handoffRetentionArtifacts,
  covered_component_retention: outputReference(retentionPath),
  retention_rules: retention.retention_rules,
};
putJson(handoffRetentionPath, handoffRetention);

const handoff = {
  schema: 'studio-haze.lispex-topaz-component-handoff/v1',
  id: 'lispex-f15-topaz-component-handoff/v1',
  status: 'provider-complete-consumer-authority-not-granted',
  provider: 'lispex-release',
  provider_root: outputReference(rootPath),
  provider_court: outputReference(courtPath),
  coordination_snapshot: outputReference(snapshotPath),
  retention: outputReference(handoffRetentionPath),
  bounded_lda_c1: {
    purpose: 'independent Topaz construction of retained restricted artifacts and receipt cores',
    component_id: restricted.component_id,
    evaluator_source_revision: restricted.source_revision,
    artifact: {
      path: restricted.evaluator_path,
      bytes: restricted.evaluator_bytes,
      sha256: restricted.evaluator_sha256,
      imports: 0,
    },
    contracts: restrictedContract.contracts,
    native_artifact_magic: 'LPXART01',
    request_digest_present: false,
    replay_request_present: false,
    whole_envelope_digest_present: false,
    machine_contract: outputReference(restrictedMachinePath),
    vectors: reference(restrictedVectorsPath),
    vector_artifacts: restrictedVectorFiles.map((item) => reference(item.path)),
    provider_component: reference(
      'embedding-readiness/release-dag/components/lispex-embed-evaluator-1.12.4.v1.json'
    ),
    provider_court: reference(
      'docs/bounded-execution/f12-final-provider-court.v1.json'
    ),
    redistribution: {
      path: restricted.redistribution_path,
      sha256: restricted.redistribution_sha256,
      bytes: restrictedRedistribution.length,
    },
    changed: false,
  },
  full_lda_f0: {
    purpose: 'separately qualified complete current-profile component intake input',
    language_profile_authority: {
      id: contracts.language_profile_id,
      ...reference('product/contracts/lispex-profile-1.5.json'),
    },
    executable_profile_id: contracts.semantic_profile_id,
    feature_set_sha256: contracts.feature_set_sha256,
    component_id: contracts.component_id,
    evaluator_source_revision: sourceRevision,
    provider_wire_revision: wireRevision,
    qualification_revision: qualificationRevision,
    cross_architecture_revision: crossArchitectureRevision,
    artifact: {
      path: evaluatorPath,
      bytes: evaluatorBytes,
      sha256: evaluatorSha,
      imports: 0,
    },
    contracts,
    native_artifact_magic: 'LPXFAR01',
    request_digest_present: true,
    replay_request_present: true,
    whole_envelope_digest: 'trailing raw SHA-256 over every preceding artifact byte',
    semantic_surface_closure: outputReference(semanticClosurePath),
    machine_contract: reference(machinePath),
    vectors: reference(vectorsPath),
    vector_artifacts: vectorFiles.map((item) => reference(item.path)),
    component_manifest: outputReference(componentPath),
    qualification: outputReference(qualificationPath),
    provider_attestation: outputReference(attestationPath),
    redistribution_attestation: outputReference(distributionAttestationPath),
    redistribution: {
      path: archivePath,
      sha256: sha(archive),
      bytes: archive.length,
    },
  },
  preserved_relations: {
    aot_topaz_compiler: {
      version: '5.11',
      artifact_sha256: aotCompilerSha,
      changed: false,
    },
    lit_evidence_changed: false,
    restricted_receipts_changed: false,
  },
  authority_not_granted: {
    topaz_adapter_implementation: true,
    topaz_package_implementation: true,
    topaz_target_admission: true,
    topaz_component_admission: true,
    topaz_release: true,
    public_compatibility: true,
    public_distribution: true,
    portable_receipt_issuer: true,
    vouch: true,
    external_action: true,
  },
  graph_rules: {
    component_only: true,
    whole_lispex_release_pin: false,
    source_dependency: false,
    latest_resolution: false,
    mutual_final_manifest_digest_reference: false,
  },
};
putJson(handoffPath, handoff);

const config = {
  schema: 'lispex.full-provider-release-config/v1',
  status: 'exact-offline-pointer-not-release-build-input',
  coordination_snapshot: outputReference(snapshotPath),
  topaz_handoff: outputReference(handoffPath),
  network: false,
  uses_latest: false,
  automatic_discovery: false,
  build_input: false,
};
putJson(configPath, config);
const view = {
  schema: 'lispex.current-full-evaluator-provider/v1',
  status: 'generated-provider-view-never-build-input',
  generated_from: {
    snapshot_sha256: sha(outputs.get(snapshotPath)),
    config_sha256: sha(outputs.get(configPath)),
  },
  provider: {
    component_id: contracts.component_id,
    evaluator_sha256: evaluatorSha,
    evaluator_bytes: evaluatorBytes,
    imports: 0,
    language_profile_id: contracts.language_profile_id,
    executable_profile_id: contracts.semantic_profile_id,
    feature_set_sha256: contracts.feature_set_sha256,
    model_id: contracts.model_id,
    final_provider_court: {
      status: 'passed',
      sha256: sha(outputs.get(courtPath)),
      evidence_digests: evidenceRefs.length,
      safety_precedence_count: 0,
      fresh_instance_passed: true,
    },
    redistribution_sha256: sha(archive),
    topaz_handoff: outputReference(handoffPath),
  },
  retained_restricted_component: {
    artifact_sha256: restricted.evaluator_sha256,
    redistribution_sha256: restricted.redistribution_sha256,
    changed: false,
  },
  aot_component_independence: {
    topaz_compiler_artifact_sha256: aotCompilerSha,
    evaluator_advance_changed_aot_pin: false,
  },
  topaz_consumption_implementation: false,
  build_input: false,
};
putJson(viewPath, view);

for (const [relative, bytes] of outputs) {
  const destination = path.join(root, relative);
  if (mode === '--write') {
    mkdirSync(path.dirname(destination), { recursive: true });
    writeFileSync(destination, bytes);
  } else {
    const actual = readFileSync(destination);
    if (!actual.equals(bytes)) throw new Error(`${relative} is stale`);
  }
}

console.log(
  `F15 final provider ${mode === '--write' ? 'generated' : 'verified'} (${outputs.size} files; evaluator ${evaluatorSha}; redistribution ${sha(archive)}; 7 evidence digests; safety precedence 0)`
);
