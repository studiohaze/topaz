use crate::limits::{expect_keys, expect_object, limit};
use crate::runtime::EPOCH_TICK;
use crate::runtime::InvocationControl;
use crate::*;

const MAX_APPLICATION_WALL_MILLIS: u64 = 86_400_000;
const CALL_OPEN: u8 = 0;
const CALL_RUNNING: u8 = 1;
const CALL_CANCELLED: u8 = 2;
const CALL_DEADLINE: u8 = 3;
const CALL_SETTLED: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Aggregate concurrency, byte, fuel, cache, and wall-time ceilings for an application.
pub struct ApplicationQuotas {
    pub concurrent_evaluations: u32,
    pub queued_evaluations: u32,
    pub total_evaluations: u64,
    pub aggregate_input_bytes: u64,
    pub aggregate_result_bytes: u64,
    pub aggregate_output_bytes: u64,
    pub aggregate_transcript_bytes: u64,
    pub aggregate_safety_fuel: u64,
    pub prepared_bytes: u64,
    pub wall_millis: u64,
}

impl ApplicationQuotas {
    /// Parses and validates the exact application-quota JSON schema.
    pub fn parse_json(input: &str) -> Result<Self, ApplicationQuotasError> {
        let root = json_parse(input).map_err(|error| {
            ApplicationQuotasError::Json(format!(
                "{} at {}:{}",
                error.message, error.line, error.column
            ))
        })?;
        let root = expect_object(&root, "root")
            .map_err(|error| ApplicationQuotasError::Schema(error_field(error)))?;
        let fields = [
            "schema",
            "concurrent_evaluations",
            "queued_evaluations",
            "total_evaluations",
            "aggregate_input_bytes",
            "aggregate_result_bytes",
            "aggregate_output_bytes",
            "aggregate_transcript_bytes",
            "aggregate_safety_fuel",
            "prepared_bytes",
            "wall_millis",
        ];
        expect_keys(root, &fields, "root")
            .map_err(|error| ApplicationQuotasError::Schema(error_field(error)))?;
        match root.get("schema") {
            Some(JsonValue::String(value)) if value.as_ref() == APPLICATION_QUOTAS_SCHEMA => {}
            _ => return Err(ApplicationQuotasError::Schema("root.schema".into())),
        }
        let read = |key: &str, maximum: u64| {
            limit(root, key, maximum, "root")
                .map_err(|error| ApplicationQuotasError::Schema(error_field(error)))
        };
        let quotas = Self {
            concurrent_evaluations: u32::try_from(read("concurrent_evaluations", u32::MAX.into())?)
                .map_err(|_| {
                    ApplicationQuotasError::Schema("root.concurrent_evaluations".into())
                })?,
            queued_evaluations: u32::try_from(read("queued_evaluations", u32::MAX.into())?)
                .map_err(|_| ApplicationQuotasError::Schema("root.queued_evaluations".into()))?,
            total_evaluations: read("total_evaluations", u64::MAX)?,
            aggregate_input_bytes: read("aggregate_input_bytes", u64::MAX)?,
            aggregate_result_bytes: read("aggregate_result_bytes", u64::MAX)?,
            aggregate_output_bytes: read("aggregate_output_bytes", u64::MAX)?,
            aggregate_transcript_bytes: read("aggregate_transcript_bytes", u64::MAX)?,
            aggregate_safety_fuel: read("aggregate_safety_fuel", u64::MAX)?,
            prepared_bytes: read("prepared_bytes", u64::MAX)?,
            wall_millis: read("wall_millis", MAX_APPLICATION_WALL_MILLIS)?,
        };
        quotas
            .validate()
            .map_err(|error| ApplicationQuotasError::Invalid(error.to_string()))
    }

    fn validate(self) -> Result<Self, ApplicationError> {
        if self.concurrent_evaluations == 0 {
            return Err(ApplicationError::Configuration(
                "concurrent_evaluations must be positive",
            ));
        }
        if self.total_evaluations == 0
            || self.aggregate_input_bytes == 0
            || self.aggregate_result_bytes == 0
            || self.aggregate_output_bytes == 0
            || self.aggregate_transcript_bytes == 0
            || self.aggregate_safety_fuel == 0
            || self.prepared_bytes == 0
        {
            return Err(ApplicationError::Configuration(
                "aggregate application quotas must be positive",
            ));
        }
        if self.aggregate_safety_fuel < SAFETY_FUEL {
            return Err(ApplicationError::Configuration(
                "aggregate_safety_fuel cannot admit one evaluation",
            ));
        }
        if self.wall_millis == 0 || self.wall_millis > MAX_APPLICATION_WALL_MILLIS {
            return Err(ApplicationError::Configuration(
                "wall_millis is outside the supported range",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Syntax, schema, or range failure while admitting application quotas.
pub enum ApplicationQuotasError {
    Json(String),
    Schema(String),
    Invalid(String),
}

impl fmt::Display for ApplicationQuotasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid JSON: {error}"),
            Self::Schema(field) => write!(formatter, "invalid application quota field `{field}`"),
            Self::Invalid(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for ApplicationQuotasError {}

fn error_field(error: LimitsError) -> String {
    match error {
        LimitsError::Schema(field) => field,
        LimitsError::Json(error) => error,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Point-in-time aggregate reservations and usage for an application runtime.
pub struct ApplicationSnapshot {
    pub quotas: ApplicationQuotas,
    pub active_evaluations: u32,
    pub queued_evaluations: u32,
    pub accepted_evaluations: u64,
    pub reserved_input_bytes: u64,
    pub reserved_result_bytes: u64,
    pub reserved_output_bytes: u64,
    pub reserved_transcript_bytes: u64,
    pub reserved_safety_fuel: u64,
    pub prepared_entries: u64,
    pub prepared_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Operational refusal raised before or around evaluator execution.
pub enum OperationalFault {
    Cancelled,
    DeadlineExceeded,
    QueueFull,
    TotalEvaluationsExceeded,
    InputQuotaExceeded,
    ResultQuotaExceeded,
    OutputQuotaExceeded,
    TranscriptQuotaExceeded,
    SafetyFuelQuotaExceeded,
    PreparedBytesQuotaExceeded,
    TokenAlreadyUsed,
}

impl OperationalFault {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline-exceeded",
            Self::QueueFull => "queue-full",
            Self::TotalEvaluationsExceeded => "total-evaluations-exceeded",
            Self::InputQuotaExceeded => "aggregate-input-exceeded",
            Self::ResultQuotaExceeded => "aggregate-result-exceeded",
            Self::OutputQuotaExceeded => "aggregate-output-exceeded",
            Self::TranscriptQuotaExceeded => "aggregate-transcript-exceeded",
            Self::SafetyFuelQuotaExceeded => "aggregate-safety-fuel-exceeded",
            Self::PreparedBytesQuotaExceeded => "prepared-bytes-exceeded",
            Self::TokenAlreadyUsed => "cancellation-token-already-used",
        }
    }
}

impl fmt::Display for OperationalFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Configuration, quota, cancellation, or evaluator failure at the application boundary.
pub enum ApplicationError {
    Configuration(&'static str),
    Operational(OperationalFault),
    Runtime(RunError),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(reason) => write!(formatter, "invalid application quota: {reason}"),
            Self::Operational(reason) => {
                write!(formatter, "application operation refused: {reason}")
            }
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ApplicationError {}

impl From<RunError> for ApplicationError {
    fn from(error: RunError) -> Self {
        match error {
            RunError::Operational(fault) => Self::Operational(fault),
            error => Self::Runtime(error),
        }
    }
}

pub(crate) struct CallControl {
    phase: AtomicU8,
}

impl CallControl {
    pub(crate) fn new() -> Self {
        Self {
            phase: AtomicU8::new(CALL_OPEN),
        }
    }

    pub(crate) fn begin(&self) -> Result<(), OperationalFault> {
        self.phase
            .compare_exchange(CALL_OPEN, CALL_RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|phase| match phase {
                CALL_CANCELLED => OperationalFault::Cancelled,
                CALL_DEADLINE => OperationalFault::DeadlineExceeded,
                _ => OperationalFault::TokenAlreadyUsed,
            })
    }

    fn cancel(&self) -> bool {
        loop {
            let phase = self.phase.load(Ordering::Acquire);
            if matches!(phase, CALL_SETTLED | CALL_CANCELLED) {
                return false;
            }
            if self
                .phase
                .compare_exchange(phase, CALL_CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub(crate) fn expire(&self) -> bool {
        self.phase
            .compare_exchange(
                CALL_RUNNING,
                CALL_DEADLINE,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(crate) fn settle(&self) -> Result<(), OperationalFault> {
        self.phase
            .compare_exchange(
                CALL_RUNNING,
                CALL_SETTLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|phase| match phase {
                CALL_CANCELLED => OperationalFault::Cancelled,
                CALL_DEADLINE => OperationalFault::DeadlineExceeded,
                _ => OperationalFault::TokenAlreadyUsed,
            })
    }

    pub(crate) fn operational_fault(&self) -> Option<OperationalFault> {
        match self.phase.load(Ordering::Acquire) {
            CALL_CANCELLED => Some(OperationalFault::Cancelled),
            CALL_DEADLINE => Some(OperationalFault::DeadlineExceeded),
            _ => None,
        }
    }
}

#[derive(Clone)]
/// Shareable cancellation signal for one queued or active evaluation.
pub struct CancellationToken {
    pub(crate) control: Arc<CallControl>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self {
            control: Arc::new(CallControl::new()),
        }
    }

    /// Requests cancellation and reports whether this call changed the signal.
    pub fn cancel(&self) -> bool {
        self.control.cancel()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CancellationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CancellationToken")
            .field("phase", &self.control.phase.load(Ordering::Acquire))
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PreparedCacheKey {
    component_sha256: &'static str,
    profile_id: &'static str,
    payload_sha256: String,
    payload_len: usize,
}

struct ApplicationState {
    active: u32,
    queued: u32,
    accepted: u64,
    reserved_input: u64,
    reserved_result: u64,
    reserved_output: u64,
    reserved_transcript: u64,
    reserved_safety_fuel: u64,
    prepared_bytes: u64,
    prepared: BTreeMap<PreparedCacheKey, PreparedRule>,
}

struct ApplicationShared {
    quotas: ApplicationQuotas,
    state: Mutex<ApplicationState>,
    capacity: Condvar,
}

#[derive(Clone)]
/// Reusable evaluator wrapped in aggregate quotas and prepared-rule accounting.
pub struct ApplicationRuntime {
    runtime: ReusableRuntime,
    shared: Arc<ApplicationShared>,
}

impl ApplicationRuntime {
    pub fn new(quotas: ApplicationQuotas) -> Result<Self, ApplicationError> {
        Self::with_runtime(quotas, ReusableRuntime::embedded()?)
    }

    /// Create the same aggregate quota, cancellation, concurrency, cleanup,
    /// and prepared-cache envelope around the separately retained complete
    /// current-profile evaluator.
    #[cfg(feature = "full-profile-contract")]
    pub fn full_profile(quotas: ApplicationQuotas) -> Result<Self, ApplicationError> {
        Self::with_runtime(quotas, ReusableRuntime::full_profile()?)
    }

    fn with_runtime(
        quotas: ApplicationQuotas,
        runtime: ReusableRuntime,
    ) -> Result<Self, ApplicationError> {
        let quotas = quotas.validate()?;
        Ok(Self {
            runtime,
            shared: Arc::new(ApplicationShared {
                quotas,
                state: Mutex::new(ApplicationState {
                    active: 0,
                    queued: 0,
                    accepted: 0,
                    reserved_input: 0,
                    reserved_result: 0,
                    reserved_output: 0,
                    reserved_transcript: 0,
                    reserved_safety_fuel: 0,
                    prepared_bytes: 0,
                    prepared: BTreeMap::new(),
                }),
                capacity: Condvar::new(),
            }),
        })
    }

    #[must_use]
    /// Captures current reservations and usage under the runtime's state lock.
    pub fn snapshot(&self) -> ApplicationSnapshot {
        let state = lock_unpoison(&self.shared.state);
        ApplicationSnapshot {
            quotas: self.shared.quotas,
            active_evaluations: state.active,
            queued_evaluations: state.queued,
            accepted_evaluations: state.accepted,
            reserved_input_bytes: state.reserved_input,
            reserved_result_bytes: state.reserved_result,
            reserved_output_bytes: state.reserved_output,
            reserved_transcript_bytes: state.reserved_transcript,
            reserved_safety_fuel: state.reserved_safety_fuel,
            prepared_entries: state.prepared.len() as u64,
            prepared_bytes: state.prepared_bytes,
        }
    }

    /// Evaluates an admitted prepared rule under per-call and aggregate limits.
    pub fn evaluate(
        &self,
        prepared: &PreparedRule,
        input: &[u8],
        limits: EvaluateLimits,
        token: &CancellationToken,
    ) -> Result<RawEvaluation, ApplicationError> {
        self.evaluate_with_safety_fuel(prepared, input, limits, token, SAFETY_FUEL)
    }

    pub(crate) fn evaluate_with_safety_fuel(
        &self,
        prepared: &PreparedRule,
        input: &[u8],
        limits: EvaluateLimits,
        token: &CancellationToken,
        safety_fuel: u64,
    ) -> Result<RawEvaluation, ApplicationError> {
        self.runtime
            .validate_evaluation_request(prepared, input, limits)?;
        token
            .control
            .begin()
            .map_err(ApplicationError::Operational)?;
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(self.shared.quotas.wall_millis))
            .ok_or(ApplicationError::Configuration("wall deadline overflow"))?;
        let _active = self.admit(prepared, input, limits, token, deadline, safety_fuel)?;
        let control = InvocationControl {
            call: Arc::clone(&token.control),
            deadline,
        };
        let result = self.runtime.evaluate_with_safety_fuel(
            prepared,
            input,
            limits,
            safety_fuel,
            Some(control),
        );
        match result {
            Ok(result) => {
                if Instant::now() >= deadline {
                    token.control.expire();
                }
                token
                    .control
                    .settle()
                    .map_err(ApplicationError::Operational)?;
                Ok(result)
            }
            Err(error) => match token.control.operational_fault() {
                Some(fault) => Err(ApplicationError::Operational(fault)),
                None => Err(ApplicationError::from(error)),
            },
        }
    }

    pub(crate) fn admit(
        &self,
        prepared: &PreparedRule,
        input: &[u8],
        limits: EvaluateLimits,
        token: &CancellationToken,
        deadline: Instant,
        safety_fuel: u64,
    ) -> Result<ActiveEvaluation, ApplicationError> {
        let quotas = self.shared.quotas;
        let mut state = lock_unpoison(&self.shared.state);
        if let Some(fault) = token.control.operational_fault() {
            return Err(ApplicationError::Operational(fault));
        }
        checked_reserve(
            state.accepted,
            1,
            quotas.total_evaluations,
            OperationalFault::TotalEvaluationsExceeded,
        )?;
        checked_reserve(
            state.reserved_input,
            input.len() as u64,
            quotas.aggregate_input_bytes,
            OperationalFault::InputQuotaExceeded,
        )?;
        checked_reserve(
            state.reserved_result,
            limits.result_bytes,
            quotas.aggregate_result_bytes,
            OperationalFault::ResultQuotaExceeded,
        )?;
        checked_reserve(
            state.reserved_output,
            limits.output_bytes,
            quotas.aggregate_output_bytes,
            OperationalFault::OutputQuotaExceeded,
        )?;
        checked_reserve(
            state.reserved_transcript,
            limits.transcript_bytes,
            quotas.aggregate_transcript_bytes,
            OperationalFault::TranscriptQuotaExceeded,
        )?;
        checked_reserve(
            state.reserved_safety_fuel,
            safety_fuel,
            quotas.aggregate_safety_fuel,
            OperationalFault::SafetyFuelQuotaExceeded,
        )?;

        let key = PreparedCacheKey {
            component_sha256: prepared.component_sha256,
            profile_id: prepared.profile_id,
            payload_sha256: prepared.payload_sha256.clone(),
            payload_len: prepared.payload.len(),
        };
        let new_prepared_bytes = if state.prepared.contains_key(&key) {
            state.prepared_bytes
        } else {
            checked_reserve(
                state.prepared_bytes,
                prepared.payload.len() as u64,
                quotas.prepared_bytes,
                OperationalFault::PreparedBytesQuotaExceeded,
            )?
        };

        let must_queue = state.active >= quotas.concurrent_evaluations;
        if must_queue && state.queued >= quotas.queued_evaluations {
            return Err(ApplicationError::Operational(OperationalFault::QueueFull));
        }

        state.accepted += 1;
        state.reserved_input += input.len() as u64;
        state.reserved_result += limits.result_bytes;
        state.reserved_output += limits.output_bytes;
        state.reserved_transcript += limits.transcript_bytes;
        state.reserved_safety_fuel += safety_fuel;
        if !state.prepared.contains_key(&key) {
            state.prepared_bytes = new_prepared_bytes;
            state.prepared.insert(key, prepared.clone());
        }

        if !must_queue {
            state.active += 1;
            return Ok(ActiveEvaluation {
                shared: Arc::clone(&self.shared),
            });
        }

        state.queued += 1;
        loop {
            if let Some(fault) = token.control.operational_fault() {
                state.queued -= 1;
                self.shared.capacity.notify_all();
                return Err(ApplicationError::Operational(fault));
            }
            let now = Instant::now();
            if now >= deadline {
                token.control.expire();
                state.queued -= 1;
                self.shared.capacity.notify_all();
                return Err(ApplicationError::Operational(
                    OperationalFault::DeadlineExceeded,
                ));
            }
            if state.active < quotas.concurrent_evaluations {
                state.queued -= 1;
                state.active += 1;
                return Ok(ActiveEvaluation {
                    shared: Arc::clone(&self.shared),
                });
            }
            let wait = deadline.saturating_duration_since(now).min(EPOCH_TICK);
            let (next, _) = self
                .shared
                .capacity
                .wait_timeout(state, wait)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }
}

fn checked_reserve(
    current: u64,
    requested: u64,
    maximum: u64,
    fault: OperationalFault,
) -> Result<u64, ApplicationError> {
    let value = current
        .checked_add(requested)
        .ok_or(ApplicationError::Operational(fault))?;
    if value > maximum {
        Err(ApplicationError::Operational(fault))
    } else {
        Ok(value)
    }
}

fn lock_unpoison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

pub(crate) struct ActiveEvaluation {
    shared: Arc<ApplicationShared>,
}

impl Drop for ActiveEvaluation {
    fn drop(&mut self) {
        let mut state = lock_unpoison(&self.shared.state);
        assert!(state.active > 0, "active evaluation accounting underflow");
        state.active -= 1;
        self.shared.capacity.notify_all();
    }
}
