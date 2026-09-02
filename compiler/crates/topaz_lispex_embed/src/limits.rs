use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Preparation limits are fixed before source bytes reach the evaluator.
pub struct PrepareLimits {
    pub raw_source_bytes: u64,
    pub prepare_work: u64,
    pub logical_allocation: u64,
    pub syntax_depth: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Evaluation accounts semantic work separately from observable output and result bytes.
pub struct EvaluateLimits {
    pub canonical_input_bytes: u64,
    pub eval_work: u64,
    pub logical_allocation: u64,
    pub semantic_frames: u64,
    pub traversal_depth: u64,
    pub output_bytes: u64,
    pub diagnostic_bytes: u64,
    pub transcript_bytes: u64,
    pub transcript_events: u64,
    pub result_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Keeping both phases together prevents evaluation from inheriting ambient defaults.
pub struct Limits {
    pub prepare: PrepareLimits,
    pub evaluate: EvaluateLimits,
}

impl Limits {
    pub const MAXIMUM: Self = Self {
        prepare: PrepareLimits {
            raw_source_bytes: 4_096,
            prepare_work: 1_000_000,
            logical_allocation: 1_000_000,
            syntax_depth: 64,
        },
        evaluate: EvaluateLimits {
            canonical_input_bytes: 4_096,
            eval_work: 1_000_000,
            logical_allocation: 1_000_000,
            semantic_frames: 1_000,
            traversal_depth: 256,
            output_bytes: 1_000_000,
            diagnostic_bytes: 1_000_000,
            transcript_bytes: 1_000_000,
            transcript_events: 100,
            result_bytes: 1_000_000,
        },
    };

    /// Parses the exact limits schema and rejects values above the product maxima.
    pub fn parse_json(input: &str) -> Result<Self, LimitsError> {
        let root = json_parse(input).map_err(|error| {
            LimitsError::Json(format!(
                "{} at {}:{}",
                error.message, error.line, error.column
            ))
        })?;
        let root = expect_object(&root, "root")?;
        expect_keys(root, &["schema", "prepare", "evaluate"], "root")?;
        match root.get("schema") {
            Some(JsonValue::String(value)) if value.as_ref() == LIMITS_SCHEMA => {}
            _ => return Err(LimitsError::Schema("root.schema".into())),
        }
        let prepare = expect_object(
            root.get("prepare")
                .ok_or_else(|| LimitsError::Schema("root.prepare".into()))?,
            "prepare",
        )?;
        expect_keys(
            prepare,
            &[
                "raw_source_bytes",
                "prepare_work",
                "logical_allocation",
                "syntax_depth",
            ],
            "prepare",
        )?;
        let evaluate = expect_object(
            root.get("evaluate")
                .ok_or_else(|| LimitsError::Schema("root.evaluate".into()))?,
            "evaluate",
        )?;
        expect_keys(
            evaluate,
            &[
                "canonical_input_bytes",
                "eval_work",
                "logical_allocation",
                "semantic_frames",
                "traversal_depth",
                "output_bytes",
                "diagnostic_bytes",
                "transcript_bytes",
                "transcript_events",
                "result_bytes",
            ],
            "evaluate",
        )?;
        Ok(Self {
            prepare: PrepareLimits {
                raw_source_bytes: limit(
                    prepare,
                    "raw_source_bytes",
                    Self::MAXIMUM.prepare.raw_source_bytes,
                    "prepare",
                )?,
                prepare_work: limit(
                    prepare,
                    "prepare_work",
                    Self::MAXIMUM.prepare.prepare_work,
                    "prepare",
                )?,
                logical_allocation: limit(
                    prepare,
                    "logical_allocation",
                    Self::MAXIMUM.prepare.logical_allocation,
                    "prepare",
                )?,
                syntax_depth: limit(
                    prepare,
                    "syntax_depth",
                    Self::MAXIMUM.prepare.syntax_depth,
                    "prepare",
                )?,
            },
            evaluate: EvaluateLimits {
                canonical_input_bytes: limit(
                    evaluate,
                    "canonical_input_bytes",
                    Self::MAXIMUM.evaluate.canonical_input_bytes,
                    "evaluate",
                )?,
                eval_work: limit(
                    evaluate,
                    "eval_work",
                    Self::MAXIMUM.evaluate.eval_work,
                    "evaluate",
                )?,
                logical_allocation: limit(
                    evaluate,
                    "logical_allocation",
                    Self::MAXIMUM.evaluate.logical_allocation,
                    "evaluate",
                )?,
                semantic_frames: limit(
                    evaluate,
                    "semantic_frames",
                    Self::MAXIMUM.evaluate.semantic_frames,
                    "evaluate",
                )?,
                traversal_depth: limit(
                    evaluate,
                    "traversal_depth",
                    Self::MAXIMUM.evaluate.traversal_depth,
                    "evaluate",
                )?,
                output_bytes: limit(
                    evaluate,
                    "output_bytes",
                    Self::MAXIMUM.evaluate.output_bytes,
                    "evaluate",
                )?,
                diagnostic_bytes: limit(
                    evaluate,
                    "diagnostic_bytes",
                    Self::MAXIMUM.evaluate.diagnostic_bytes,
                    "evaluate",
                )?,
                transcript_bytes: limit(
                    evaluate,
                    "transcript_bytes",
                    Self::MAXIMUM.evaluate.transcript_bytes,
                    "evaluate",
                )?,
                transcript_events: limit(
                    evaluate,
                    "transcript_events",
                    Self::MAXIMUM.evaluate.transcript_events,
                    "evaluate",
                )?,
                result_bytes: limit(
                    evaluate,
                    "result_bytes",
                    Self::MAXIMUM.evaluate.result_bytes,
                    "evaluate",
                )?,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// JSON or field-schema failure while admitting evaluator limits.
pub enum LimitsError {
    Json(String),
    Schema(String),
}

impl fmt::Display for LimitsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid JSON: {error}"),
            Self::Schema(field) => write!(formatter, "invalid limits field `{field}`"),
        }
    }
}

impl std::error::Error for LimitsError {}

pub(crate) fn expect_object<'a>(
    value: &'a JsonValue,
    path: &str,
) -> Result<&'a BTreeMap<Rc<str>, JsonValue>, LimitsError> {
    match value {
        JsonValue::Object(entries) => Ok(entries),
        _ => Err(LimitsError::Schema(path.into())),
    }
}

pub(crate) fn expect_keys(
    entries: &BTreeMap<Rc<str>, JsonValue>,
    expected: &[&str],
    path: &str,
) -> Result<(), LimitsError> {
    if entries.len() != expected.len() || !expected.iter().all(|key| entries.contains_key(*key)) {
        return Err(LimitsError::Schema(path.into()));
    }
    Ok(())
}

pub(crate) fn limit(
    entries: &BTreeMap<Rc<str>, JsonValue>,
    key: &str,
    maximum: u64,
    parent: &str,
) -> Result<u64, LimitsError> {
    let path = format!("{parent}.{key}");
    let JsonValue::Number(number) = entries
        .get(key)
        .ok_or_else(|| LimitsError::Schema(path.clone()))?
    else {
        return Err(LimitsError::Schema(path));
    };
    if number.lexeme.is_empty() || !number.lexeme.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LimitsError::Schema(path));
    }
    let value = number
        .lexeme
        .parse::<u64>()
        .map_err(|_| LimitsError::Schema(path.clone()))?;
    if value > maximum {
        return Err(LimitsError::Schema(path));
    }
    Ok(value)
}
