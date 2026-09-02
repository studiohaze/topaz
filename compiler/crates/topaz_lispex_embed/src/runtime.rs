use crate::application::CallControl;
use crate::protocol::{
    MAX_RESPONSE_BYTES, evaluate_request, parse_response, prepare_request, verify_response_contract,
};
use crate::report::build_report;
use crate::value_codec::{hex_lower, strip_sha256_prefix};
use crate::*;

pub(crate) const EPOCH_TICK: Duration = Duration::from_millis(2);
static BOUNDED_RUNTIME: OnceLock<Result<Runtime, RunError>> = OnceLock::new();
#[cfg(feature = "full-profile-contract")]
static FULL_RUNTIME: OnceLock<Result<Runtime, RunError>> = OnceLock::new();
pub(crate) struct Runtime {
    engine: Engine,
    module: Module,
    _epoch_ticker: JoinHandle<()>,
    component_sha256: &'static str,
    profile_id: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Separates language failure and limit exhaustion from transport and engine errors.
pub enum SettledCategory {
    Complete,
    SemanticFailure,
    LimitExhaustion,
}

impl SettledCategory {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::SemanticFailure => "semantic-failure",
            Self::LimitExhaustion => "limit-exhaustion",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Records whether the convenience `run` path created the required fresh evaluator instances.
pub struct RunRecord {
    pub category: SettledCategory,
    pub operation: &'static str,
    pub code: String,
    pub result: Option<Vec<u8>>,
    pub report_json: String,
    pub fresh_instances: u8,
}

#[derive(Clone, PartialEq, Eq)]
/// Immutable prepared payload bound to evaluator, profile, request, and host identities.
pub struct PreparedRule {
    pub(crate) payload: Arc<[u8]>,
    pub(crate) payload_sha256: String,
    pub(crate) component_sha256: &'static str,
    pub(crate) profile_id: &'static str,
    pub(crate) prepare_request_sha256: String,
    pub(crate) prepare_code: String,
    pub(crate) binding_digests: [Option<String>; 6],
}

impl fmt::Debug for PreparedRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRule")
            .field("payload_sha256", &self.payload_sha256)
            .field("payload_len", &self.payload.len())
            .field("component_sha256", &self.component_sha256)
            .field("profile_id", &self.profile_id)
            .field("prepare_request_sha256", &self.prepare_request_sha256)
            .finish_non_exhaustive()
    }
}

impl PreparedRule {
    #[must_use]
    pub fn component_sha256(&self) -> &str {
        self.component_sha256
    }

    #[must_use]
    pub fn profile_id(&self) -> &str {
        self.profile_id
    }

    #[must_use]
    pub fn prepare_request_sha256(&self) -> &str {
        &self.prepare_request_sha256
    }

    #[must_use]
    pub fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    #[must_use]
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Limit exhaustion remains a semantic outcome rather than a runtime error.
pub struct PrepareLimitExhaustion {
    pub code: String,
    pub request_sha256: String,
    pub fresh_instances: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Successful prepared rule or a semantic preparation limit exhaustion.
pub enum PrepareOutcome {
    Prepared(Box<PreparedRule>),
    LimitExhaustion(PrepareLimitExhaustion),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Exact evaluator response and request bytes before consumer-artifact wrapping.
pub struct RawEvaluation {
    pub category: SettledCategory,
    pub code: String,
    pub result: Option<Vec<u8>>,
    pub request_sha256: String,
    pub fresh_instances: u8,
    /// Exact canonical evaluator request retained for consumer-artifact
    /// construction. It is never placed directly in a portable core.
    pub request_bytes: Vec<u8>,
    /// Exact raw evaluator response retained for one-pass evidence wrapping.
    pub response_bytes: Vec<u8>,
    pub(crate) usage: Option<[u64; 9]>,
}

#[derive(Clone)]
pub(crate) struct InvocationControl {
    pub(crate) call: Arc<CallControl>,
    pub(crate) deadline: Instant,
}

#[derive(Clone, Copy)]
/// Reuse retains a fixed component pointer and cannot trigger profile selection.
pub struct ReusableRuntime {
    runtime: &'static Runtime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Closed refusal, contract, engine, and operational failures of the embedded runtime.
pub enum RunError {
    InputRefusal(&'static str),
    RequestRefusal(String),
    SelectionRefusal(&'static str),
    ContractViolation(&'static str),
    EngineFault(&'static str),
    Operational(OperationalFault),
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputRefusal(reason) => write!(formatter, "input refused: {reason}"),
            Self::RequestRefusal(code) => write!(formatter, "evaluator request refused: {code}"),
            Self::SelectionRefusal(field) => {
                write!(formatter, "embedding selection refused: {field}")
            }
            Self::ContractViolation(reason) => {
                write!(formatter, "embedding contract violation: {reason}")
            }
            Self::EngineFault(reason) => write!(formatter, "embedded evaluator fault: {reason}"),
            Self::Operational(reason) => {
                write!(formatter, "application operation refused: {reason}")
            }
        }
    }
}

impl std::error::Error for RunError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Prepare,
    Evaluate,
}

impl Operation {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Evaluate => "evaluate",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    Prepared,
    Complete,
    SemanticFailure,
    LimitExhaustion,
    RequestRefusal,
    EngineFault,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) operation: Operation,
    pub(crate) category: Category,
    pub(crate) code: String,
    pub(crate) payload: Vec<u8>,
    pub(crate) digests: [Option<String>; 6],
    pub(crate) usage: Option<[u64; 9]>,
}

pub(crate) struct ReportInputs<'a> {
    pub(crate) source: &'a [u8],
    pub(crate) input: &'a [u8],
    pub(crate) limits: Limits,
    pub(crate) prepare_request_sha256: &'a str,
    pub(crate) prepare_code: &'a str,
    pub(crate) evaluate: Option<&'a RawEvaluation>,
    pub(crate) safety_fuel: u64,
}

impl ReusableRuntime {
    /// Admits and returns the pinned bounded evaluator runtime.
    pub fn embedded() -> Result<Self, RunError> {
        verify_embedded_evaluator()?;
        let runtime = runtime()?;
        if runtime.component_sha256 != EVALUATOR_SHA256 {
            return Err(RunError::SelectionRefusal("runtime-component-digest"));
        }
        Ok(Self { runtime })
    }

    #[must_use]
    pub const fn profile_id(self) -> &'static str {
        self.runtime.profile_id
    }

    #[must_use]
    pub const fn evaluator_sha256(self) -> &'static str {
        self.runtime.component_sha256
    }

    /// Select the exact retained complete-current-profile component. This is
    /// a separate constructor so the immutable 5.18 bounded identity cannot
    /// be widened by a selector or fallback.
    #[cfg(feature = "full-profile-contract")]
    pub fn full_profile() -> Result<Self, RunError> {
        verify_full_embedded_evaluator()?;
        let runtime = full_runtime()?;
        if runtime.component_sha256 != FULL_EVALUATOR_SHA256
            || runtime.profile_id != FULL_PROFILE_ID
        {
            return Err(RunError::SelectionRefusal("runtime-component-digest"));
        }
        Ok(Self { runtime })
    }

    /// Load a provider-produced full-profile preparation artifact without
    /// re-running preparation. The caller must supply the exact lock-bound
    /// request digest; bounded LPXART01 bytes are rejected by the LPXFAR01
    /// decoder before any payload can enter this runtime.
    #[cfg(feature = "full-profile-contract")]
    pub fn load_full_prepared_consumer_artifact(
        self,
        bytes: &[u8],
        preparation_request_sha256: &str,
    ) -> Result<PreparedRule, RunError> {
        if self.runtime.profile_id != FULL_PROFILE_ID {
            return Err(RunError::SelectionRefusal("prepared-profile"));
        }
        let artifact = decode_full_artifact(bytes)
            .map_err(|error| RunError::ContractViolation(error.code()))?;
        if artifact.kind != FullArtifactKind::Prepare
            || artifact.category != FullArtifactCategory::Prepared
            || artifact.evaluator_sha256 != FULL_EVALUATOR_SHA256
            || hex_lower(&artifact.request_sha256)
                != strip_sha256_prefix(preparation_request_sha256)?
        {
            return Err(RunError::SelectionRefusal("prepared-artifact-identity"));
        }
        let response = parse_response(&artifact.response)?;
        self.load_prepared(
            response,
            strip_sha256_prefix(preparation_request_sha256)?.to_string(),
        )
    }

    /// Load an exact package-produced prepared artifact without executing
    /// preparation again. The caller supplies both lock-bound request
    /// identities, and the canonical consumer artifact must bind the same
    /// submission before its immutable prepared payload is admitted.
    pub fn load_prepared_consumer_artifact(
        self,
        bytes: &[u8],
        preparation_request_sha256: &str,
        preparation_submission_sha256: &str,
    ) -> Result<PreparedRule, RunError> {
        verify_artifact(bytes).map_err(|error| RunError::ContractViolation(error.code()))?;
        let artifact =
            decode_artifact(bytes).map_err(|error| RunError::ContractViolation(error.code()))?;
        if artifact.kind != ArtifactKind::Prepare
            || artifact.category != ArtifactCategory::Prepared
            || artifact.evaluator_sha256 != EVALUATOR_SHA256
            || artifact.identities[5].as_deref()
                != Some(strip_sha256_prefix(preparation_submission_sha256)?)
        {
            return Err(RunError::SelectionRefusal("prepared-artifact-identity"));
        }
        let response = parse_response(&artifact.response)?;
        self.load_prepared(
            response,
            strip_sha256_prefix(preparation_request_sha256)?.to_string(),
        )
    }

    /// Prepares source under exact caller limits and the product safety-fuel ceiling.
    pub fn prepare(self, source: &[u8], limits: PrepareLimits) -> Result<PrepareOutcome, RunError> {
        self.prepare_with_safety_fuel(source, limits, SAFETY_FUEL)
    }

    fn prepare_with_safety_fuel(
        self,
        source: &[u8],
        limits: PrepareLimits,
        safety_fuel: u64,
    ) -> Result<PrepareOutcome, RunError> {
        if source.len() as u64 > limits.raw_source_bytes {
            return Err(RunError::InputRefusal("source exceeds raw_source_bytes"));
        }
        let request = prepare_request(source, limits)?;
        let request_sha256 = sha256_hex(&request);
        let response = parse_response(&invoke(
            self.runtime,
            Operation::Prepare,
            &request,
            safety_fuel,
            None,
        )?)?;
        verify_response_contract(&response)?;
        match response.category {
            Category::Prepared => Ok(PrepareOutcome::Prepared(Box::new(
                self.load_prepared(response, request_sha256)?,
            ))),
            Category::LimitExhaustion => {
                Ok(PrepareOutcome::LimitExhaustion(PrepareLimitExhaustion {
                    code: response.code,
                    request_sha256,
                    fresh_instances: 1,
                }))
            }
            Category::RequestRefusal => Err(RunError::RequestRefusal(response.code)),
            Category::EngineFault => Err(RunError::EngineFault("provider-engine-fault")),
            _ => Err(RunError::ContractViolation(
                "prepare returned a non-prepare category",
            )),
        }
    }

    fn load_prepared(
        self,
        response: Response,
        prepare_request_sha256: String,
    ) -> Result<PreparedRule, RunError> {
        if response.operation != Operation::Prepare
            || response.category != Category::Prepared
            || response.code != "prepared"
            || response.payload.is_empty()
        {
            return Err(RunError::ContractViolation(
                "prepared payload loader received an invalid response",
            ));
        }
        let payload_sha256 = sha256_hex(&response.payload);
        Ok(PreparedRule {
            payload: Arc::from(response.payload),
            payload_sha256,
            component_sha256: self.runtime.component_sha256,
            profile_id: self.runtime.profile_id,
            prepare_request_sha256,
            prepare_code: response.code,
            binding_digests: response.digests,
        })
    }

    /// Evaluates an admitted prepared rule without re-running preparation.
    pub fn evaluate(
        self,
        prepared: &PreparedRule,
        input: &[u8],
        limits: EvaluateLimits,
    ) -> Result<RawEvaluation, RunError> {
        self.evaluate_with_safety_fuel(prepared, input, limits, SAFETY_FUEL, None)
    }

    pub(crate) fn evaluate_with_safety_fuel(
        self,
        prepared: &PreparedRule,
        input: &[u8],
        limits: EvaluateLimits,
        safety_fuel: u64,
        control: Option<InvocationControl>,
    ) -> Result<RawEvaluation, RunError> {
        self.validate_evaluation_request(prepared, input, limits)?;
        let request = evaluate_request(&prepared.payload, input, limits)?;
        let request_sha256 = sha256_hex(&request);
        let response_bytes = invoke(
            self.runtime,
            Operation::Evaluate,
            &request,
            safety_fuel,
            control,
        )?;
        let response = parse_response(&response_bytes)?;
        verify_response_contract(&response)?;
        let category = match response.category {
            Category::RequestRefusal => return Err(RunError::RequestRefusal(response.code)),
            Category::EngineFault => return Err(RunError::EngineFault("provider-engine-fault")),
            Category::Complete => SettledCategory::Complete,
            Category::SemanticFailure => SettledCategory::SemanticFailure,
            Category::LimitExhaustion => SettledCategory::LimitExhaustion,
            Category::Prepared => {
                return Err(RunError::ContractViolation(
                    "evaluate returned a prepare category",
                ));
            }
        };
        if matches!(
            response.category,
            Category::Complete | Category::SemanticFailure
        ) && (response.digests[0] != prepared.binding_digests[3]
            || response.digests[1] != prepared.binding_digests[4])
        {
            return Err(RunError::ContractViolation(
                "evaluation is not bound to the prepared rule",
            ));
        }
        if response.category == Category::Complete {
            validate_value(&response.payload, false).map_err(RunError::ContractViolation)?;
        }
        Ok(RawEvaluation {
            category,
            code: response.code,
            result: (category == SettledCategory::Complete).then_some(response.payload),
            request_sha256,
            fresh_instances: 1,
            request_bytes: request,
            response_bytes,
            usage: response.usage,
        })
    }

    pub(crate) fn validate_evaluation_request(
        self,
        prepared: &PreparedRule,
        input: &[u8],
        limits: EvaluateLimits,
    ) -> Result<(), RunError> {
        if prepared.component_sha256 != self.runtime.component_sha256 {
            return Err(RunError::SelectionRefusal("prepared-component-digest"));
        }
        if prepared.profile_id != self.runtime.profile_id {
            return Err(RunError::SelectionRefusal("prepared-profile"));
        }
        if input.len() as u64 > limits.canonical_input_bytes {
            return Err(RunError::InputRefusal(
                "input exceeds canonical_input_bytes",
            ));
        }
        validate_value(input, true).map_err(RunError::InputRefusal)?;
        Ok(())
    }
}

/// Build the exact provider-defined preparation request without executing the
/// evaluator. Package locking records this digest so later verification never
/// needs to re-prepare a rule.
pub fn preparation_request_sha256(
    source: &[u8],
    limits: PrepareLimits,
) -> Result<String, RunError> {
    if source.len() as u64 > limits.raw_source_bytes {
        return Err(RunError::InputRefusal("source exceeds raw_source_bytes"));
    }
    prepare_request(source, limits).map(|request| sha256_hex(&request))
}

/// Derives the canonical submission identity used by locked prepared artifacts.
pub fn preparation_submission_sha256(
    source: &[u8],
    limits: PrepareLimits,
) -> Result<String, RunError> {
    if source.len() as u64 > limits.raw_source_bytes {
        return Err(RunError::InputRefusal("source exceeds raw_source_bytes"));
    }
    let request = prepare_request(source, limits)?;
    artifact::submission_sha256(&request).map_err(|error| RunError::ContractViolation(error.code()))
}

/// Execute one bounded preparation and wrap its raw response in the canonical
/// `LPXART01` consumer artifact. This is a package-lock operation. Loading or
/// evaluating the resulting bytes is a separate step and never happens as a
/// fallback during locked verification.
pub fn prepare_consumer_artifact(
    source: &[u8],
    limits: PrepareLimits,
) -> Result<Vec<u8>, RunError> {
    if source.len() as u64 > limits.raw_source_bytes {
        return Err(RunError::InputRefusal("source exceeds raw_source_bytes"));
    }
    let request = prepare_request(source, limits)?;
    let runtime = ReusableRuntime::embedded()?;
    let response = invoke(
        runtime.runtime,
        Operation::Prepare,
        &request,
        SAFETY_FUEL,
        None,
    )?;
    wrap_prepare_artifact(
        &response,
        &request,
        [
            limits.raw_source_bytes,
            limits.prepare_work,
            limits.logical_allocation,
            limits.syntax_depth,
        ],
    )
    .map_err(|error| RunError::ContractViolation(error.code()))
}

/// Execute one preparation with the separately retained complete-current-
/// profile component and return its exact LPXFAR01 consumer artifact.
#[cfg(feature = "full-profile-contract")]
pub fn prepare_full_consumer_artifact(
    source: &[u8],
    limits: PrepareLimits,
) -> Result<Vec<u8>, RunError> {
    if source.len() as u64 > limits.raw_source_bytes {
        return Err(RunError::InputRefusal("source exceeds raw_source_bytes"));
    }
    let request = prepare_request(source, limits)?;
    let runtime = ReusableRuntime::full_profile()?;
    let response = invoke(
        runtime.runtime,
        Operation::Prepare,
        &request,
        SAFETY_FUEL,
        None,
    )?;
    wrap_full_prepare_artifact(
        &response,
        &request,
        [
            limits.raw_source_bytes,
            limits.prepare_work,
            limits.logical_allocation,
            limits.syntax_depth,
        ],
    )
    .map_err(|error| RunError::ContractViolation(error.code()))
}

/// Prepares and evaluates one source and input pair through the bounded runtime.
pub fn run(source: &[u8], input: &[u8], limits: Limits) -> Result<RunRecord, RunError> {
    run_with_safety_fuel(source, input, limits, SAFETY_FUEL)
}

pub(crate) fn run_with_safety_fuel(
    source: &[u8],
    input: &[u8],
    limits: Limits,
    safety_fuel: u64,
) -> Result<RunRecord, RunError> {
    if source.len() as u64 > limits.prepare.raw_source_bytes {
        return Err(RunError::InputRefusal("source exceeds raw_source_bytes"));
    }
    if input.len() as u64 > limits.evaluate.canonical_input_bytes {
        return Err(RunError::InputRefusal(
            "input exceeds canonical_input_bytes",
        ));
    }
    validate_value(input, true).map_err(RunError::InputRefusal)?;
    let runtime = ReusableRuntime::embedded()?;
    let prepared = match runtime.prepare_with_safety_fuel(source, limits.prepare, safety_fuel)? {
        PrepareOutcome::LimitExhaustion(exhaustion) => {
            return Ok(build_report(ReportInputs {
                source,
                input,
                limits,
                prepare_request_sha256: &exhaustion.request_sha256,
                prepare_code: &exhaustion.code,
                evaluate: None,
                safety_fuel,
            }));
        }
        PrepareOutcome::Prepared(prepared) => prepared,
    };
    let evaluate =
        runtime.evaluate_with_safety_fuel(&prepared, input, limits.evaluate, safety_fuel, None)?;
    Ok(build_report(ReportInputs {
        source,
        input,
        limits,
        prepare_request_sha256: prepared.prepare_request_sha256(),
        prepare_code: &prepared.prepare_code,
        evaluate: Some(&evaluate),
        safety_fuel,
    }))
}

pub(crate) fn runtime() -> Result<&'static Runtime, RunError> {
    match BOUNDED_RUNTIME
        .get_or_init(|| build_runtime(EVALUATOR_BYTES, EVALUATOR_SHA256, PROFILE_ID, 19))
    {
        Ok(runtime) => Ok(runtime),
        Err(error) => Err(error.clone()),
    }
}

#[cfg(feature = "full-profile-contract")]
fn full_runtime() -> Result<&'static Runtime, RunError> {
    match FULL_RUNTIME.get_or_init(|| {
        build_runtime(
            full_artifact::FULL_EVALUATOR_BYTES,
            FULL_EVALUATOR_SHA256,
            FULL_PROFILE_ID,
            19,
        )
    }) {
        Ok(runtime) => Ok(runtime),
        Err(error) => Err(error.clone()),
    }
}

fn build_runtime(
    evaluator_bytes: &[u8],
    component_sha256: &'static str,
    profile_id: &'static str,
    initial_memory_pages: u64,
) -> Result<Runtime, RunError> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    let engine = Engine::new(&config).map_err(|_| RunError::EngineFault("runtime-init"))?;
    let module = Module::from_binary(&engine, evaluator_bytes)
        .map_err(|_| RunError::EngineFault("module"))?;
    check_module_surface(&module, initial_memory_pages)?;
    let epoch_engine = engine.clone();
    let epoch_ticker = thread::Builder::new()
        .name("topaz-lispex-epoch".to_string())
        .spawn(move || {
            loop {
                thread::sleep(EPOCH_TICK);
                epoch_engine.increment_epoch();
            }
        })
        .map_err(|_| RunError::EngineFault("runtime-init"))?;
    Ok(Runtime {
        engine,
        module,
        _epoch_ticker: epoch_ticker,
        component_sha256,
        profile_id,
    })
}

pub(crate) fn invoke(
    runtime: &Runtime,
    operation: Operation,
    request: &[u8],
    safety_fuel: u64,
    control: Option<InvocationControl>,
) -> Result<Vec<u8>, RunError> {
    let mut store = Store::new(&runtime.engine, ());
    store
        .set_fuel(safety_fuel)
        .map_err(|_| RunError::EngineFault("safety-fuel-configuration"))?;
    if let Some(control) = control.as_ref() {
        let callback_call = Arc::clone(&control.call);
        let deadline = control.deadline;
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(move |_| {
            if Instant::now() >= deadline {
                callback_call.expire();
            }
            match callback_call.operational_fault() {
                Some(_) => Ok(UpdateDeadline::Interrupt),
                None => Ok(UpdateDeadline::Continue(1)),
            }
        });
    } else {
        store.set_epoch_deadline(u64::MAX / 2);
    }
    let control_call = control.as_ref().map(|control| Arc::clone(&control.call));
    let instance = Instance::new(&mut store, &runtime.module, &[])
        .map_err(|_| engine_call_error(&store, control_call.as_deref()))?;
    invoke_instance(operation, request, instance, store, control_call.as_deref())
}

fn invoke_instance(
    operation: Operation,
    request: &[u8],
    instance: Instance,
    mut store: Store<()>,
    control: Option<&CallControl>,
) -> Result<Vec<u8>, RunError> {
    let version = instance
        .get_typed_func::<(), u32>(&mut store, "lispex_embed_abi_version")
        .map_err(|_| RunError::ContractViolation("ABI version export missing"))?
        .call(&mut store, ())
        .map_err(|_| engine_call_error(&store, control))?;
    if version != ABI_VERSION {
        return Err(RunError::ContractViolation("ABI version mismatch"));
    }
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut store, "lispex_embed_alloc")
        .map_err(|_| RunError::ContractViolation("allocator export mismatch"))?;
    let dealloc = instance
        .get_typed_func::<(u32, u32), u32>(&mut store, "lispex_embed_dealloc")
        .map_err(|_| RunError::ContractViolation("deallocator export mismatch"))?;
    let operation = instance
        .get_typed_func::<(u32, u32), u64>(
            &mut store,
            match operation {
                Operation::Prepare => "lispex_embed_prepare",
                Operation::Evaluate => "lispex_embed_evaluate",
            },
        )
        .map_err(|_| RunError::ContractViolation("operation export mismatch"))?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or(RunError::ContractViolation("memory export missing"))?;
    let request_len = u32::try_from(request.len())
        .map_err(|_| RunError::ContractViolation("request exceeds canonical u32"))?;
    let request_ptr = alloc
        .call(&mut store, request_len)
        .map_err(|_| engine_call_error(&store, control))?;
    if request_ptr == 0 {
        return Err(RunError::EngineFault("allocator-refusal"));
    }
    memory
        .write(&mut store, request_ptr as usize, request)
        .map_err(|_| RunError::EngineFault("request-memory-write"))?;
    let packed = operation
        .call(&mut store, (request_ptr, request_len))
        .map_err(|_| engine_call_error(&store, control))?;
    if packed == 0 {
        return Err(RunError::EngineFault("empty-response-handle"));
    }
    let response_ptr = (packed >> 32) as u32;
    let response_len = packed as u32;
    if response_len as usize > MAX_RESPONSE_BYTES {
        return Err(RunError::EngineFault("response-size"));
    }
    let mut response = vec![0; response_len as usize];
    memory
        .read(&store, response_ptr as usize, &mut response)
        .map_err(|_| RunError::EngineFault("response-memory-read"))?;
    if dealloc
        .call(&mut store, (response_ptr, response_len))
        .map_err(|_| engine_call_error(&store, control))?
        != 1
    {
        return Err(RunError::EngineFault("response-deallocation"));
    }
    Ok(response)
}

fn engine_call_error(store: &Store<()>, control: Option<&CallControl>) -> RunError {
    if let Some(fault) = control.and_then(CallControl::operational_fault) {
        RunError::Operational(fault)
    } else if store.get_fuel().ok() == Some(0) {
        RunError::EngineFault("safety-fuel-exhausted")
    } else {
        RunError::EngineFault("wasm-trap")
    }
}

pub(crate) fn verify_embedded_evaluator() -> Result<(), RunError> {
    if sha256_hex(EVALUATOR_BYTES) != EVALUATOR_SHA256 {
        return Err(RunError::SelectionRefusal("artifact-digest"));
    }
    Ok(())
}

#[cfg(feature = "full-profile-contract")]
fn verify_full_embedded_evaluator() -> Result<(), RunError> {
    if sha256_hex(full_artifact::FULL_EVALUATOR_BYTES) != FULL_EVALUATOR_SHA256 {
        return Err(RunError::SelectionRefusal("artifact-digest"));
    }
    Ok(())
}

fn check_module_surface(module: &Module, initial_memory_pages: u64) -> Result<(), RunError> {
    if module.imports().next().is_some() {
        return Err(RunError::ContractViolation(
            "evaluator imports a host capability",
        ));
    }
    let mut functions = Vec::new();
    let mut memory = None;
    for export in module.exports() {
        match export.ty() {
            ExternType::Func(_) => functions.push(export.name().to_string()),
            ExternType::Memory(memory_type) if export.name() == "memory" && memory.is_none() => {
                memory = Some((memory_type.minimum(), memory_type.maximum()));
            }
            ExternType::Global(_) if matches!(export.name(), "__data_end" | "__heap_base") => {}
            _ => {
                return Err(RunError::ContractViolation(
                    "evaluator export surface mismatch",
                ));
            }
        }
    }
    functions.sort();
    let mut expected = [
        "lispex_embed_abi_version",
        "lispex_embed_alloc",
        "lispex_embed_dealloc",
        "lispex_embed_evaluate",
        "lispex_embed_prepare",
    ]
    .map(str::to_string)
    .to_vec();
    expected.sort();
    if functions != expected || memory != Some((initial_memory_pages, Some(256))) {
        return Err(RunError::ContractViolation(
            "evaluator ABI or memory surface mismatch",
        ));
    }
    Ok(())
}
