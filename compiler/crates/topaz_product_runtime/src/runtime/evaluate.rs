use super::{environment::*, machine::*, model::*};
use crate::diagnostic::*;
use crate::program::{model::*, validate::*};
use crate::wire::*;
use crate::*;

impl Machine {
    pub(crate) fn eval(
        &mut self,
        operation_index: usize,
        environment: Environment,
    ) -> Result<Flow, String> {
        run_local(self.eval_async(operation_index, environment))
    }

    pub(crate) async fn cooperative_tick(&mut self) {
        let Some(remaining) = self.cooperative_remaining.as_mut() else {
            return;
        };
        if *remaining == 0 {
            *remaining = CONCURRENT_STEP_QUANTUM;
            YieldOnce(false).await;
        }
        *remaining -= 1;
    }

    pub(crate) fn eval_async(
        &mut self,
        operation_index: usize,
        environment: Environment,
    ) -> LocalFuture<'_, Result<Flow, String>> {
        Box::pin(self.eval_body(operation_index, environment))
    }

    pub(crate) async fn eval_body(
        &mut self,
        operation_index: usize,
        environment: Environment,
    ) -> Result<Flow, String> {
        self.cooperative_tick().await;
        let steps = self.steps.get().saturating_add(1);
        self.steps.set(steps);
        let operation = self.program.operations[operation_index].clone();
        if std::env::var_os("TOPAZ_STAGE1_TRACE").is_some() && steps.is_multiple_of(5_000_000) {
            eprintln!(
                "stage1-runtime-trace: steps={} at {}:{}-{} {}",
                steps, operation.module, operation.lo, operation.hi, operation.kind
            );
        }
        if steps > STAGE1_EXECUTION_STEP_LIMIT {
            return Err(format!(
                "Stage 1 execution-step limit exceeded at {}:{}-{} {}",
                operation.module, operation.lo, operation.hi, operation.kind
            ));
        }
        let result = self
            .eval_operation(operation_index, &operation, environment)
            .await;
        result.map_err(|error| {
            if is_control_signal(&error) {
                error
            } else {
                format!(
                    "{error}\n  at {}:{}-{} {} ({})",
                    operation.module, operation.lo, operation.hi, operation.kind, operation.id
                )
            }
        })
    }

    pub(crate) fn eval_operation<'a>(
        &'a mut self,
        operation_index: usize,
        operation: &'a Operation,
        environment: Environment,
    ) -> LocalFuture<'a, Result<Flow, String>> {
        match operation.kind.as_str() {
            "import"
            | "record"
            | "enum"
            | "newtype"
            | "type-alias"
            | "function"
            | "implementation"
            | "protocol"
            | "binding/capture"
            | "expression/integer"
            | "expression/float"
            | "expression/boolean"
            | "expression/null"
            | "expression/unit"
            | "expression/duration"
            | "expression/string-text"
            | "expression/identifier"
            | "expression/lambda"
            | "expression/placeholder"
            | "continue" => {
                let result = self.eval_simple_operation(operation_index, operation, environment);
                Box::pin(async move { result })
            }
            "module" | "export" | "expression/parenthesized" | "expression/block" => {
                Box::pin(self.eval_sequence(&operation.operands, environment))
            }
            "using" => Box::pin(self.eval_using(operation, environment)),
            "expression/if" => Box::pin(self.eval_if(operation, environment)),
            "expression/match" => Box::pin(self.eval_match(operation, environment)),
            "expression/for" => Box::pin(self.eval_for(operation, environment)),
            "expression/loop" => Box::pin(self.eval_loop(operation, environment)),
            "while" => Box::pin(self.eval_while(operation, environment)),
            "expression/result-propagation" | "return" | "break" => {
                Box::pin(self.eval_return_control(operation, environment))
            }
            "expression/call" | "expression/pipeline" => self.eval_call(operation, environment),
            "expression/binary" => Box::pin(self.eval_binary(operation, environment)),
            "expression/range" => Box::pin(self.eval_range(operation, environment)),
            "expression/compose"
            | "expression/member"
            | "expression/optional-member"
            | "expression/index"
            | "expression/unary" => Box::pin(self.eval_call_access(operation, environment)),
            "constant"
            | "expression/string"
            | "expression/array"
            | "expression/set"
            | "expression/map"
            | "expression/comprehension"
            | "expression/concurrent"
            | "expression/record-literal"
            | "expression/record-update"
            | "let"
            | "assignment" => Box::pin(self.eval_collection_binding(operation, environment)),
            other => {
                let error = format!(
                    "Stage 1 runtime does not yet execute `{other}` at {}:{}-{}",
                    operation.module, operation.lo, operation.hi
                );
                Box::pin(async move { Err(error) })
            }
        }
    }

    pub(crate) fn eval_simple_operation(
        &self,
        operation_index: usize,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        match operation.kind.as_str() {
            "import" | "record" | "enum" | "newtype" | "type-alias" | "function"
            | "implementation" | "protocol" | "binding/capture" => Ok(Flow::Value(unit())),
            "expression/integer" => operation
                .detail
                .parse::<i64>()
                .map(|value| Flow::Value(RuntimeValue::Data(Value::Int(value))))
                .map_err(|_| format!("invalid Stage 1 integer `{}`", operation.detail)),
            "expression/float" => operation
                .detail
                .parse::<f64>()
                .map(|value| Flow::Value(RuntimeValue::Data(Value::Float(value))))
                .map_err(|_| format!("invalid Stage 1 float `{}`", operation.detail)),
            "expression/boolean" => Ok(Flow::Value(RuntimeValue::Data(Value::Bool(
                operation.detail == "true",
            )))),
            "expression/null" => Ok(Flow::Value(RuntimeValue::Data(Value::Null))),
            "expression/unit" => Ok(Flow::Value(unit())),
            "expression/duration" => Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_TYPE,
                "duration literals exist only in the `concurrent` timeout clause (§15)",
                span(operation),
            ))),
            "expression/string-text" => {
                let mut decoded = String::new();
                topaz_value::value::decode_escapes(
                    &operation.detail,
                    &mut decoded,
                    span(operation),
                )
                .map_err(|error| format!("{error:?}"))?;
                Ok(Flow::Value(RuntimeValue::Data(Value::str(decoded))))
            }
            "expression/identifier" => self.eval_identifier(operation, environment),
            "expression/lambda" => Ok(Flow::Value(RuntimeValue::Function {
                operation: operation_index,
                environment,
            })),
            "expression/placeholder" => environment
                .slot("_")
                .map(|slot| Flow::Value(slot.borrow().clone()))
                .ok_or_else(|| {
                    runtime_diagnostic(topaz_value::fault(
                        topaz_value::codes::GUARD_TYPE,
                        "`_` is only valid inside a pipeline stage (§11)",
                        span(operation),
                    ))
                }),
            "continue" => Ok(Flow::Continue {
                target: operation.control_target.clone(),
            }),
            other => Err(format!(
                "Stage 1 runtime simple operation routing drifted to `{other}`"
            )),
        }
    }

    pub(crate) async fn eval_return_control(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        match operation.kind.as_str() {
            "expression/result-propagation" => {
                let value = data(self.eval_value(operation.operands[0], environment).await?)?;
                match value {
                    Value::Ok(value) => Ok(Flow::Value(RuntimeValue::Data((*value).clone()))),
                    Value::Err(_) => {
                        self.propagating = Some(RuntimeValue::Data(value));
                        Err(PROPAGATE_SIGNAL.to_string())
                    }
                    other => Err(format!(
                        "result propagation requires Result, found `{}`",
                        other.kind()
                    )),
                }
            }
            "return" => {
                let value = match operation.operands.first() {
                    Some(operand) => self.eval_value(*operand, environment).await?,
                    None => unit(),
                };
                Ok(Flow::Return(value))
            }
            "break" => {
                let value = match operation.operands.first() {
                    Some(operand) => self.eval_value(*operand, environment).await?,
                    None => unit(),
                };
                Ok(Flow::Break {
                    target: operation.control_target.clone(),
                    value,
                })
            }
            other => Err(format!(
                "Stage 1 runtime return/control routing drifted to `{other}`"
            )),
        }
    }

    pub(crate) async fn eval_binary(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let left = data(
            self.eval_value(operation.operands[0], environment.clone())
                .await?,
        )?;
        if operation.detail == "and" || operation.detail == "or" {
            let Value::Bool(left) = left else {
                return Err(format!("{} requires a bool left operand", operation.detail));
            };
            if (operation.detail == "and" && !left) || (operation.detail == "or" && left) {
                return Ok(Flow::Value(RuntimeValue::Data(Value::Bool(left))));
            }
            return self.eval_async(operation.operands[1], environment).await;
        }
        if operation.detail == "coalesce" {
            let result =
                topaz_value::value::short_circuit_lhs(left, BinaryOp::Coalesce, span(operation))
                    .map_err(runtime_diagnostic)?;
            return match result {
                Some(value) => Ok(Flow::Value(RuntimeValue::Data(value))),
                None => self.eval_async(operation.operands[1], environment).await,
            };
        }
        let right = data(self.eval_value(operation.operands[1], environment).await?)?;
        let operator = binary_operator(&operation.detail)?;
        let result = topaz_value::value::binary_value(operator, left, right, span(operation))
            .map_err(runtime_diagnostic)?;
        Ok(Flow::Value(RuntimeValue::Data(result)))
    }

    pub(crate) async fn eval_call_access(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        match operation.kind.as_str() {
            "expression/compose" => {
                let left = data(
                    self.eval_value(operation.operands[0], environment.clone())
                        .await?,
                )?;
                let right = data(self.eval_value(operation.operands[1], environment).await?)?;
                Ok(Flow::Value(RuntimeValue::Data(Value::Composed(Rc::new((
                    left, right,
                ))))))
            }
            "expression/member" => {
                let receiver = self
                    .eval_value(
                        *operation
                            .operands
                            .first()
                            .ok_or_else(|| format!("{} has no receiver", operation.id))?,
                        environment,
                    )
                    .await?;
                if let RuntimeValue::Type(name) = receiver {
                    return self.eval_nominal_member(operation, &name);
                }
                let receiver = data(receiver)?;
                let value = topaz_value::value::member_value_required(
                    &receiver,
                    &operation.detail,
                    span(operation),
                )
                .map_err(|error| format!("{error:?}"))?;
                Ok(Flow::Value(RuntimeValue::Data(value)))
            }
            "expression/optional-member" => {
                let receiver = data(
                    self.eval_value(
                        *operation
                            .operands
                            .first()
                            .ok_or_else(|| format!("{} has no receiver", operation.id))?,
                        environment,
                    )
                    .await?,
                )?;
                let value = topaz_value::value::optional_member(
                    receiver,
                    &operation.detail,
                    span(operation),
                )
                .map_err(|error| format!("{error:?}"))?;
                Ok(Flow::Value(RuntimeValue::Data(value)))
            }
            "expression/index" => {
                let object = data(
                    self.eval_value(operation.operands[0], environment.clone())
                        .await?,
                )?;
                let index = data(self.eval_value(operation.operands[1], environment).await?)?;
                let result = topaz_value::value::index_value(object, index, span(operation))
                    .map_err(runtime_diagnostic)?;
                Ok(Flow::Value(RuntimeValue::Data(result)))
            }
            "expression/unary" => {
                let value = data(self.eval_value(operation.operands[0], environment).await?)?;
                let operator = match operation.detail.as_str() {
                    "pos" | "plus" => UnaryOp::Plus,
                    "neg" | "minus" => UnaryOp::Minus,
                    "not" => UnaryOp::Not,
                    other => return Err(format!("unsupported unary operator `{other}`")),
                };
                let result = topaz_value::value::unary_value(operator, value, span(operation))
                    .map_err(runtime_diagnostic)?;
                Ok(Flow::Value(RuntimeValue::Data(result)))
            }
            other => Err(format!(
                "Stage 1 runtime call/access routing drifted to `{other}`"
            )),
        }
    }

    pub(crate) async fn eval_collection_binding(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        match operation.kind.as_str() {
            "constant" => {
                let value = self
                    .eval_value(
                        *operation
                            .operands
                            .first()
                            .ok_or_else(|| format!("{} has no value", operation.id))?,
                        environment.clone(),
                    )
                    .await?;
                let key = format!("{}::{}", operation.module, operation.binding_name);
                self.globals.define(key, value.clone());
                if !operation.declaration_identity.is_empty() {
                    self.globals
                        .define(operation.declaration_identity.clone(), value.clone());
                }
                Ok(Flow::Value(value))
            }
            "expression/string" => self.eval_string(operation, environment).await,
            "expression/array" => self.eval_array(operation, environment).await,
            "expression/set" => {
                let mut values = Vec::with_capacity(operation.operands.len());
                for operand in &operation.operands {
                    values.push(data(self.eval_value(*operand, environment.clone()).await?)?);
                }
                let value = builtin_set_of(values, span(operation)).map_err(runtime_diagnostic)?;
                Ok(Flow::Value(RuntimeValue::Data(value)))
            }
            "expression/map" => self.eval_map(operation, environment).await,
            "expression/comprehension" => self.eval_comprehension(operation, environment).await,
            "expression/concurrent" => self.eval_concurrent(operation, environment).await,
            "expression/record-literal" | "expression/record-update" => {
                self.eval_record(operation, environment).await
            }
            "let" => {
                let value = self
                    .eval_value(operation.operands[0], environment.clone())
                    .await?;
                self.bind(operation.operands[1], value, environment)?;
                Ok(Flow::Value(unit()))
            }
            "assignment" => self.eval_assignment(operation, environment).await,
            other => Err(format!(
                "Stage 1 runtime collection/binding routing drifted to `{other}`"
            )),
        }
    }

    pub(crate) async fn eval_sequence(
        &mut self,
        operations: &[usize],
        environment: Environment,
    ) -> Result<Flow, String> {
        let mut last = unit();
        let mut deferred = Vec::new();
        let mut outcome = None;
        for operation in operations {
            if self.program.operations[*operation].kind == "defer" {
                let action = *self.program.operations[*operation]
                    .operands
                    .first()
                    .ok_or_else(|| {
                        format!(
                            "{} has no deferred action",
                            self.program.operations[*operation].id
                        )
                    })?;
                deferred.push(action);
                continue;
            }
            match self.eval_async(*operation, environment.clone()).await {
                Ok(Flow::Value(value)) => last = value,
                Ok(flow) => {
                    outcome = Some(Ok(flow));
                    break;
                }
                Err(error) if is_control_signal(&error) => {
                    outcome = Some(Err(error));
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        for action in deferred.into_iter().rev() {
            let deferred_result = self.eval_async(action, environment.clone()).await;
            let error = match deferred_result {
                Ok(Flow::Value(_)) => None,
                Ok(Flow::Return(_)) => Some("deferred action attempted to return".to_string()),
                Ok(Flow::Break { .. }) => Some("deferred action attempted to break".to_string()),
                Ok(Flow::Continue { .. }) => {
                    Some("deferred action attempted to continue".to_string())
                }
                Err(error) => Some(error),
            };
            if let Some(error) = error
                && let Some(host) = &self.host
            {
                host.defer_error(&error);
            }
        }
        outcome.unwrap_or(Ok(Flow::Value(last)))
    }

    pub(crate) async fn eval_using(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let value = data(
            self.eval_value(operation.operands[0], environment.clone())
                .await?,
        )?;
        let Value::Resource(handle) = value else {
            return Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_TYPE,
                format!("`using` expects a `File`, found `{}`", value.kind()),
                span(operation),
            )));
        };
        let host = self
            .host
            .clone()
            .ok_or_else(|| "`using` requires an admitted product host".to_string())?;
        let body_environment = EnvironmentFrame::child(environment);
        self.bind_declared_value(
            operation,
            RuntimeValue::Data(Value::Resource(handle)),
            body_environment.clone(),
        );
        let outcome = self
            .eval_async(operation.operands[1], body_environment)
            .await;
        match outcome {
            Ok(Flow::Value(_)) => {
                host.close(handle);
                Ok(Flow::Value(unit()))
            }
            Ok(flow) => {
                host.close(handle);
                Ok(flow)
            }
            Err(error) if is_control_signal(&error) => {
                host.close(handle);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn eval_value(
        &mut self,
        operation: usize,
        environment: Environment,
    ) -> Result<RuntimeValue, String> {
        match self.eval_async(operation, environment).await? {
            Flow::Value(value) => Ok(value),
            Flow::Return(value) => {
                self.returning = Some(value);
                Err(RETURN_SIGNAL.to_string())
            }
            flow @ Flow::Break { .. } => {
                self.loop_control = Some(flow);
                Err(BREAK_SIGNAL.to_string())
            }
            flow @ Flow::Continue { .. } => {
                self.loop_control = Some(flow);
                Err(CONTINUE_SIGNAL.to_string())
            }
        }
    }

    pub(crate) fn eval_identifier(
        &self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        if operation.reference_identity == "builtin::None" || operation.detail == "None" {
            return Ok(Flow::Value(RuntimeValue::Data(Value::None)));
        }
        let keys = [
            operation.reference_identity.as_str(),
            operation.detail.as_str(),
        ];
        for key in keys {
            if !key.is_empty()
                && let Some(slot) = environment.slot(key)
            {
                return Ok(Flow::Value(slot.borrow().clone()));
            }
        }
        for key in keys {
            if !key.is_empty() {
                if let Some(slot) = self.globals.slot(key) {
                    return Ok(Flow::Value(slot.borrow().clone()));
                }
                if let Some(function) = self.functions.get(key) {
                    return Ok(Flow::Value(RuntimeValue::Function {
                        operation: *function,
                        environment: self.globals.clone(),
                    }));
                }
            }
        }
        if let Some(kind) = direct_builtin(&operation.reference_identity)
            .or_else(|| Builtin::free(&operation.detail))
        {
            return Ok(Flow::Value(RuntimeValue::Data(Value::Builtin {
                kind,
                recv: None,
            })));
        }
        let name = if !operation.reference_identity.is_empty()
            && self.nominals.get(&operation.reference_identity).is_some()
        {
            operation.reference_identity.clone()
        } else {
            operation.detail.clone()
        };
        Ok(Flow::Value(RuntimeValue::Type(name)))
    }

    pub(crate) async fn eval_string(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let mut result = String::new();
        for operand in &operation.operands {
            let value = data(self.eval_value(*operand, environment.clone()).await?)?;
            match value {
                Value::Str(value) => result.push_str(&value),
                other => result.push_str(&topaz_value::value::render(&other)),
            }
        }
        Ok(Flow::Value(RuntimeValue::Data(Value::str(result))))
    }

    pub(crate) async fn eval_call_arguments(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<EvaluatedCallArguments, String> {
        let operand_start = usize::from(!operation.operands.is_empty());
        let operands = &operation.operands[operand_start..];
        if operation.call_arguments.is_empty() {
            let mut positional = Vec::with_capacity(operands.len());
            for operand in operands {
                positional.push(self.eval_value(*operand, environment.clone()).await?);
            }
            return Ok(EvaluatedCallArguments::positional(positional));
        }
        if operation.call_arguments.len() != operands.len() {
            return Err(format!(
                "self target call `{}` has {} argument plans for {} operands",
                operation.id,
                operation.call_arguments.len(),
                operands.len()
            ));
        }
        let mut arguments = EvaluatedCallArguments::default();
        for (argument, operand) in operation.call_arguments.iter().zip(operands) {
            let value = self.eval_value(*operand, environment.clone()).await?;
            match &argument.binding {
                CallArgumentBinding::Positional => {
                    if arguments.seen_spread {
                        arguments.spread.push(value);
                    } else {
                        arguments.positional.push(value);
                    }
                }
                CallArgumentBinding::Named(name) => arguments.named.push((name.clone(), value)),
                CallArgumentBinding::Spread => {
                    let mut spread = Vec::new();
                    call_spread_extend(
                        &mut spread,
                        data(value)?,
                        Span::new(FileId(0), argument.lo, argument.hi),
                    )
                    .map_err(runtime_diagnostic)?;
                    arguments
                        .spread
                        .extend(spread.into_iter().map(RuntimeValue::Data));
                    arguments.seen_spread = true;
                }
                CallArgumentBinding::InsertedLead => {
                    return Err(format!(
                        "self target direct call `{}` contains a pipeline lead",
                        operation.id
                    ));
                }
            }
        }
        Ok(arguments)
    }

    pub(crate) async fn eval_array(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let mut values = Vec::with_capacity(operation.operands.len());
        for (operand, label) in operation.operands.iter().zip(&operation.operand_labels) {
            let value = data(self.eval_value(*operand, environment.clone()).await?)?;
            if label.contains("array-element/spread[") {
                array_spread_extend(&mut values, value, span(operation))
                    .map_err(runtime_diagnostic)?;
            } else {
                values.push(value);
            }
        }
        Ok(Flow::Value(RuntimeValue::Data(Value::array(values))))
    }

    pub(crate) async fn eval_range(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let lo = data(
            self.eval_value(operation.operands[0], environment.clone())
                .await?,
        )?;
        let hi = data(
            self.eval_value(operation.operands[1], environment.clone())
                .await?,
        )?;
        let step = match operation.operands.get(2) {
            Some(operand) => Some(data(self.eval_value(*operand, environment).await?)?),
            None => None,
        };
        let inclusive = match operation.detail.as_str() {
            "true" => true,
            "false" => false,
            other => {
                return Err(format!(
                    "{} has invalid range inclusive flag `{other}`",
                    operation.id
                ));
            }
        };
        let value =
            make_range(lo, hi, inclusive, step, span(operation)).map_err(runtime_diagnostic)?;
        Ok(Flow::Value(RuntimeValue::Data(value)))
    }

    pub(crate) async fn eval_comprehension(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let kind = match operation.detail.as_str() {
            "array" => ComprehensionKind::Array,
            "set" => ComprehensionKind::Set,
            "map" => ComprehensionKind::Map,
            other => {
                return Err(format!(
                    "{} has unsupported comprehension collection `{other}`",
                    operation.id
                ));
            }
        };
        let mut clause_parts = BTreeMap::<usize, ComprehensionClauseParts>::new();
        let mut body = None;
        let mut body_key = None;
        let mut body_value = None;
        for (operand, label) in operation.operands.iter().zip(&operation.operand_labels) {
            if let Some(index) = comprehension_clause_index(label) {
                let parts = clause_parts.entry(index).or_default();
                if label.contains("/iterator:") {
                    parts.iterator = Some(*operand);
                } else if label.contains("/pattern:") {
                    parts.pattern = Some(*operand);
                } else if label.contains("/condition:") {
                    parts.condition = Some(*operand);
                } else {
                    return Err(format!(
                        "{} has unrecognized comprehension clause operand `{label}`",
                        operation.id
                    ));
                }
            } else if label.contains("bodyKey:") {
                body_key = Some(*operand);
            } else if label.contains("bodyValue:") {
                body_value = Some(*operand);
            } else if label.contains("body:") {
                body = Some(*operand);
            } else {
                return Err(format!(
                    "{} has unrecognized comprehension operand `{label}`",
                    operation.id
                ));
            }
        }
        let clauses = clause_parts
            .into_iter()
            .map(
                |(index, parts)| match (parts.iterator, parts.pattern, parts.condition) {
                    (Some(iterator), Some(pattern), None) => {
                        Ok(ComprehensionClause::For { iterator, pattern })
                    }
                    (None, None, Some(condition)) => Ok(ComprehensionClause::If { condition }),
                    _ => Err(format!(
                        "{} has incomplete comprehension clause {index}",
                        operation.id
                    )),
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        let body = match (body, body_key, body_value) {
            (Some(body), None, None) => ComprehensionBody::Element(body),
            (None, Some(key), Some(value)) => ComprehensionBody::Entry { key, value },
            _ => {
                return Err(format!(
                    "{} has an incomplete comprehension body",
                    operation.id
                ));
            }
        };
        let mut output = match (&kind, &body) {
            (ComprehensionKind::Array | ComprehensionKind::Set, ComprehensionBody::Element(_)) => {
                ComprehensionOutput::Elements(Vec::new())
            }
            (ComprehensionKind::Map, ComprehensionBody::Entry { .. }) => {
                ComprehensionOutput::Entries(Vec::new())
            }
            _ => {
                return Err(format!(
                    "{} comprehension body does not match its collection kind",
                    operation.id
                ));
            }
        };
        self.eval_comprehension_clause(operation, &clauses, 0, &body, environment, &mut output)
            .await?;
        let value = match (kind, output) {
            (ComprehensionKind::Array, ComprehensionOutput::Elements(elements)) => {
                Value::array(elements)
            }
            (ComprehensionKind::Set, ComprehensionOutput::Elements(elements)) => {
                builtin_set_of(elements, span(operation)).map_err(runtime_diagnostic)?
            }
            (ComprehensionKind::Map, ComprehensionOutput::Entries(entries)) => {
                builtin_map_of(entries, span(operation)).map_err(runtime_diagnostic)?
            }
            _ => {
                return Err(format!(
                    "{} comprehension body does not match its collection kind",
                    operation.id
                ));
            }
        };
        Ok(Flow::Value(RuntimeValue::Data(value)))
    }

    pub(crate) fn eval_comprehension_clause<'a>(
        &'a mut self,
        operation: &'a Operation,
        clauses: &'a [ComprehensionClause],
        index: usize,
        body: &'a ComprehensionBody,
        environment: Environment,
        output: &'a mut ComprehensionOutput,
    ) -> LocalFuture<'a, Result<(), String>> {
        Box::pin(self.eval_comprehension_clause_body(
            operation,
            clauses,
            index,
            body,
            environment,
            output,
        ))
    }

    pub(crate) async fn eval_comprehension_clause_body(
        &mut self,
        operation: &Operation,
        clauses: &[ComprehensionClause],
        index: usize,
        body: &ComprehensionBody,
        environment: Environment,
        output: &mut ComprehensionOutput,
    ) -> Result<(), String> {
        let Some(clause) = clauses.get(index) else {
            match (body, output) {
                (ComprehensionBody::Element(body), ComprehensionOutput::Elements(elements)) => {
                    elements.push(data(self.eval_value(*body, environment).await?)?)
                }
                (
                    ComprehensionBody::Entry { key, value },
                    ComprehensionOutput::Entries(entries),
                ) => {
                    let key = data(self.eval_value(*key, environment.clone()).await?)?;
                    let value = data(self.eval_value(*value, environment).await?)?;
                    entries.push((key, value));
                }
                _ => {
                    return Err(format!(
                        "{} comprehension output disagrees with its body",
                        operation.id
                    ));
                }
            }
            return Ok(());
        };
        match clause {
            ComprehensionClause::For { iterator, pattern } => {
                let iterator = data(self.eval_value(*iterator, environment.clone()).await?)?;
                let items = for_items(&iterator, span(operation)).map_err(runtime_diagnostic)?;
                for item in items {
                    let item_environment = EnvironmentFrame::child(environment.clone());
                    if !self
                        .match_pattern(*pattern, RuntimeValue::Data(item), item_environment.clone())
                        .await?
                    {
                        return Err(format!(
                            "{} comprehension pattern did not match an element",
                            operation.id
                        ));
                    }
                    self.eval_comprehension_clause(
                        operation,
                        clauses,
                        index + 1,
                        body,
                        item_environment,
                        output,
                    )
                    .await?;
                }
            }
            ComprehensionClause::If { condition } => {
                let condition = data(self.eval_value(*condition, environment.clone()).await?)?;
                if condition_bool(&condition, "if", span(operation)).map_err(runtime_diagnostic)? {
                    self.eval_comprehension_clause(
                        operation,
                        clauses,
                        index + 1,
                        body,
                        environment,
                        output,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn eval_map(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let mut map = topaz_value::value::OrderedMap::default();
        let mut index = 0;
        while index < operation.operands.len() {
            let key = data(
                self.eval_value(operation.operands[index], environment.clone())
                    .await?,
            )?;
            let value = data(
                self.eval_value(
                    *operation
                        .operands
                        .get(index + 1)
                        .ok_or_else(|| format!("{} has an unpaired map key", operation.id))?,
                    environment.clone(),
                )
                .await?,
            )?;
            map.insert_value(&key, value)
                .map_err(|error| format!("{error:?}"))?;
            index += 2;
        }
        Ok(Flow::Value(RuntimeValue::Data(Value::Map(Rc::new(
            RefCell::new(map),
        )))))
    }

    pub(crate) async fn eval_record(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let mut fields = BTreeMap::new();
        let mut nominal = None::<(
            Rc<str>,
            Option<Rc<str>>,
            Option<Rc<str>>,
            Vec<(Rc<str>, Option<String>)>,
        )>;
        for (operand, label) in operation.operands.iter().zip(&operation.operand_labels) {
            let value = self.eval_value(*operand, environment.clone()).await?;
            if label.starts_with("base:") {
                match value {
                    RuntimeValue::Data(value) => {
                        if let Value::NominalRecord {
                            record_id,
                            declaration_identity,
                            method_identity,
                            fields: nominal_fields,
                        } = &value
                        {
                            nominal = Some((
                                record_id.clone(),
                                declaration_identity.clone(),
                                method_identity.clone(),
                                nominal_fields
                                    .iter()
                                    .map(|(name, _)| (name.clone(), None))
                                    .collect(),
                            ));
                        }
                        merge_record(&mut fields, &value)?;
                    }
                    RuntimeValue::Type(name) => {
                        let Some(fact) = self.nominals.get(&name) else {
                            continue;
                        };
                        if fact.kind != "record" {
                            return Err(format!(
                                "self target record literal `{}` uses non-record `{}`",
                                operation.id, fact.identity
                            ));
                        }
                        nominal = Some((
                            Rc::from(fact.identity.as_str()),
                            None,
                            None,
                            fact.members
                                .iter()
                                .map(|member| {
                                    (
                                        Rc::from(member.name.as_str()),
                                        member.default_operation_id.clone(),
                                    )
                                })
                                .collect(),
                        ));
                    }
                    RuntimeValue::Function { .. } => {
                        return Err(format!(
                            "self target record literal `{}` has a function base",
                            operation.id
                        ));
                    }
                    RuntimeValue::EnumConstructor {
                        identity, variant, ..
                    } => {
                        return Err(format!(
                            "self target record literal `{}` has enum constructor base `{identity}.{variant}`",
                            operation.id
                        ));
                    }
                }
                continue;
            }
            if label.starts_with("spread:") {
                merge_record(&mut fields, &data(value)?)?;
                continue;
            }
            let Some(name) = field_name(label) else {
                return Err(format!(
                    "{} has an unrecognized record field `{label}`",
                    operation.id
                ));
            };
            fields.insert(name, data(value)?);
        }
        if let Some((record_id, declaration_identity, method_identity, order)) = nominal {
            let mut ordered = Vec::with_capacity(fields.len());
            for (name, default_operation_id) in order {
                let value = match fields.remove(name.as_ref()) {
                    Some(value) => value,
                    None => {
                        let default_operation_id = default_operation_id.ok_or_else(|| {
                            format!("self target record `{record_id}` is missing field `{name}`")
                        })?;
                        let default_operation = self
                            .program
                            .operations
                            .iter()
                            .position(|operation| operation.id == default_operation_id)
                            .ok_or_else(|| {
                                format!(
                                    "self target record `{record_id}` default `{default_operation_id}` is absent"
                                )
                            })?;
                        data(
                            self.eval_value(default_operation, environment.clone())
                                .await?,
                        )?
                    }
                };
                ordered.push((name, value));
            }
            ordered.extend(
                fields
                    .into_iter()
                    .map(|(name, value)| (Rc::from(name), value)),
            );
            let value = match declaration_identity {
                Some(identity) => Value::nominal_record_with_identities(
                    record_id,
                    identity,
                    method_identity,
                    ordered,
                ),
                None => {
                    Value::nominal_record_with_method_identity(record_id, method_identity, ordered)
                }
            };
            return Ok(Flow::Value(RuntimeValue::Data(value)));
        }
        if let Some(fact) = self.nominals.operation(&operation.id).cloned() {
            if fact.kind != "record" {
                return Err(format!(
                    "self target operation `{}` is a record literal but its nominal fact is `{}`",
                    operation.id, fact.kind
                ));
            }
            let identity = fact.identity.clone();
            let members = fact.members.clone();
            let mut ordered = Vec::with_capacity(members.len());
            for member in &members {
                let value = match fields.remove(&member.name) {
                    Some(value) => value,
                    None => {
                        let default_operation_id =
                            member.default_operation_id.as_deref().ok_or_else(|| {
                                format!(
                                    "self target record literal `{identity}` omitted field `{}`",
                                    member.name
                                )
                            })?;
                        let default_operation = self
                            .program
                            .operations
                            .iter()
                            .position(|operation| operation.id == default_operation_id)
                            .ok_or_else(|| {
                                format!(
                                    "self target record literal `{identity}` default `{default_operation_id}` is absent"
                                )
                            })?;
                        data(
                            self.eval_value(default_operation, environment.clone())
                                .await?,
                        )?
                    }
                };
                ordered.push((Rc::from(member.name.as_str()), value));
            }
            if !fields.is_empty() {
                return Err(format!(
                    "self target record literal `{}` has unprojected field(s): {}",
                    fact.identity,
                    fields.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            return Ok(Flow::Value(RuntimeValue::Data(Value::nominal_record(
                &identity, ordered,
            ))));
        }
        Ok(Flow::Value(RuntimeValue::Data(Value::record(fields))))
    }
}
