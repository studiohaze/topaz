use super::environment::*;
use crate::diagnostic::*;
use crate::program::model::*;
use crate::wire::*;
use crate::*;

#[derive(Clone)]
pub(crate) enum RuntimeValue {
    Data(Value),
    Function {
        operation: usize,
        environment: Environment,
    },
    Type(String),
    EnumConstructor {
        identity: String,
        variant: String,
        variant_index: u32,
        arity: usize,
    },
}

pub(crate) struct ProductClosure {
    pub(crate) operation: usize,
    pub(crate) environment: Environment,
}

impl std::fmt::Debug for ProductClosure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductClosure")
            .field("operation", &self.operation)
            .finish_non_exhaustive()
    }
}

impl TpzCall for ProductClosure {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn name(&self) -> Option<&str> {
        None
    }

    // Like the AST interpreter's closure carrier, this value is driven by its
    // owning machine after downcast. It never enters the emitted async ABI.
    fn call(&self, _cx: RtCx, _args: Vec<Value>) -> CallFuture {
        Box::pin(async {
            Err(RtError {
                code: topaz_value::codes::GUARD_UNIMPLEMENTED,
                message: "self target closures run on the product machine, not the async call ABI"
                    .to_string(),
                span: Span::new(FileId(0), 0, 0),
            })
        })
    }

    fn arity(&self) -> usize {
        0
    }

    fn param_name(&self, _n: usize) -> Option<&str> {
        None
    }
}

pub(crate) fn product_closure(operation: usize, environment: Environment) -> Value {
    Value::Closure(Rc::new(ProductClosure {
        operation,
        environment,
    }))
}

pub(crate) fn product_closure_parts(value: &Value) -> Option<(usize, Environment)> {
    let Value::Closure(callable) = value else {
        return None;
    };
    callable
        .as_any()
        .downcast_ref::<ProductClosure>()
        .map(|closure| (closure.operation, closure.environment.clone()))
}

#[derive(Default)]
pub(crate) struct EvaluatedCallArguments {
    pub(crate) positional: Vec<RuntimeValue>,
    pub(crate) named: Vec<(String, RuntimeValue)>,
    pub(crate) spread: Vec<RuntimeValue>,
    pub(crate) seen_spread: bool,
}

pub(crate) struct NamedDataArguments {
    pub(crate) positional: Vec<Value>,
    pub(crate) named: Vec<(String, Value)>,
}

impl EvaluatedCallArguments {
    pub(crate) fn positional(values: Vec<RuntimeValue>) -> Self {
        Self {
            positional: values,
            ..Self::default()
        }
    }

    pub(crate) fn prepend(&mut self, value: RuntimeValue) {
        self.positional.insert(0, value);
    }

    pub(crate) fn supplied(&self) -> usize {
        self.positional.len() + self.named.len() + self.spread.len()
    }

    pub(crate) fn into_positional(
        self,
        operation: &Operation,
    ) -> Result<Vec<RuntimeValue>, String> {
        if self.named.is_empty() && !self.seen_spread {
            return Ok(self.positional);
        }
        Err(format!(
            "self target call `{}` requires argument binding metadata at dispatch",
            operation.id
        ))
    }

    pub(crate) fn into_named_data(
        self,
        operation: &Operation,
    ) -> Result<NamedDataArguments, String> {
        if self.seen_spread {
            return Err(format!(
                "spread arguments require a variadic parameter (§5) at {}",
                operation.id
            ));
        }
        let positional = self
            .positional
            .into_iter()
            .map(data)
            .collect::<Result<Vec<_>, _>>()?;
        let named = self
            .named
            .into_iter()
            .map(|(name, value)| data(value).map(|value| (name, value)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(NamedDataArguments { positional, named })
    }

    pub(crate) fn into_builtin_data(
        self,
        kind: Builtin,
        receiver_bound: bool,
        operation: &Operation,
    ) -> Result<Vec<Value>, String> {
        if self.seen_spread && kind.arity_range().1.is_some() {
            return Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_ARITY,
                "spread arguments require a variadic parameter (§5)",
                span(operation),
            )));
        }
        let mut positional = self
            .positional
            .into_iter()
            .map(data)
            .collect::<Result<Vec<_>, _>>()?;
        positional.extend(
            self.spread
                .into_iter()
                .map(data)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let named = self
            .named
            .into_iter()
            .map(|(name, value)| data(value).map(|value| (name, value)))
            .collect::<Result<Vec<_>, _>>()?;
        bind_builtin_named_args(kind, receiver_bound, positional, named, span(operation))
            .map_err(runtime_diagnostic)
    }

    pub(crate) fn into_parameter_data(
        self,
        names: &[&str],
        operation: &Operation,
    ) -> Result<Vec<Value>, String> {
        if self.seen_spread {
            return Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_ARITY,
                "spread arguments require a variadic parameter (§5)",
                span(operation),
            )));
        }
        if self.positional.len() > names.len() {
            return Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_ARITY,
                format!("expected {} argument(s), found more", names.len()),
                span(operation),
            )));
        }
        let positional = self
            .positional
            .into_iter()
            .map(data)
            .collect::<Result<Vec<_>, _>>()?;
        let named = self
            .named
            .into_iter()
            .map(|(name, value)| data(value).map(|value| (name, value)))
            .collect::<Result<Vec<_>, _>>()?;
        let slots = bind_named_arg_slots(
            positional.into_iter().map(Some).collect(),
            names.len(),
            |index| names.get(index).copied(),
            named,
            span(operation),
        )
        .map_err(runtime_diagnostic)?;
        slots
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                value.ok_or_else(|| {
                    runtime_diagnostic(topaz_value::fault(
                        topaz_value::codes::GUARD_ARITY,
                        format!(
                            "missing argument for parameter `{}` (§5)",
                            names.get(index).copied().unwrap_or("?")
                        ),
                        span(operation),
                    ))
                })
            })
            .collect()
    }

    pub(crate) fn into_runtime_parameters(
        self,
        names: &[&str],
        operation: &Operation,
    ) -> Result<Vec<RuntimeValue>, String> {
        if self.seen_spread {
            return Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_ARITY,
                "spread arguments require a variadic parameter (§5)",
                span(operation),
            )));
        }
        if self.positional.len() > names.len() {
            return Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_ARITY,
                format!("expected {} argument(s), found more", names.len()),
                span(operation),
            )));
        }
        let mut slots = self.positional.into_iter().map(Some).collect::<Vec<_>>();
        slots.resize_with(names.len(), || None);
        for (name, value) in self.named {
            let Some(index) = names.iter().position(|candidate| *candidate == name) else {
                return Err(runtime_diagnostic(topaz_value::fault(
                    topaz_value::codes::GUARD_ARITY,
                    format!("no parameter named `{name}` (§5)"),
                    span(operation),
                )));
            };
            if slots[index].is_some() {
                return Err(runtime_diagnostic(topaz_value::fault(
                    topaz_value::codes::GUARD_ARITY,
                    format!("parameter `{name}` is given twice (§5)"),
                    span(operation),
                )));
            }
            slots[index] = Some(value);
        }
        slots
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                value.ok_or_else(|| {
                    runtime_diagnostic(topaz_value::fault(
                        topaz_value::codes::GUARD_ARITY,
                        format!(
                            "missing argument for parameter `{}` (§5)",
                            names.get(index).copied().unwrap_or("?")
                        ),
                        span(operation),
                    ))
                })
            })
            .collect()
    }
}

pub(crate) struct PlannedCallLayout {
    pub(crate) callee_operand: usize,
    pub(crate) pipe_lead_operand: Option<usize>,
    pub(crate) receiver_operand: Option<usize>,
    pub(crate) argument_operands: Vec<usize>,
    pub(crate) method: String,
}

pub(crate) enum EvaluatedArgument {
    Value(RuntimeValue),
    Spread(Vec<RuntimeValue>),
}

pub(crate) fn one(arguments: &mut Vec<Value>, operation: &Operation) -> Result<Value, String> {
    if arguments.len() != 1 {
        return Err(format!(
            "{} expects one argument, found {}",
            operation.call_target,
            arguments.len()
        ));
    }
    Ok(arguments.remove(0))
}

pub(crate) fn binary_operator(operator: &str) -> Result<BinaryOp, String> {
    Ok(match operator {
        "pow" => BinaryOp::Pow,
        "mul" | "multiply" => BinaryOp::Mul,
        "div" | "divide" => BinaryOp::Div,
        "rem" | "remainder" => BinaryOp::Rem,
        "add" => BinaryOp::Add,
        "sub" | "subtract" => BinaryOp::Sub,
        "lt" | "less-than" => BinaryOp::Lt,
        "le" | "less-or-equal" => BinaryOp::Le,
        "gt" | "greater-than" => BinaryOp::Gt,
        "ge" | "greater-or-equal" => BinaryOp::Ge,
        "eq" | "equal" => BinaryOp::Eq,
        "ne" | "not-equal" => BinaryOp::Ne,
        "in" => BinaryOp::In,
        "and" => BinaryOp::And,
        "or" => BinaryOp::Or,
        "coalesce" => BinaryOp::Coalesce,
        other => return Err(format!("unsupported binary operator `{other}`")),
    })
}

pub(crate) fn assignment_value(
    operator: &str,
    left: Option<RuntimeValue>,
    right: RuntimeValue,
    operation: &Operation,
) -> Result<RuntimeValue, String> {
    if operator.is_empty() || operator == "assign" {
        return Ok(right);
    }
    let left = data(
        left.ok_or_else(|| format!("compound assignment `{operator}` omitted its left value"))?,
    )?;
    let right = data(right)?;
    Ok(RuntimeValue::Data(
        topaz_value::value::binary_value(binary_operator(operator)?, left, right, span(operation))
            .map_err(|error| format!("{error:?}"))?,
    ))
}

pub(crate) fn runtime_value_kind(value: &RuntimeValue) -> String {
    match value {
        RuntimeValue::Data(value) => value.kind().to_string(),
        RuntimeValue::Function { .. } => "function".to_string(),
        RuntimeValue::Type(name) => format!("type({name})"),
        RuntimeValue::EnumConstructor {
            identity, variant, ..
        } => format!("enum-constructor({identity}.{variant})"),
    }
}

pub(crate) fn field_name(label: &str) -> Option<String> {
    let marker = label.find("field-initializer[")?;
    let after = &label[marker..];
    let equals = after.find('=')?;
    let remainder = &after[equals + 1..];
    Some(remainder.split('/').next()?.to_string())
}

pub(crate) fn function_default_parameter_index(label: &str) -> Option<usize> {
    let mut segments = label.split('/');
    while let Some(segment) = segments.next() {
        if let Some(parameter) = segment.strip_prefix("parameters:") {
            return (segments.next()? == "default:0").then(|| parameter.parse().ok())?;
        }
    }
    None
}

pub(crate) fn function_parameter_default(
    program: &Program,
    projected_defaults: &BTreeMap<usize, usize>,
    index: usize,
    parameter: usize,
) -> Option<usize> {
    projected_defaults
        .get(&index)
        .copied()
        .or_else(|| program.operations[parameter].operands.first().copied())
}

pub(crate) fn record_pattern_field_name(label: &str) -> Option<&str> {
    let marker = "record-pattern-field[";
    let start = label.find(marker)? + marker.len();
    let index_end = label[start..].find(']')? + start;
    let name = label[index_end + 1..]
        .strip_prefix('=')?
        .split('/')
        .next()?;
    (!name.is_empty()).then_some(name)
}

pub(crate) fn list_pattern_rest(label: &str) -> bool {
    label.contains("list-pattern/rest[")
}

pub(crate) fn comprehension_clause_index(label: &str) -> Option<usize> {
    ["comprehension-clause/for[", "comprehension-clause/if["]
        .into_iter()
        .find_map(|marker| {
            let start = label.find(marker)? + marker.len();
            let end = label[start..].find(']')? + start;
            label[start..end].parse().ok()
        })
}

pub(crate) fn concurrent_arm_label(label: &str) -> Option<(usize, &str)> {
    let marker = "concurrent-arm[";
    let start = label.find(marker)? + marker.len();
    let end = label[start..].find(']')? + start;
    let index = label[start..end].parse().ok()?;
    let name = label[end + 1..].strip_prefix('=')?.split('/').next()?;
    (!name.is_empty()).then_some((index, name))
}

pub(crate) fn discovery_identifier_byte(value: i64) -> bool {
    value == 95
        || (48..=57).contains(&value)
        || (65..=90).contains(&value)
        || (97..=122).contains(&value)
        || value >= 128
}

pub(crate) fn merge_record(
    target: &mut BTreeMap<String, Value>,
    value: &Value,
) -> Result<(), String> {
    match value {
        Value::Record(fields) => {
            target.extend(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone())),
            );
            Ok(())
        }
        Value::NominalRecord { fields, .. } => {
            target.extend(
                fields
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.clone())),
            );
            Ok(())
        }
        other => Err(format!(
            "record update base has runtime type `{}`",
            other.kind()
        )),
    }
}

pub(crate) fn json_to_value(value: &JsonValue) -> Result<Value, String> {
    Ok(match value {
        // Every JSON surface consumed by the private compiler K is decoded
        // through `JSON.parseAs<T>` and its nullable fields are `Option<_>`.
        // The shared typed decoder therefore observes JSON null as `None`,
        // not as the untyped `null` value.
        JsonValue::Null => Value::None,
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::String(value) => Value::str(value),
        JsonValue::Number(value) => {
            if let Some(value) = value.int {
                Value::Int(value)
            } else {
                Value::Float(
                    value
                        .lexeme
                        .parse::<f64>()
                        .map_err(|_| "JSON float is invalid".to_string())?,
                )
            }
        }
        JsonValue::Array(values) => Value::array(
            values
                .iter()
                .map(json_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        JsonValue::Object(fields) => Value::record(
            fields
                .iter()
                .map(|(name, value)| Ok((name.to_string(), json_to_value(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
    })
}
