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
        if let Some(result) = self.eval_leaf(operation_index, &environment) {
            return Box::pin(async move { result });
        }
        Box::pin(self.eval_body(operation_index, environment))
    }

    /// Accounts one step and reports the step ceiling shared by every route.
    fn account_step(&self, operation: &Operation) -> Option<String> {
        let steps = self.steps.get().saturating_add(1);
        self.steps.set(steps);
        if steps.is_multiple_of(5_000_000) && std::env::var_os("TOPAZ_STAGE1_TRACE").is_some() {
            eprintln!(
                "stage1-runtime-trace: steps={} at {}:{}-{} {}",
                steps, operation.module, operation.lo, operation.hi, operation.kind
            );
        }
        (steps > STAGE1_EXECUTION_STEP_LIMIT).then(|| {
            format!(
                "Stage 1 execution-step limit exceeded at {}:{}-{} {}",
                operation.module, operation.lo, operation.hi, operation.kind
            )
        })
    }

    fn operation_failure(operation: &Operation, error: String) -> String {
        if is_control_signal(&error) {
            error
        } else {
            format!(
                "{error}\n  at {}:{}-{} {} ({})",
                operation.module, operation.lo, operation.hi, operation.kind, operation.id
            )
        }
    }

    /// Evaluates an operation that needs neither operand evaluation nor
    /// suspension, so the step loop reaches its value without allocating a
    /// future for it.
    pub(crate) fn eval_leaf(
        &self,
        operation_index: usize,
        environment: &Environment,
    ) -> Option<Result<Flow, String>> {
        if self.cooperative_remaining.is_some() {
            return None;
        }
        let kind = *self.program.kinds.get(operation_index)?;
        if !kind.is_leaf() {
            return None;
        }
        let operation = &self.program.operations[operation_index];
        if let Some(error) = self.account_step(operation) {
            return Some(Err(error));
        }
        Some(
            self.eval_simple_operation(kind, operation_index, operation, environment.clone())
                .map_err(|error| Self::operation_failure(operation, error)),
        )
    }

    pub(crate) async fn eval_body(
        &mut self,
        operation_index: usize,
        environment: Environment,
    ) -> Result<Flow, String> {
        self.cooperative_tick().await;
        let program = self.program.clone();
        let operation = &program.operations[operation_index];
        let kind = program.kinds[operation_index];
        if let Some(error) = self.account_step(operation) {
            return Err(error);
        }
        let result = self
            .eval_operation(kind, operation_index, operation, environment)
            .await;
        result.map_err(|error| Self::operation_failure(operation, error))
    }

    pub(crate) fn eval_operation<'a>(
        &'a mut self,
        kind: OpKind,
        operation_index: usize,
        operation: &'a Operation,
        environment: Environment,
    ) -> LocalFuture<'a, Result<Flow, String>> {
        match kind {
            kind if kind.is_leaf() => {
                let result =
                    self.eval_simple_operation(kind, operation_index, operation, environment);
                Box::pin(async move { result })
            }
            OpKind::Module | OpKind::Export | OpKind::Parenthesized => {
                Box::pin(self.eval_sequence(&operation.operands, environment))
            }
            OpKind::Block => Box::pin(
                self.eval_sequence(&operation.operands, EnvironmentFrame::child(environment)),
            ),
            OpKind::Using => Box::pin(self.eval_using(operation, environment)),
            OpKind::If => Box::pin(self.eval_if(operation, environment)),
            OpKind::Match => Box::pin(self.eval_match(operation_index, operation, environment)),
            OpKind::For => Box::pin(self.eval_for(operation, environment)),
            OpKind::Loop => Box::pin(self.eval_loop(operation, environment)),
            OpKind::While => Box::pin(self.eval_while(operation, environment)),
            OpKind::ResultPropagation | OpKind::Return | OpKind::Break => {
                Box::pin(self.eval_return_control(operation, environment))
            }
            OpKind::Call | OpKind::Pipeline => {
                self.eval_call(operation_index, operation, environment)
            }
            OpKind::Binary => Box::pin(self.eval_binary(operation, environment)),
            OpKind::Range => Box::pin(self.eval_range(operation, environment)),
            OpKind::Compose
            | OpKind::Member
            | OpKind::OptionalMember
            | OpKind::Index
            | OpKind::Unary => Box::pin(self.eval_call_access(operation, environment)),
            OpKind::Constant
            | OpKind::StringInterpolation
            | OpKind::Array
            | OpKind::Set
            | OpKind::Map
            | OpKind::Comprehension
            | OpKind::Concurrent
            | OpKind::RecordLiteral
            | OpKind::RecordUpdate
            | OpKind::Let
            | OpKind::Assignment => {
                Box::pin(self.eval_collection_binding(operation_index, operation, environment))
            }
            _ => {
                let error = format!(
                    "Stage 1 runtime does not yet execute `{}` at {}:{}-{}",
                    operation.kind, operation.module, operation.lo, operation.hi
                );
                Box::pin(async move { Err(error) })
            }
        }
    }

    pub(crate) fn eval_simple_operation(
        &self,
        kind: OpKind,
        operation_index: usize,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        match kind {
            OpKind::Import
            | OpKind::Record
            | OpKind::Enum
            | OpKind::Newtype
            | OpKind::TypeAlias
            | OpKind::Implementation
            | OpKind::Protocol
            | OpKind::BindingCapture => Ok(Flow::Value(unit())),
            OpKind::Function => {
                for key in [&operation.binding_name, &operation.declaration_identity] {
                    if !key.is_empty() {
                        environment.define_function(
                            key.clone(),
                            operation_index,
                            self.interned(&operation.binding_name),
                        );
                    }
                }
                Ok(Flow::Value(unit()))
            }
            OpKind::Integer | OpKind::Length | OpKind::Float | OpKind::StringText => self
                .literal_value(kind, operation_index, operation)
                .map(|value| Flow::Value(RuntimeValue::Data(value))),
            OpKind::Boolean => Ok(Flow::Value(RuntimeValue::Data(Value::Bool(
                operation.detail == "true",
            )))),
            OpKind::Null => Ok(Flow::Value(RuntimeValue::Data(Value::Null))),
            OpKind::Unit => Ok(Flow::Value(unit())),
            OpKind::Duration => Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_TYPE,
                "duration literals exist only in the `concurrent` timeout clause (§15)",
                span(operation),
            ))),
            OpKind::Identifier => self.eval_identifier(operation_index, operation, environment),
            OpKind::Lambda => Ok(Flow::Value(RuntimeValue::Function {
                operation: operation_index,
                environment,
                name: None,
            })),
            OpKind::Placeholder => environment
                .slot("_")
                .map(|slot| Flow::Value(slot.borrow().clone()))
                .ok_or_else(|| {
                    runtime_diagnostic(topaz_value::fault(
                        topaz_value::codes::GUARD_TYPE,
                        "`_` is only valid inside a pipeline stage (§11)",
                        span(operation),
                    ))
                }),
            OpKind::Continue => Ok(Flow::Continue {
                target: operation.control_target.clone(),
            }),
            _ => Err(format!(
                "Stage 1 runtime simple operation routing drifted to `{}`",
                operation.kind
            )),
        }
    }

    /// Returns the operation's literal value, decoding the wire text once per
    /// program instead of once per evaluation. The decoded values are immutable
    /// scalars and shared strings, so reuse is indistinguishable from decoding
    /// again.
    fn literal_value(
        &self,
        kind: OpKind,
        operation_index: usize,
        operation: &Operation,
    ) -> Result<Value, String> {
        if let Some(Some(value)) = self.literals.borrow().get(operation_index) {
            return Ok(value.clone());
        }
        let value = match kind {
            OpKind::Length => {
                let (unit, ticks) = operation
                    .detail
                    .split_once(':')
                    .ok_or("invalid checked Length payload")?;
                let unit = topaz_value::LengthUnit::from_name(unit)
                    .ok_or("invalid checked Length unit")?;
                let ticks = ticks
                    .parse::<i64>()
                    .map_err(|_| "invalid checked Length ticks")?;
                topaz_value::length_value(topaz_value::Length { unit, ticks })
            }
            _ => match self.program.literal_facts[operation_index]
                .as_ref()
                .ok_or("missing admitted literal fact")?
                .as_ref()
                .map_err(Clone::clone)?
            {
                topaz_value::IrLiteral::Integer(value) => Value::Int(*value),
                topaz_value::IrLiteral::Float(bits) => Value::Float(f64::from_bits(*bits)),
                topaz_value::IrLiteral::Text(value) => Value::str(value.clone()),
                topaz_value::IrLiteral::Duration(_) => {
                    return Err("duration is not a value literal".into());
                }
            },
        };
        let mut literals = self.literals.borrow_mut();
        if literals.is_empty() {
            literals.resize(self.program.operations.len(), None);
        }
        if let Some(slot) = literals.get_mut(operation_index) {
            *slot = Some(value.clone());
        }
        Ok(value)
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
                match receiver {
                    RuntimeValue::Module(name) => {
                        return self
                            .module_member(&name, &operation.detail)
                            .map(Flow::Value);
                    }
                    RuntimeValue::Type(name) => return self.eval_nominal_member(operation, &name),
                    _ => {}
                }
                let receiver = data(receiver)?;
                let value = topaz_value::member_value_with_version(
                    &receiver,
                    &operation.detail,
                    span(operation),
                    self.program.language_version,
                )
                .and_then(|value| {
                    value.ok_or_else(|| {
                        no_member_fault(&receiver, &operation.detail, span(operation))
                    })
                })
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
        operation_index: usize,
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
                        EnvironmentFrame::for_constants(environment.clone()),
                    )
                    .await?;
                let key = format!("{}::{}", operation.module, operation.binding_name);
                environment.define_constant(key, value.clone());
                environment.define_constant(operation.binding_name.clone(), value.clone());
                if !operation.declaration_identity.is_empty() {
                    environment
                        .define_constant(operation.declaration_identity.clone(), value.clone());
                }
                // A global defined after initialization replaces the slot a
                // settled reference would name, so the settled entries go.
                if self.globals_settled.get() {
                    self.global_references.borrow_mut().clear();
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
                self.eval_record(operation_index, operation, environment)
                    .await
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
        for index in operations {
            let operation = &self.program.operations[*index];
            if self.program.kinds[*index] == OpKind::Function {
                for key in [&operation.binding_name, &operation.declaration_identity] {
                    if !key.is_empty() {
                        environment.reserve_function(
                            key.clone(),
                            *index,
                            self.interned(&operation.binding_name),
                        );
                    }
                }
            }
        }
        let mut last = unit();
        let mut deferred = Vec::new();
        let mut outcome = None;
        for operation in operations {
            if self.program.kinds[*operation] == OpKind::Defer {
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
            let step = match self.eval_leaf(*operation, &environment) {
                Some(result) => result,
                None => self.eval_async(*operation, environment.clone()).await,
            };
            match step {
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
        let flow = match self.eval_leaf(operation, &environment) {
            Some(result) => result?,
            None => self.eval_async(operation, environment).await?,
        };
        match flow {
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
        operation_index: usize,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let keys = [
            operation.reference_identity.as_str(),
            operation.detail.as_str(),
        ];
        if let Some(value) = environment.lookup(&keys) {
            return Ok(Flow::Value(value));
        }
        // Selected imports are actual outer bindings. Consult the initialized
        // source module before rejecting an unpublished local declaration.
        if let Some((module, name)) = self
            .program
            .selected_imports
            .get(&operation.module)
            .and_then(|imports| imports.get(&operation.detail))
            && let Some(imported) = self.module_environments.get(module)
            && let Some(value) = imported.lookup(&[name])
        {
            return Ok(Flow::Value(value));
        }
        // Neither static function targets nor prelude fallback publish a
        // reserved declaration. Only an actual outer execution binding wins.
        if environment.has_function(&keys) {
            return Err(runtime_diagnostic(topaz_value::fault(
                topaz_value::codes::GUARD_UNBOUND,
                format!("`{}` is not bound", operation.detail),
                span(operation),
            )));
        }
        if operation.reference_identity == "builtin::None" || operation.detail == "None" {
            return Ok(Flow::Value(RuntimeValue::Data(Value::None)));
        }
        let settled = self.globals_settled.get();
        if settled
            && let Some(Some(reference)) = self
                .global_references
                .borrow()
                .get(operation_index)
                .cloned()
        {
            return Ok(Flow::Value(self.global_reference_value(&reference)));
        }
        let reference = self.resolve_global_reference(operation, keys);
        let value = self.global_reference_value(&reference);
        if settled {
            let mut references = self.global_references.borrow_mut();
            if references.is_empty() {
                references.resize(self.program.operations.len(), None);
            }
            if let Some(slot) = references.get_mut(operation_index) {
                *slot = Some(Rc::new(reference));
            }
        }
        Ok(Flow::Value(value))
    }

    fn resolve_global_reference(&self, operation: &Operation, keys: [&str; 2]) -> GlobalReference {
        for key in keys {
            if !key.is_empty() {
                if let Some(slot) = self.globals.slot(key) {
                    return GlobalReference::Slot(slot);
                }
                if let Some(function) = self.functions.get(key) {
                    return GlobalReference::Function(*function);
                }
                if let Some((module, name)) = key.rsplit_once("::")
                    && let Some(environment) = self.module_environments.get(module)
                {
                    if let Some(slot) = environment.slot(name) {
                        return GlobalReference::Slot(slot);
                    }
                    if let Some(RuntimeValue::Function { operation, .. }) =
                        environment.lookup(&[name])
                    {
                        return GlobalReference::Function(operation);
                    }
                }
            }
        }
        if let Some(imported) = self
            .program
            .namespace_imports
            .get(&operation.module)
            .and_then(|imports| imports.get(&operation.detail))
        {
            return GlobalReference::Module(imported.clone());
        }
        if let Some(kind) = direct_builtin_with_version(
            &operation.reference_identity,
            self.program.language_version,
        )
        .or_else(|| Builtin::free_with_version(&operation.detail, self.program.language_version))
        {
            return GlobalReference::Builtin(kind);
        }
        let name = if !operation.reference_identity.is_empty()
            && self.nominals.get(&operation.reference_identity).is_some()
        {
            operation.reference_identity.clone()
        } else {
            operation.detail.clone()
        };
        GlobalReference::Type(name)
    }

    fn global_reference_value(&self, reference: &GlobalReference) -> RuntimeValue {
        match reference {
            GlobalReference::Slot(slot) => slot.borrow().clone(),
            GlobalReference::Function(function) => RuntimeValue::Function {
                operation: *function,
                environment: self.function_environment(*function),
                name: Some(self.interned(&self.program.operations[*function].binding_name)),
            },
            GlobalReference::Builtin(kind) => RuntimeValue::Data(Value::Builtin {
                kind: *kind,
                recv: None,
            }),
            GlobalReference::Type(name) => RuntimeValue::Type(name.clone()),
            GlobalReference::Module(name) => RuntimeValue::Module(name.clone()),
        }
    }

    pub(crate) fn module_member(&self, module: &str, member: &str) -> Result<RuntimeValue, String> {
        let identity = format!("{module}::{member}");
        if let Some(slot) = self.globals.slot(&identity) {
            return Ok(slot.borrow().clone());
        }
        if let Some(environment) = self.module_environments.get(module)
            && let Some(value) = environment.lookup(&[member])
        {
            return Ok(value);
        }
        if let Some(function) = self.functions.get(&identity) {
            return Ok(RuntimeValue::Function {
                operation: *function,
                environment: self.function_environment(*function),
                name: Some(self.interned(&self.program.operations[*function].binding_name)),
            });
        }
        if self.nominals.get(&identity).is_some() {
            return Ok(RuntimeValue::Type(identity));
        }
        Err(format!(
            "self target module `{module}` has no initialized member `{member}`"
        ))
    }

    pub(crate) async fn eval_string(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        if let Some((tag, _)) = operation.detail.split_once('|') {
            let mut parts = Vec::new();
            let mut part = String::new();
            let mut values = Vec::new();
            for operand in &operation.operands {
                let value = data(self.eval_value(*operand, environment.clone()).await?)?;
                if self.program.kinds[*operand] == OpKind::StringText {
                    part.push_str(&topaz_value::value::render(&value));
                } else {
                    parts.push(std::mem::take(&mut part));
                    values.push(value);
                }
            }
            parts.push(part);
            return Ok(Flow::Value(RuntimeValue::Data(topaz_value::make_template(
                tag.to_string(),
                parts,
                values,
            ))));
        }
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
                        span_at(operation, argument.lo, argument.hi),
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

    /// Returns the record declaration this literal builds, reading its nominal
    /// fact once per site.
    fn record_site(
        &self,
        operation_index: usize,
        operation: &Operation,
    ) -> Result<Option<Rc<RecordShape>>, String> {
        if let Some(Some(entry)) = self.record_sites.borrow().get(operation_index) {
            return Ok(entry.clone());
        }
        let nominals = self.nominals.clone();
        let shape = match nominals.operation(&operation.id) {
            Some(fact) if fact.kind == "record" => Some(self.record_shape(fact)),
            Some(fact) => {
                return Err(format!(
                    "self target operation `{}` is a record literal but its nominal fact is `{}`",
                    operation.id, fact.kind
                ));
            }
            None => None,
        };
        let mut sites = self.record_sites.borrow_mut();
        if sites.is_empty() {
            sites.resize(self.program.operations.len(), None);
        }
        if let Some(slot) = sites.get_mut(operation_index) {
            *slot = Some(shape.clone());
        }
        Ok(shape)
    }

    pub(crate) async fn eval_record(
        &mut self,
        operation_index: usize,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let mut fields = BTreeMap::new();
        let nominals = self.nominals.clone();
        let mut nominal = None::<(
            Rc<str>,
            Option<Rc<str>>,
            Option<Rc<str>>,
            Vec<(Rc<str>, Option<Rc<str>>)>,
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
                        let Some(fact) = nominals.get(&name) else {
                            continue;
                        };
                        if fact.kind != "record" {
                            return Err(format!(
                                "self target record literal `{}` uses non-record `{}`",
                                operation.id, fact.identity
                            ));
                        }
                        let shape = self.record_shape(fact);
                        nominal = Some((
                            shape.name.clone(),
                            Some(shape.identity.clone()),
                            Some(shape.identity.clone()),
                            shape.members.clone(),
                        ));
                    }
                    RuntimeValue::Module(name) => {
                        return Err(format!(
                            "self target record literal `{}` has module base `{name}`",
                            operation.id
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
                            .operation_index(&default_operation_id)
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
            let value = Value::NominalRecord {
                record_id,
                declaration_identity,
                method_identity,
                fields: ordered.into(),
            };
            return Ok(Flow::Value(RuntimeValue::Data(value)));
        }
        if let Some(shape) = self.record_site(operation_index, operation)? {
            let identity = shape.identity.clone();
            let mut ordered = Vec::with_capacity(shape.members.len());
            for (name, default_operation_id) in &shape.members {
                let value = match fields.remove(name.as_ref()) {
                    Some(value) => value,
                    None => {
                        let default_operation_id =
                            default_operation_id.as_deref().ok_or_else(|| {
                                format!(
                                    "self target record literal `{identity}` omitted field `{name}`"
                                )
                            })?;
                        let default_operation = self
                            .operation_index(default_operation_id)
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
                ordered.push((name.clone(), value));
            }
            if !fields.is_empty() {
                return Err(format!(
                    "self target record literal `{identity}` has unprojected field(s): {}",
                    fields.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            return Ok(Flow::Value(RuntimeValue::Data(Value::NominalRecord {
                record_id: shape.name.clone(),
                declaration_identity: Some(identity.clone()),
                method_identity: Some(identity),
                fields: ordered.into(),
            })));
        }
        Ok(Flow::Value(RuntimeValue::Data(Value::record(fields))))
    }
}
