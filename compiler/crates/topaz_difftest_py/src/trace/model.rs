use crate::BTreeMap;

#[derive(Debug, Clone)]
pub(crate) struct PyTrace {
    pub(crate) version: u64,
    pub(crate) status: String,
    pub(crate) stdout: Vec<String>,
    pub(crate) files: Vec<TraceFile>,
    pub(crate) defer_errors: Vec<TraceDeferError>,
    pub(crate) fault: Option<TraceFault>,
    pub(crate) value: Option<TraceValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TraceValue {
    Int(i64),
    Bool(bool),
    Null,
    Str(String),
    List(Vec<TraceValue>),
    Some(Box<TraceValue>),
    ResultOk(Box<TraceValue>),
    ResultErr(Box<TraceValue>),
    Record(BTreeMap<String, TraceValue>),
    F64(u64),
    Bytes(String),
    Map(Vec<(TraceValue, TraceValue)>),
    Set(Vec<TraceValue>),
    Enum {
        id: String,
        variant: String,
        index: u64,
        payloads: Vec<TraceValue>,
    },
    Range {
        lo: i64,
        hi: i64,
        inclusive: bool,
        step: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraceFile {
    pub(crate) path: String,
    pub(crate) content: TraceValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraceDeferError {
    pub(crate) rendered: String,
    pub(crate) fault: Option<TraceFault>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraceFault {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) span: TraceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraceSpan {
    pub(crate) file: i64,
    pub(crate) lo: i64,
    pub(crate) hi: i64,
}
