use super::{environment::*, machine::*, model::*};
use crate::diagnostic::*;
use crate::program::{model::*, validate::*};
use crate::wire::*;
use crate::*;

impl Machine {
    pub(crate) async fn call_function_inner(
        &mut self,
        operation_index: usize,
        parent_environment: Environment,
        arguments: EvaluatedCallArguments,
    ) -> Result<RuntimeValue, String> {
        let operation = self.program.operations[operation_index].clone();
        let environment = EnvironmentFrame::child(parent_environment);
        let mut parameters = Vec::new();
        let mut projected_defaults = BTreeMap::new();
        let mut body = None;
        for (ordinal, operand) in operation.operands.iter().copied().enumerate() {
            let child = &self.program.operations[operand];
            if matches!(
                child.kind.as_str(),
                "binding/parameter" | "binding/variadic-parameter"
            ) {
                parameters.push(operand);
            } else if child.kind == "expression/block"
                || operation
                    .operand_labels
                    .get(ordinal)
                    .is_some_and(|label| label.starts_with("body:"))
            {
                body = Some(operand);
            } else if let Some(parameter_index) = operation
                .operand_labels
                .get(ordinal)
                .and_then(|label| function_default_parameter_index(label))
                .filter(|parameter_index| {
                    projected_defaults
                        .insert(*parameter_index, operand)
                        .is_some()
                })
            {
                return Err(format!(
                    "{} duplicates the default for parameter {parameter_index}",
                    operation.id
                ));
            }
        }
        let variadic = parameters.last().copied().filter(|parameter| {
            self.program.operations[*parameter].kind == "binding/variadic-parameter"
        });
        let fixed_count = parameters.len() - usize::from(variadic.is_some());
        let fixed_parameters = &parameters[..fixed_count];
        if projected_defaults.keys().any(|index| *index >= fixed_count) {
            return Err(format!(
                "{} projects a default outside its fixed parameters",
                operation.id
            ));
        }
        let supplied = arguments.supplied();
        let EvaluatedCallArguments {
            positional,
            named,
            spread,
            seen_spread,
        } = arguments;
        let positional_fixed = positional.len().min(fixed_count);
        let mut slots = vec![None; fixed_count];
        let mut rest = Vec::new();
        for (index, value) in positional.into_iter().enumerate() {
            if index < fixed_count {
                slots[index] = Some(value);
            } else if variadic.is_some() {
                rest.push(value);
            } else {
                return Err(format!(
                    "{}::{} expects at most {} argument(s), found {}",
                    operation.module, operation.binding_name, fixed_count, supplied
                ));
            }
        }
        if seen_spread {
            if variadic.is_none() {
                return Err("spread arguments require a variadic parameter (§5)".to_string());
            }
            if fixed_parameters[positional_fixed..]
                .iter()
                .enumerate()
                .any(|(offset, parameter)| {
                    function_parameter_default(
                        &self.program,
                        &projected_defaults,
                        positional_fixed + offset,
                        *parameter,
                    )
                    .is_none()
                })
            {
                return Err(
                    "a spread argument cannot skip an unsatisfied fixed parameter (§5)".to_string(),
                );
            }
            rest.extend(spread);
        }
        for (name, value) in named {
            let Some(index) = fixed_parameters.iter().position(|parameter| {
                let parameter = &self.program.operations[*parameter];
                parameter.binding_name == name
                    || (parameter.binding_name.is_empty() && parameter.detail == name)
            }) else {
                return Err(format!("no parameter named `{name}` (§5)"));
            };
            if slots[index].is_some() {
                return Err(format!("parameter `{name}` is given twice (§5)"));
            }
            slots[index] = Some(value);
        }
        for (index, (parameter, argument)) in
            fixed_parameters.iter().copied().zip(slots).enumerate()
        {
            let argument = match argument {
                Some(argument) => argument,
                None => {
                    let default = function_parameter_default(
                        &self.program,
                        &projected_defaults,
                        index,
                        parameter,
                    )
                    .ok_or_else(|| {
                        format!(
                            "{}::{} omitted required parameter {}",
                            operation.module, operation.binding_name, index
                        )
                    })?;
                    self.eval_value(default, environment.clone()).await?
                }
            };
            self.bind(parameter, argument, environment.clone())?;
        }
        if let Some(parameter) = variadic {
            let values = rest.into_iter().map(data).collect::<Result<Vec<_>, _>>()?;
            self.bind(
                parameter,
                RuntimeValue::Data(Value::array(values)),
                environment.clone(),
            )?;
        }
        match body {
            Some(body) => match self.eval_async(body, environment).await {
                Ok(Flow::Value(value) | Flow::Return(value)) => Ok(value),
                Ok(Flow::Break { .. } | Flow::Continue { .. }) => {
                    Err("loop control escaped a Stage 1 function".to_string())
                }
                Err(error) if error == PROPAGATE_SIGNAL => self
                    .propagating
                    .take()
                    .ok_or_else(|| "Stage 1 propagation value is missing".to_string()),
                Err(error) if error == RETURN_SIGNAL => self
                    .returning
                    .take()
                    .ok_or_else(|| "Stage 1 return value is missing".to_string()),
                Err(error) if error == BREAK_SIGNAL || error == CONTINUE_SIGNAL => {
                    self.loop_control.take();
                    Err("loop control escaped a Stage 1 function".to_string())
                }
                Err(error) => Err(error),
            },
            None => Ok(unit()),
        }
    }

    pub(crate) fn call_callback_callee<'a>(
        &'a mut self,
        callee: Value,
        arguments: Vec<Value>,
        operation: &'a Operation,
    ) -> LocalFuture<'a, Result<Value, String>> {
        Box::pin(self.call_callback_callee_body(callee, arguments, operation))
    }

    pub(crate) async fn call_callback_callee_body(
        &mut self,
        callee: Value,
        arguments: Vec<Value>,
        operation: &Operation,
    ) -> Result<Value, String> {
        if let Some((callback, environment)) = product_closure_parts(&callee) {
            return data(
                self.call_function_with_environment_async(
                    callback,
                    environment,
                    EvaluatedCallArguments::positional(
                        arguments.into_iter().map(RuntimeValue::Data).collect(),
                    ),
                    span(operation),
                )
                .await?,
            );
        }
        if let Value::Builtin { kind, recv } = callee {
            return self
                .call_builtin_value(
                    kind,
                    recv,
                    EvaluatedCallArguments::positional(
                        arguments.into_iter().map(RuntimeValue::Data).collect(),
                    ),
                    operation,
                )
                .await;
        }
        if let Value::Composed(pair) = callee {
            let first = self
                .call_callback_callee(pair.0.clone(), arguments, operation)
                .await?;
            return self
                .call_callback_callee(pair.1.clone(), vec![first], operation)
                .await;
        }
        Err(format!(
            "self target callback is not callable: {}",
            callee.kind()
        ))
    }

    pub(crate) async fn drive_callback_hof(
        &mut self,
        mut execution: CallbackHofExecution,
        operation: &Operation,
    ) -> Result<Value, String> {
        loop {
            match execution.next() {
                CallbackHofStep::Complete(value) => return Ok(value),
                CallbackHofStep::Call {
                    pending,
                    callee,
                    args,
                } => {
                    let result = self.call_callback_callee(callee, args, operation).await?;
                    execution = pending
                        .resume(result, span(operation))
                        .map_err(runtime_diagnostic)?;
                }
            }
        }
    }

    pub(crate) async fn drive_callback_key_collection(
        &mut self,
        mut collection: CallbackKeyCollection,
        operation: &Operation,
    ) -> Result<(Vec<Value>, Vec<Value>), String> {
        loop {
            match collection.next() {
                CallbackKeyStep::Complete { items, keys } => return Ok((items, keys)),
                CallbackKeyStep::Call(pending) => {
                    let (callee, item) = pending.invocation();
                    let key = self
                        .call_callback_callee(callee, vec![item], operation)
                        .await?;
                    collection = pending.resume(key);
                }
            }
        }
    }

    pub(crate) async fn drive_callback_retain(
        &mut self,
        mut execution: CallbackRetainExecution,
        operation: &Operation,
    ) -> Result<Vec<Value>, String> {
        loop {
            match execution.next() {
                CallbackRetainStep::Complete(values) => return Ok(values),
                CallbackRetainStep::Call(pending) => {
                    let (callee, item) = pending.invocation();
                    let predicate = self
                        .call_callback_callee(callee, vec![item], operation)
                        .await?;
                    execution = pending
                        .resume(predicate, span(operation))
                        .map_err(runtime_diagnostic)?;
                }
            }
        }
    }

    pub(crate) async fn drive_callback_map_hof(
        &mut self,
        mut execution: CallbackMapHofExecution,
        operation: &Operation,
    ) -> Result<Value, String> {
        loop {
            match execution.next() {
                CallbackMapHofStep::Complete(value) => return Ok(value),
                CallbackMapHofStep::Call {
                    pending,
                    callee,
                    args,
                } => {
                    let result = self.call_callback_callee(callee, args, operation).await?;
                    execution = pending
                        .resume(result, span(operation))
                        .map_err(runtime_diagnostic)?;
                }
            }
        }
    }

    pub(crate) async fn drive_callback_receiver_map(
        &mut self,
        step: CallbackReceiverMapStep,
        method: &str,
        operation: &Operation,
    ) -> Result<Value, String> {
        match step {
            CallbackReceiverMapStep::Complete(value) => Ok(value),
            CallbackReceiverMapStep::Call {
                pending,
                callee,
                input,
            } => {
                let result = self
                    .call_callback_callee(callee, vec![input], operation)
                    .await?;
                Ok(pending.resume(result))
            }
            CallbackReceiverMapStep::Delegate { receiver, callback } => {
                self.drive_callback_hof(
                    prepare_callback_hof(
                        CallbackHofKind::Map,
                        vec![receiver, callback],
                        span(operation),
                    )
                    .map_err(runtime_diagnostic)?,
                    operation,
                )
                .await
            }
            CallbackReceiverMapStep::Unsupported { receiver } => Err(runtime_diagnostic(
                no_member_fault(&receiver, method, span(operation)),
            )),
        }
    }

    pub(crate) async fn call_callback_builtin(
        &mut self,
        kind: Builtin,
        recv: Option<Rc<Value>>,
        args: Vec<Value>,
        operation: &Operation,
    ) -> Result<Option<Value>, String> {
        if let Some(hof) = CallbackHofKind::from_builtin(kind) {
            let args = match recv.as_ref() {
                Some(receiver) => {
                    let mut receiver_args = Vec::with_capacity(args.len() + 1);
                    receiver_args.push((**receiver).clone());
                    receiver_args.extend(args);
                    receiver_args
                }
                None => args,
            };
            return self
                .drive_callback_hof(
                    prepare_callback_hof(hof, args, span(operation)).map_err(runtime_diagnostic)?,
                    operation,
                )
                .await
                .map(Some);
        }
        let Some(receiver) = recv else {
            return Ok(None);
        };
        let receiver = (*receiver).clone();
        let result = match kind {
            Builtin::OkOrElse => {
                let [callback]: [Value; 1] = args.try_into().map_err(|values: Vec<Value>| {
                    format!("okOrElse expects one callback, found {}", values.len())
                })?;
                match prepare_callback_ok_or_else(receiver, callback) {
                    CallbackOkOrElseStep::Complete(value) => value,
                    CallbackOkOrElseStep::Call { pending, callee } => {
                        let result = self
                            .call_callback_callee(callee, Vec::new(), operation)
                            .await?;
                        pending.resume(result)
                    }
                    CallbackOkOrElseStep::Unsupported { receiver } => {
                        return Err(runtime_diagnostic(no_member_fault(
                            &receiver,
                            "okOrElse",
                            span(operation),
                        )));
                    }
                }
            }
            Builtin::OptionMap | Builtin::ResultMap => {
                let [callback]: [Value; 1] = args.try_into().map_err(|values: Vec<Value>| {
                    format!("map expects one callback, found {}", values.len())
                })?;
                self.drive_callback_receiver_map(
                    prepare_callback_receiver_map(receiver, callback),
                    "map",
                    operation,
                )
                .await?
            }
            Builtin::OptionFlatMap | Builtin::ResultFlatMap => {
                let [callback]: [Value; 1] = args.try_into().map_err(|values: Vec<Value>| {
                    format!("flatMap expects one callback, found {}", values.len())
                })?;
                self.drive_callback_receiver_map(
                    prepare_callback_receiver_flat_map(receiver, callback),
                    "flatMap",
                    operation,
                )
                .await?
            }
            Builtin::ArrSortBy | Builtin::ArrSortedBy => {
                let [callback]: [Value; 1] = args.try_into().map_err(|values: Vec<Value>| {
                    format!("callback sort expects one callback, found {}", values.len())
                })?;
                let Value::Array(cell) = receiver else {
                    return Err(runtime_diagnostic(no_member_fault(
                        &receiver,
                        if kind == Builtin::ArrSortBy {
                            "sortBy"
                        } else {
                            "sortedBy"
                        },
                        span(operation),
                    )));
                };
                let items = cell.borrow().clone();
                let (items, keys) = self
                    .drive_callback_key_collection(
                        prepare_callback_key_collection(items, callback),
                        operation,
                    )
                    .await?;
                let sorted =
                    sorted_by_keys(&items, &keys, span(operation)).map_err(runtime_diagnostic)?;
                if kind == Builtin::ArrSortBy {
                    *cell.borrow_mut() = sorted;
                    Value::Unit
                } else {
                    Value::array(sorted)
                }
            }
            Builtin::ArrRetain => {
                let [callback]: [Value; 1] = args.try_into().map_err(|values: Vec<Value>| {
                    format!("retain expects one callback, found {}", values.len())
                })?;
                let Value::Array(cell) = receiver else {
                    return Err(runtime_diagnostic(no_member_fault(
                        &receiver,
                        "retain",
                        span(operation),
                    )));
                };
                let items = cell.borrow().clone();
                let retained = self
                    .drive_callback_retain(prepare_callback_retain(items, callback), operation)
                    .await?;
                *cell.borrow_mut() = retained;
                Value::Unit
            }
            Builtin::MapFilter | Builtin::MapMapValues => {
                let [callback]: [Value; 1] = args.try_into().map_err(|values: Vec<Value>| {
                    format!("map callback expects one callback, found {}", values.len())
                })?;
                let Value::Map(map) = receiver else {
                    return Err(runtime_diagnostic(no_member_fault(
                        &receiver,
                        if kind == Builtin::MapFilter {
                            "filter"
                        } else {
                            "mapValues"
                        },
                        span(operation),
                    )));
                };
                let callback_kind = if kind == Builtin::MapFilter {
                    CallbackMapHofKind::Filter
                } else {
                    CallbackMapHofKind::MapValues
                };
                let pairs = map.borrow().pairs();
                self.drive_callback_map_hof(
                    prepare_callback_map_hof(callback_kind, pairs, callback),
                    operation,
                )
                .await?
            }
            Builtin::MapUpdate => {
                let [key, initial, callback]: [Value; 3] =
                    args.try_into().map_err(|values: Vec<Value>| {
                        format!("update expects three arguments, found {}", values.len())
                    })?;
                let Value::Map(map) = receiver else {
                    return Err(runtime_diagnostic(no_member_fault(
                        &receiver,
                        "update",
                        span(operation),
                    )));
                };
                match prepare_callback_map_update(map, key, initial, callback, span(operation))
                    .map_err(runtime_diagnostic)?
                {
                    CallbackMapUpdateStep::Complete(value) => value,
                    CallbackMapUpdateStep::Call {
                        pending,
                        callee,
                        existing,
                    } => {
                        let result = self
                            .call_callback_callee(callee, vec![existing], operation)
                            .await?;
                        pending
                            .resume(result, span(operation))
                            .map_err(runtime_diagnostic)?
                    }
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    pub(crate) async fn call_builtin_value(
        &mut self,
        kind: Builtin,
        recv: Option<Rc<Value>>,
        arguments: EvaluatedCallArguments,
        operation: &Operation,
    ) -> Result<Value, String> {
        let mut arguments = arguments.into_builtin_data(kind, recv.is_some(), operation)?;
        if kind == Builtin::Input {
            return if arguments.is_empty() {
                Ok(Value::str(self.stdin.clone()))
            } else {
                Err("input expects no arguments".to_string())
            };
        }
        if let Some(outcome) = call_pure_builtin(kind, &mut arguments, span(operation)) {
            return outcome.map_err(runtime_diagnostic);
        }
        if let Some(host) = self.host.as_deref()
            && let Some(outcome) = call_host_builtin(host, kind, &mut arguments, span(operation))
        {
            let value = outcome.map_err(runtime_diagnostic)?;
            return Ok(if kind.lispex_application_operation().is_some() {
                self.nominals.project_host_value(value)
            } else {
                value
            });
        }
        let callback_route = CallbackHofKind::from_builtin(kind).is_some()
            || recv.as_ref().is_some_and(|receiver| {
                receiver_builtin_by_kind(receiver, kind)
                    .is_some_and(|route| route.route == ReceiverBuiltinRoute::Callback)
            });
        if callback_route {
            return self
                .call_callback_builtin(kind, recv, arguments, operation)
                .await?
                .ok_or_else(|| format!("self target builtin `{kind:?}` lost its callback route"));
        }
        if let Some(receiver) = recv
            && let Some(route) = receiver_builtin_by_kind(&receiver, kind)
        {
            return match route.route {
                ReceiverBuiltinRoute::Method => call_method(
                    (*receiver).clone(),
                    route.name,
                    arguments,
                    span(operation),
                    span(operation),
                )
                .map_err(runtime_diagnostic),
                ReceiverBuiltinRoute::Resource => {
                    let host = self.host.as_deref().ok_or_else(|| {
                        format!(
                            "self target resource method `{}` has no admitted product host",
                            route.name
                        )
                    })?;
                    call_resource_method(
                        host,
                        (*receiver).clone(),
                        route.name,
                        arguments,
                        span(operation),
                        span(operation),
                    )
                    .map_err(runtime_diagnostic)
                }
                ReceiverBuiltinRoute::Callback => Err(format!(
                    "self target callback method `{}` has no executable route",
                    route.name
                )),
            };
        }
        Err(format!(
            "self target builtin `{kind:?}` has no executable route"
        ))
    }

    pub(crate) fn call_runtime_callee<'a>(
        &'a mut self,
        callee: RuntimeValue,
        arguments: EvaluatedCallArguments,
        operation: &'a Operation,
    ) -> LocalFuture<'a, Result<Flow, String>> {
        Box::pin(self.call_runtime_callee_body(callee, arguments, operation))
    }

    pub(crate) async fn call_runtime_callee_body(
        &mut self,
        callee: RuntimeValue,
        arguments: EvaluatedCallArguments,
        operation: &Operation,
    ) -> Result<Flow, String> {
        match callee {
            RuntimeValue::Function {
                operation: function,
                environment,
            } => self
                .call_function_with_environment_async(
                    function,
                    environment,
                    arguments,
                    span(operation),
                )
                .await
                .map(Flow::Value),
            RuntimeValue::Data(value) => {
                if let Some((function, environment)) = product_closure_parts(&value) {
                    return self
                        .call_function_with_environment_async(
                            function,
                            environment,
                            arguments,
                            span(operation),
                        )
                        .await
                        .map(Flow::Value);
                }
                if let Value::Builtin { kind, recv } = value {
                    return self
                        .call_builtin_value(kind, recv, arguments, operation)
                        .await
                        .map(RuntimeValue::Data)
                        .map(Flow::Value);
                }
                if let Value::Composed(pair) = value {
                    let first = self
                        .call_runtime_callee(
                            RuntimeValue::Data(pair.0.clone()),
                            arguments,
                            operation,
                        )
                        .await?;
                    let Flow::Value(first) = first else {
                        return Err("control flow escaped a composed call".to_string());
                    };
                    return self
                        .call_runtime_callee(
                            RuntimeValue::Data(pair.1.clone()),
                            EvaluatedCallArguments::positional(vec![first]),
                            operation,
                        )
                        .await;
                }
                Err(format!(
                    "Stage 1 call target `{}` is not callable",
                    operation.call_target
                ))
            }
            RuntimeValue::Type(name) if matches!(name.as_str(), "Some" | "Ok" | "Err") => {
                let mut builtin_operation = operation.clone();
                builtin_operation.call_target = format!("builtin::{name}");
                self.call_builtin(&builtin_operation, arguments.into_positional(operation)?)
            }
            RuntimeValue::Type(name) => {
                self.construct_nominal(operation, &name, arguments.into_positional(operation)?)
            }
            RuntimeValue::EnumConstructor {
                identity,
                variant,
                variant_index,
                arity,
            } => self.construct_enum_value(
                operation,
                &identity,
                &variant,
                variant_index,
                arity,
                arguments.into_positional(operation)?,
            ),
        }
    }

    pub(crate) fn bind(
        &self,
        operation_index: usize,
        value: RuntimeValue,
        environment: Environment,
    ) -> Result<(), String> {
        let operation = &self.program.operations[operation_index];
        if operation.kind == "pattern/wildcard" {
            return Ok(());
        }
        if !matches!(
            operation.kind.as_str(),
            "pattern/binding"
                | "pattern/typed-binding"
                | "binding/parameter"
                | "binding/variadic-parameter"
        ) {
            return Err(format!(
                "unsupported Stage 1 binding pattern `{}`",
                operation.kind
            ));
        }
        self.bind_declared_value(operation, value, environment);
        Ok(())
    }

    pub(crate) fn bind_declared_value(
        &self,
        operation: &Operation,
        value: RuntimeValue,
        environment: Environment,
    ) {
        if !operation.declaration_identity.is_empty() {
            environment.define(operation.declaration_identity.clone(), value.clone());
        }
        if !operation.binding_name.is_empty() {
            environment.define(operation.binding_name.clone(), value);
        } else if !operation.detail.is_empty() {
            environment.define(operation.detail.clone(), value);
        }
    }

    pub(crate) fn planned_call_layout(
        &self,
        operation: &Operation,
    ) -> Result<PlannedCallLayout, String> {
        let direct = operation.kind == "expression/call";
        let (callee_operand, lead_operand, stage_arguments, method) = if direct {
            (
                *operation
                    .operands
                    .first()
                    .ok_or_else(|| format!("{} has no call callee", operation.id))?,
                None,
                Some(operation.operands.as_slice()),
                operation.call_method.clone(),
            )
        } else {
            let lead = *operation
                .operands
                .first()
                .ok_or_else(|| format!("{} has no pipeline lead", operation.id))?;
            let stage_index = *operation
                .operands
                .get(1)
                .ok_or_else(|| format!("{} has no pipeline stage", operation.id))?;
            let stage = &self.program.operations[stage_index];
            if stage.kind == "expression/call" {
                (
                    *stage
                        .operands
                        .first()
                        .ok_or_else(|| format!("{} has no stage callee", operation.id))?,
                    Some(lead),
                    Some(stage.operands.as_slice()),
                    operation.call_stage_method.clone(),
                )
            } else {
                (
                    stage_index,
                    Some(lead),
                    None,
                    operation.call_stage_method.clone(),
                )
            }
        };
        let receiver_operand = if method.is_empty() {
            None
        } else {
            Some(
                *self.program.operations[callee_operand]
                    .operands
                    .first()
                    .ok_or_else(|| format!("{} has no member receiver", operation.id))?,
            )
        };
        let mut argument_operands = Vec::with_capacity(operation.call_arguments.len());
        for (index, argument) in operation.call_arguments.iter().enumerate() {
            if matches!(argument.binding, CallArgumentBinding::InsertedLead) {
                argument_operands.push(lead_operand.ok_or_else(|| {
                    format!("{} argument {index} has no pipeline lead", operation.id)
                })?);
                continue;
            }
            let source_index = argument
                .source_index
                .ok_or_else(|| format!("{} argument {index} has no source index", operation.id))?;
            let operands = stage_arguments.ok_or_else(|| {
                format!(
                    "{} non-call pipeline stage has written arguments",
                    operation.id
                )
            })?;
            argument_operands.push(*operands.get(source_index + 1).ok_or_else(|| {
                format!(
                    "{} argument {index} source index {source_index} is out of range",
                    operation.id
                )
            })?);
        }
        Ok(PlannedCallLayout {
            callee_operand,
            pipe_lead_operand: lead_operand,
            receiver_operand,
            argument_operands,
            method,
        })
    }

    pub(crate) fn prepare_planned_member(
        &self,
        operation: &Operation,
        method: &str,
        receiver: &RuntimeValue,
    ) -> Result<Option<RuntimeValue>, String> {
        if method.is_empty() {
            return Ok(None);
        }
        match receiver {
            RuntimeValue::Type(_) => Ok(None),
            RuntimeValue::Data(value) => {
                if let Some(identity) = value.method_dispatch_id()
                    && operation.call_target == identity
                    && self
                        .receiver_methods
                        .contains_key(&(identity.to_string(), method.to_string()))
                {
                    return Ok(None);
                }
                if let Some(member) =
                    member_value(value, method, span(operation)).map_err(runtime_diagnostic)?
                {
                    return Ok(Some(RuntimeValue::Data(member)));
                }
                if receiver_builtin(value, method).is_some() {
                    return Ok(None);
                }
                Err(runtime_diagnostic(no_member_fault(
                    value,
                    method,
                    span(operation),
                )))
            }
            RuntimeValue::Function { .. } | RuntimeValue::EnumConstructor { .. } => Err(format!(
                "self target call `{}` has a non-data member receiver",
                operation.id
            )),
        }
    }

    pub(crate) async fn eval_planned_call(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let layout = self.planned_call_layout(operation)?;
        let mut callee = None;
        let mut receiver = None;
        let mut evaluated = (0..operation.call_arguments.len())
            .map(|_| None)
            .collect::<Vec<Option<EvaluatedArgument>>>();
        let mut wrap_some = false;
        let mut stage_environment = environment.clone();
        let mut pipe_lead_bound = false;
        for step in &operation.call_evaluations {
            match step {
                CallEvaluation::Callee => {
                    if callee.is_some() {
                        return Err(format!(
                            "self target call `{}` evaluates its callee twice",
                            operation.id
                        ));
                    }
                    callee = Some(
                        self.eval_value(layout.callee_operand, stage_environment.clone())
                            .await?,
                    );
                }
                CallEvaluation::Receiver => {
                    if receiver.is_some() || callee.is_some() {
                        return Err(format!(
                            "self target call `{}` evaluates its receiver twice",
                            operation.id
                        ));
                    }
                    if let Some(receiver_operand) = layout.receiver_operand {
                        receiver = Some(
                            self.eval_value(receiver_operand, stage_environment.clone())
                                .await?,
                        );
                        if !operation.call_optional {
                            let prepared = receiver.as_ref().ok_or_else(|| {
                                format!("self target call `{}` lost its receiver", operation.id)
                            })?;
                            callee =
                                self.prepare_planned_member(operation, &layout.method, prepared)?;
                        }
                    } else {
                        callee = Some(
                            self.eval_value(layout.callee_operand, stage_environment.clone())
                                .await?,
                        );
                    }
                }
                CallEvaluation::OptionalGuard => {
                    let guarded = receiver.take().ok_or_else(|| {
                        format!("self target call `{}` guards no receiver", operation.id)
                    })?;
                    match guarded {
                        RuntimeValue::Data(Value::None) => {
                            return Ok(Flow::Value(RuntimeValue::Data(Value::None)));
                        }
                        RuntimeValue::Data(Value::Null) => {
                            return Ok(Flow::Value(RuntimeValue::Data(Value::Null)));
                        }
                        RuntimeValue::Data(Value::Some(inner)) => {
                            receiver = Some(RuntimeValue::Data((*inner).clone()));
                            wrap_some = true;
                        }
                        other => receiver = Some(other),
                    }
                    let prepared = receiver.as_ref().ok_or_else(|| {
                        format!(
                            "self target call `{}` lost its guarded receiver",
                            operation.id
                        )
                    })?;
                    callee = self.prepare_planned_member(operation, &layout.method, prepared)?;
                }
                CallEvaluation::PipeLead => {
                    if pipe_lead_bound {
                        return Err(format!(
                            "self target call `{}` evaluates its pipeline lead twice",
                            operation.id
                        ));
                    }
                    let lead_operand = layout.pipe_lead_operand.ok_or_else(|| {
                        format!("self target call `{}` binds no pipeline lead", operation.id)
                    })?;
                    let lead = self.eval_value(lead_operand, environment.clone()).await?;
                    let placeholder_environment = EnvironmentFrame::child(environment.clone());
                    placeholder_environment.define("_".to_string(), lead);
                    stage_environment = placeholder_environment;
                    pipe_lead_bound = true;
                }
                CallEvaluation::Argument(index) => {
                    let index = *index;
                    if evaluated[index].is_some() {
                        return Err(format!(
                            "self target call `{}` evaluates argument {index} twice",
                            operation.id
                        ));
                    }
                    let argument = &operation.call_arguments[index];
                    let value = self
                        .eval_value(layout.argument_operands[index], stage_environment.clone())
                        .await?;
                    evaluated[index] = Some(match &argument.binding {
                        CallArgumentBinding::Spread => {
                            let mut spread = Vec::new();
                            call_spread_extend(
                                &mut spread,
                                data(value)?,
                                Span::new(FileId(0), argument.lo, argument.hi),
                            )
                            .map_err(runtime_diagnostic)?;
                            EvaluatedArgument::Spread(
                                spread.into_iter().map(RuntimeValue::Data).collect(),
                            )
                        }
                        _ => EvaluatedArgument::Value(value),
                    });
                }
            }
        }
        let mut arguments = EvaluatedCallArguments::default();
        for (index, (argument, value)) in operation.call_arguments.iter().zip(evaluated).enumerate()
        {
            let value = value.ok_or_else(|| {
                format!(
                    "self target call `{}` did not evaluate argument {index}",
                    operation.id
                )
            })?;
            match (&argument.binding, value) {
                (
                    CallArgumentBinding::Positional | CallArgumentBinding::InsertedLead,
                    EvaluatedArgument::Value(value),
                ) => {
                    if arguments.seen_spread {
                        arguments.spread.push(value);
                    } else {
                        arguments.positional.push(value);
                    }
                }
                (CallArgumentBinding::Named(name), EvaluatedArgument::Value(value)) => {
                    arguments.named.push((name.clone(), value));
                }
                (CallArgumentBinding::Spread, EvaluatedArgument::Spread(values)) => {
                    arguments.spread.extend(values);
                    arguments.seen_spread = true;
                }
                _ => {
                    return Err(format!(
                        "self target call `{}` has mismatched argument evaluation {index}",
                        operation.id
                    ));
                }
            }
        }
        let mut dispatched = operation.clone();
        dispatched.call_method = layout.method;
        let result = self
            .dispatch_call(&dispatched, environment, arguments, callee, receiver)
            .await?;
        if !wrap_some {
            return Ok(result);
        }
        match result {
            Flow::Value(RuntimeValue::Data(value)) => {
                Ok(Flow::Value(RuntimeValue::Data(Value::Some(Rc::new(value)))))
            }
            Flow::Value(_) => Err(format!(
                "self target optional call `{}` returned a non-data value",
                operation.id
            )),
            other => Ok(other),
        }
    }

    pub(crate) fn eval_call<'a>(
        &'a mut self,
        operation: &'a Operation,
        environment: Environment,
    ) -> LocalFuture<'a, Result<Flow, String>> {
        if !operation.call_evaluations.is_empty() {
            return Box::pin(self.eval_planned_call(operation, environment));
        }
        Box::pin(self.eval_unplanned_call(operation, environment))
    }

    pub(crate) async fn eval_unplanned_call(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let arguments = self
            .eval_call_arguments(operation, environment.clone())
            .await?;
        self.dispatch_call(operation, environment, arguments, None, None)
            .await
    }

    pub(crate) fn dispatch_call<'a>(
        &'a mut self,
        operation: &'a Operation,
        environment: Environment,
        arguments: EvaluatedCallArguments,
        prepared_callee: Option<RuntimeValue>,
        prepared_receiver: Option<RuntimeValue>,
    ) -> LocalFuture<'a, Result<Flow, String>> {
        let mut prepared_callee = prepared_callee;
        let prepared_is_dynamic = prepared_receiver.is_none()
            && prepared_callee.as_ref().is_some_and(|callee| match callee {
                RuntimeValue::Function {
                    operation: function,
                    ..
                } => self
                    .functions
                    .get(&operation.call_target)
                    .is_none_or(|target| target != function),
                RuntimeValue::Data(value) => {
                    if let Some((function, _)) = product_closure_parts(value) {
                        self.functions
                            .get(&operation.call_target)
                            .is_none_or(|target| *target != function)
                    } else if let Value::Builtin { kind, .. } = value {
                        direct_builtin(&operation.call_target) != Some(*kind)
                    } else {
                        true
                    }
                }
                RuntimeValue::Type(_) | RuntimeValue::EnumConstructor { .. } => false,
            });
        if prepared_is_dynamic {
            let Some(callee) = prepared_callee.take() else {
                let error = format!("self target call `{}` lost its callee", operation.id);
                return Box::pin(async move { Err(error) });
            };
            return self.call_runtime_callee(callee, arguments, operation);
        }
        let direct_value_callee = operation.call_callee_kind == "value"
            || (operation.call_callee_kind.is_empty()
                && operation.operands.first().is_some_and(|callee| {
                    self.program.operations[*callee].kind != "expression/member"
                }));
        if prepared_receiver.is_none()
            && direct_value_callee
            && operation.call_method.is_empty()
            && !uses_special_direct_dispatch(&operation.call_target)
            && let Some(function) = self.functions.get(&operation.call_target).copied()
        {
            let call_span = span(operation);
            return Box::pin(async move {
                self.call_function_with_arguments(function, arguments, call_span)
                    .await
                    .map(Flow::Value)
            });
        }
        Box::pin(self.dispatch_call_body(
            operation,
            environment,
            arguments,
            prepared_callee,
            prepared_receiver,
        ))
    }

    pub(crate) async fn dispatch_call_body(
        &mut self,
        operation: &Operation,
        environment: Environment,
        arguments: EvaluatedCallArguments,
        mut prepared_callee: Option<RuntimeValue>,
        prepared_receiver: Option<RuntimeValue>,
    ) -> Result<Flow, String> {
        if operation.call_target == "resolver::discoveryWord" {
            let arguments =
                arguments.into_parameter_data(&["bytes", "start", "word"], operation)?;
            let [Value::Bytes(bytes), Value::Int(start), Value::Str(word)] = arguments.as_slice()
            else {
                return Err(
                    "resolver::discoveryWord requires Bytes, int, and string arguments".to_string(),
                );
            };
            let start = usize::try_from(*start).ok();
            let expected = word.as_bytes();
            let matches = start.is_some_and(|start| {
                let end = start.saturating_add(expected.len());
                bytes.get(start..end) == Some(expected)
                    && bytes
                        .get(end)
                        .is_none_or(|value| !discovery_identifier_byte(i64::from(*value)))
            });
            return Ok(Flow::Value(RuntimeValue::Data(Value::Bool(matches))));
        }
        if matches!(
            operation.call_target.as_str(),
            "resolver::discoveryByte" | "raw::byteAt"
        ) {
            let arguments = arguments.into_parameter_data(&["bytes", "index"], operation)?;
            if arguments.len() != 2 {
                return Err(format!("{} expects two arguments", operation.call_target));
            }
            let Value::Bytes(bytes) = &arguments[0] else {
                return Err(format!("{} requires Bytes", operation.call_target));
            };
            let Value::Int(index) = arguments[1] else {
                return Err(format!("{} requires an int index", operation.call_target));
            };
            let value = usize::try_from(index)
                .ok()
                .and_then(|index| bytes.get(index).copied())
                .map(i64::from)
                .unwrap_or(-1);
            return Ok(Flow::Value(RuntimeValue::Data(Value::Int(value))));
        }
        if matches!(
            operation.call_target.as_str(),
            "resolver::discoveryIdentifierByte"
                | "raw::isAsciiDigit"
                | "raw::isIdentifierStart"
                | "raw::isIdentifierContinue"
                | "raw::utf8Width"
        ) {
            let parameter = if operation.call_target == "raw::utf8Width" {
                "first"
            } else {
                "value"
            };
            let arguments = arguments.into_parameter_data(&[parameter], operation)?;
            let [Value::Int(value)] = arguments.as_slice() else {
                return Err(format!(
                    "{} requires one int argument",
                    operation.call_target
                ));
            };
            let result = match operation.call_target.as_str() {
                "resolver::discoveryIdentifierByte" => {
                    RuntimeValue::Data(Value::Bool(discovery_identifier_byte(*value)))
                }
                "raw::isAsciiDigit" => RuntimeValue::Data(Value::Bool((48..=57).contains(value))),
                "raw::isIdentifierStart" => RuntimeValue::Data(Value::Bool(
                    *value == 95 || (65..=90).contains(value) || (97..=122).contains(value),
                )),
                "raw::isIdentifierContinue" => RuntimeValue::Data(Value::Bool(
                    *value == 95
                        || (48..=57).contains(value)
                        || (65..=90).contains(value)
                        || (97..=122).contains(value),
                )),
                "raw::utf8Width" => RuntimeValue::Data(Value::Int(if *value < 128 {
                    1
                } else if (194..=223).contains(value) {
                    2
                } else if (224..=239).contains(value) {
                    3
                } else if (240..=244).contains(value) {
                    4
                } else {
                    0
                })),
                _ => unreachable!(),
            };
            return Ok(Flow::Value(result));
        }
        if operation.call_target.starts_with("std.http::") {
            let names = match operation.call_target.as_str() {
                "std.http::text" | "std.http::json" => &["status", "body"][..],
                "std.http::header" => &["request", "name"][..],
                _ => &[][..],
            };
            let arguments = arguments.into_parameter_data(names, operation)?;
            return self.call_http(operation, arguments);
        }
        if let Some(rule_name) = operation
            .call_target
            .strip_prefix("topaz.lispex-rule-handle/v1:")
        {
            let generated_factory = format!("std.lispex.rules::{rule_name}");
            let function = self
                .functions
                .get(&generated_factory)
                .copied()
                .ok_or_else(|| {
                    format!(
                        "checked Lispex rule target `{}` has no generated factory",
                        operation.call_target
                    )
                })?;
            return self
                .call_function_with_arguments(function, arguments, span(operation))
                .await
                .map(Flow::Value);
        }
        if operation.call_target.starts_with("std.lispex::")
            && let Some(function) = self.functions.get(&operation.call_target).copied()
        {
            return self
                .call_function_with_arguments(function, arguments, span(operation))
                .await
                .map(Flow::Value);
        }
        let mut dispatched_operation = operation.clone();
        if dispatched_operation.call_method.is_empty()
            && let Some(callee) = dispatched_operation
                .operands
                .first()
                .map(|index| &self.program.operations[*index])
            && callee.kind == "expression/member"
        {
            dispatched_operation.call_method = callee.detail.clone();
        }
        let operation = &dispatched_operation;
        if !operation.call_method.is_empty() {
            let receiver = match prepared_receiver {
                Some(receiver) => receiver,
                None => {
                    let callee_operation = *operation
                        .operands
                        .first()
                        .ok_or_else(|| format!("{} has no member callee", operation.id))?;
                    let receiver_operation = *self.program.operations[callee_operation]
                        .operands
                        .first()
                        .ok_or_else(|| format!("{} has no member receiver", operation.id))?;
                    self.eval_value(receiver_operation, environment).await?
                }
            };
            if let RuntimeValue::Data(value) = &receiver
                && let Some(identity) = value.method_dispatch_id()
                && operation.call_target == identity
                && let Some(function) = self
                    .receiver_methods
                    .get(&(identity.to_string(), operation.call_method.clone()))
                    .copied()
            {
                let mut method_arguments = arguments;
                method_arguments.prepend(receiver);
                return self
                    .call_function_with_arguments(function, method_arguments, span(operation))
                    .await
                    .map(Flow::Value);
            }
            if let RuntimeValue::Type(_) = &receiver
                && let Some(identity) =
                    arguments
                        .positional
                        .first()
                        .and_then(|argument| match argument {
                            RuntimeValue::Data(value) => value.nominal_id(),
                            _ => None,
                        })
                && let Some(function) = self
                    .protocol_methods
                    .get(&(
                        operation.call_target.clone(),
                        identity.to_string(),
                        operation.call_method.clone(),
                    ))
                    .copied()
            {
                return self
                    .call_function_with_arguments(function, arguments, span(operation))
                    .await
                    .map(Flow::Value);
            }
            if let Some(callee) = prepared_callee.take() {
                return self.call_runtime_callee(callee, arguments, operation).await;
            }
            if let RuntimeValue::Data(value) = &receiver
                && let Some(route) = receiver_builtin(value, &operation.call_method)
                && matches!(route.route, ReceiverBuiltinRoute::Callback)
            {
                let all_names = builtin_param_names(route.kind);
                let offset = usize::from(matches!(
                    route.kind,
                    Builtin::MapFn | Builtin::FilterFn | Builtin::ReduceFn
                ));
                let callback_arguments =
                    arguments.into_runtime_parameters(&all_names[offset..], operation)?;
                let callback_arguments = callback_arguments
                    .into_iter()
                    .map(data)
                    .collect::<Result<Vec<_>, _>>()?;
                let result = self
                    .call_callback_builtin(
                        route.kind,
                        Some(Rc::new(value.clone())),
                        callback_arguments,
                        operation,
                    )
                    .await?
                    .ok_or_else(|| {
                        format!(
                            "self target callback method `{}` has no executable route",
                            operation.call_method
                        )
                    })?;
                return Ok(Flow::Value(RuntimeValue::Data(result)));
            }
            if let RuntimeValue::Type(name) = receiver {
                let arguments = match Builtin::static_namespace(&name, &operation.call_method) {
                    Some(kind) => arguments.into_builtin_data(kind, false, operation)?,
                    None if name == "JSON" && operation.call_method == "parseAs" => {
                        arguments.into_parameter_data(&["text"], operation)?
                    }
                    None if name == "JSON" && operation.call_method == "decode" => {
                        arguments.into_parameter_data(&["value"], operation)?
                    }
                    None => arguments
                        .into_positional(operation)?
                        .into_iter()
                        .map(data)
                        .collect::<Result<Vec<_>, _>>()?,
                };
                if matches!(
                    (name.as_str(), operation.call_method.as_str()),
                    ("Show", "show") | ("Eq", "equals") | ("Order", "compare")
                ) {
                    let result = builtin_protocol_dispatch(
                        &name,
                        &operation.call_method,
                        arguments,
                        span(operation),
                    )
                    .map_err(runtime_diagnostic)?;
                    return Ok(Flow::Value(RuntimeValue::Data(result)));
                }
                if self
                    .nominals
                    .get(&name)
                    .is_some_and(|fact| fact.kind == "enum")
                {
                    return self.construct_enum(
                        operation,
                        &name,
                        &operation.call_method,
                        arguments,
                    );
                }
                return self.call_static(operation, &name, arguments);
            }
            let receiver = data(receiver)?;
            let route = receiver_builtin(&receiver, &operation.call_method).ok_or_else(|| {
                runtime_diagnostic(no_member_fault(
                    &receiver,
                    &operation.call_method,
                    span(operation),
                ))
            })?;
            let NamedDataArguments { positional, named } = arguments.into_named_data(operation)?;
            let result = if route.route == ReceiverBuiltinRoute::Resource {
                let host = self.host.as_deref().ok_or_else(|| {
                    format!(
                        "self target resource method `{}` has no admitted product host",
                        operation.call_method
                    )
                })?;
                call_resource_method_named(
                    host,
                    receiver,
                    &operation.call_method,
                    positional,
                    named,
                    span(operation),
                    span(operation),
                )
            } else {
                call_method_named(
                    receiver,
                    &operation.call_method,
                    positional,
                    named,
                    span(operation),
                    span(operation),
                )
            }
            .map_err(runtime_diagnostic)?;
            return Ok(Flow::Value(RuntimeValue::Data(result)));
        }
        if operation.call_target.starts_with("builtin::") {
            if let Some(kind) = direct_builtin(&operation.call_target) {
                return self
                    .call_builtin_value(kind, None, arguments, operation)
                    .await
                    .map(RuntimeValue::Data)
                    .map(Flow::Value);
            }
            let names = match operation.call_target.as_str() {
                "builtin::Some" | "builtin::Ok" | "builtin::Err" => &["value"][..],
                "builtin::None" => &[][..],
                _ => return self.call_builtin(operation, arguments.into_positional(operation)?),
            };
            let arguments = arguments
                .into_parameter_data(names, operation)?
                .into_iter()
                .map(RuntimeValue::Data)
                .collect();
            return self.call_builtin(operation, arguments);
        }
        if let Some(function) = self.functions.get(&operation.call_target).copied() {
            return self
                .call_function_with_arguments(function, arguments, span(operation))
                .await
                .map(Flow::Value);
        }
        let callee = match prepared_callee {
            Some(callee) => callee,
            None => self.eval_value(operation.operands[0], environment).await?,
        };
        self.call_runtime_callee(callee, arguments, operation).await
    }

    pub(crate) fn call_builtin(
        &mut self,
        operation: &Operation,
        arguments: Vec<RuntimeValue>,
    ) -> Result<Flow, String> {
        let mut arguments = arguments
            .into_iter()
            .map(data)
            .collect::<Result<Vec<_>, _>>()?;
        let value = match operation.call_target.as_str() {
            "builtin::Some" => Value::Some(Rc::new(one(&mut arguments, operation)?)),
            "builtin::None" => Value::None,
            "builtin::Ok" => Value::Ok(Rc::new(one(&mut arguments, operation)?)),
            "builtin::Err" => Value::Err(Rc::new(one(&mut arguments, operation)?)),
            "builtin::input" => {
                if !arguments.is_empty() {
                    return Err("input expects no arguments".to_string());
                }
                Value::str(self.stdin.clone())
            }
            other => return Err(format!("unsupported Stage 1 builtin `{other}`")),
        };
        Ok(Flow::Value(RuntimeValue::Data(value)))
    }

    pub(crate) fn call_http(
        &self,
        operation: &Operation,
        mut arguments: Vec<Value>,
    ) -> Result<Flow, String> {
        let response = |status: i64, media_type: &str, body: Vec<u8>| -> Result<Value, String> {
            let mut headers = topaz_value::value::OrderedMap::default();
            headers
                .insert_value(
                    &Value::str("content-type"),
                    Value::array(vec![Value::str(media_type)]),
                )
                .map_err(|error| format!("{error:?}"))?;
            Ok(Value::nominal_record(
                "HttpResponse",
                [
                    (Rc::from("status"), Value::Int(status)),
                    (
                        Rc::from("headers"),
                        Value::Map(Rc::new(RefCell::new(headers))),
                    ),
                    (
                        Rc::from("body"),
                        Value::Bytes(Rc::from(body.into_boxed_slice())),
                    ),
                ],
            ))
        };
        let value = match operation.call_target.as_str() {
            "std.http::text" => {
                if arguments.len() != 2 {
                    return Err("std.http.text expects status and body".to_string());
                }
                let Value::Int(status) = arguments.remove(0) else {
                    return Err("std.http.text status must be int".to_string());
                };
                let Value::Str(body) = arguments.remove(0) else {
                    return Err("std.http.text body must be string".to_string());
                };
                response(
                    status,
                    "text/plain; charset=utf-8",
                    body.as_bytes().to_vec(),
                )?
            }
            "std.http::json" => {
                if arguments.len() != 2 {
                    return Err("std.http.json expects status and body".to_string());
                }
                let Value::Int(status) = arguments.remove(0) else {
                    return Err("std.http.json status must be int".to_string());
                };
                let body = arguments.remove(0);
                match topaz_value::value::json_stringify(&body, true) {
                    Ok(body) => Value::Ok(Rc::new(response(
                        status,
                        "application/json; charset=utf-8",
                        body.into_bytes(),
                    )?)),
                    Err(error) => Value::Err(Rc::new(Value::str(error))),
                }
            }
            "std.http::header" => {
                if arguments.len() != 2 {
                    return Err("std.http.header expects request and name".to_string());
                }
                let request = arguments.remove(0);
                let Value::Str(name) = arguments.remove(0) else {
                    return Err("std.http.header name must be string".to_string());
                };
                let Value::NominalRecord { fields, .. } = request else {
                    return Err("std.http.header request must be HttpRequest".to_string());
                };
                let Some(Value::Map(headers)) = fields
                    .iter()
                    .find(|(field, _)| field.as_ref() == "headers")
                    .map(|(_, value)| value)
                else {
                    return Err("std.http.header request headers are missing".to_string());
                };
                let key = Value::str(name.to_ascii_lowercase());
                match headers
                    .borrow()
                    .get_value(&key)
                    .map_err(|error| format!("{error:?}"))?
                {
                    Some(Value::Array(values)) => values
                        .borrow()
                        .first()
                        .cloned()
                        .map(|value| Value::Some(Rc::new(value)))
                        .unwrap_or(Value::None),
                    Some(_) => return Err("std.http.header values are not an array".to_string()),
                    None => Value::None,
                }
            }
            other => return Err(format!("unsupported self target host call `{other}`")),
        };
        Ok(Flow::Value(RuntimeValue::Data(value)))
    }

    pub(crate) fn call_static(
        &self,
        operation: &Operation,
        receiver: &str,
        mut arguments: Vec<Value>,
    ) -> Result<Flow, String> {
        let value =
            match (receiver, operation.call_method.as_str()) {
                ("Bytes", "encodeUtf8") => {
                    let Value::Str(value) = one(&mut arguments, operation)? else {
                        return Err("Bytes.encodeUtf8 requires a string".to_string());
                    };
                    Value::Bytes(Rc::from(value.as_bytes()))
                }
                ("Bytes", "fromArray") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_bytes_from_array(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("Bytes", "fromBase64") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_bytes_from_base64(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("ByteBuffer", "allocate") => {
                    if !(1..=2).contains(&arguments.len()) {
                        return Err(format!(
                            "ByteBuffer.allocate expects one or two arguments, found {}",
                            arguments.len()
                        ));
                    }
                    let length = arguments.remove(0);
                    let value = if arguments.is_empty() {
                        None
                    } else {
                        Some(arguments.remove(0))
                    };
                    builtin_byte_buffer_allocate(length, value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("ByteBuffer", "fromBytes") => {
                    builtin_byte_buffer_from_bytes(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("JSON", "parse") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_json_parse(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("JSON", "parseAs") => {
                    let Value::Str(value) = one(&mut arguments, operation)? else {
                        return Err("JSON.parseAs requires a string".to_string());
                    };
                    match topaz_value::value::json_parse(&value) {
                        Ok(value) => Value::Ok(Rc::new(json_to_value(&value)?)),
                        Err(error) => Value::Err(Rc::new(Value::str(format!("{error:?}")))),
                    }
                }
                ("JSON", "decode") => {
                    let Value::Json(value) = one(&mut arguments, operation)? else {
                        return Err("JSON.decode requires a JSONValue".to_string());
                    };
                    Value::Ok(Rc::new(json_to_value(&value)?))
                }
                ("JSON", "stringify") => {
                    let value = one(&mut arguments, operation)?;
                    match topaz_value::value::json_stringify(&value, true) {
                        Ok(value) => Value::Ok(Rc::new(Value::str(value))),
                        Err(error) => Value::Err(Rc::new(Value::str(error))),
                    }
                }
                ("CSV", "parse") => {
                    builtin_csv_parse(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("CSV", "parseWithHeader") => {
                    builtin_csv_parse_with_header(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("CSV", "stringify") => {
                    builtin_csv_stringify(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("CSV", "stringifyWithHeader") => {
                    if arguments.len() != 2 {
                        return Err("CSV.stringifyWithHeader expects rows and columns".to_string());
                    }
                    let rows = arguments.remove(0);
                    let columns = arguments.remove(0);
                    builtin_csv_stringify_with_header(rows, columns, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("TOML", "parse") => {
                    builtin_toml_parse(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("TOML", "stringify") => {
                    builtin_toml_stringify(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("TOML", "toJson") => {
                    builtin_toml_to_json(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("TOML", "fromJson") => {
                    builtin_toml_from_json(one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("Regex", "compile") => {
                    let pattern = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_regex_compile(pattern, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("Hash", "sha256") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_hash_sha256(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("Math", "abs") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_math_abs(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("Math", "isFinite") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_math_is_finite(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("Math", "parseFloat") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_math_parse_float(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("BigInt", "fromInt") => {
                    let value = one(&mut arguments, operation)?;
                    topaz_value::value::builtin_bigint_from_int(value, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("Map", "new") => {
                    if !arguments.is_empty() {
                        return Err(format!(
                            "Map.new expects no arguments, found {}",
                            arguments.len()
                        ));
                    }
                    topaz_value::value::builtin_map_new()
                }
                ("Set", "of") => topaz_value::value::builtin_set_of(arguments, span(operation))
                    .map_err(|error| format!("{error:?}"))?,
                ("fs", "readText") | ("FS", "readText") => {
                    let host = self.host.as_deref().ok_or_else(|| {
                        "FS.readText requires an emitted-product host".to_string()
                    })?;
                    builtin_fs_read_text(host, one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("fs", "writeText") | ("FS", "writeText") => {
                    if arguments.len() != 2 {
                        return Err("FS.writeText expects path and text".to_string());
                    }
                    let host = self.host.as_deref().ok_or_else(|| {
                        "FS.writeText requires an emitted-product host".to_string()
                    })?;
                    let path = arguments.remove(0);
                    let text = arguments.remove(0);
                    builtin_fs_write_text(host, path, text, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("fs", "readBytes") | ("FS", "readBytes") => {
                    let host = self.host.as_deref().ok_or_else(|| {
                        "FS.readBytes requires an emitted-product host".to_string()
                    })?;
                    builtin_fs_read_bytes(host, one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("fs", "writeBytes") | ("FS", "writeBytes") => {
                    if arguments.len() != 2 {
                        return Err("FS.writeBytes expects path and bytes".to_string());
                    }
                    let host = self.host.as_deref().ok_or_else(|| {
                        "FS.writeBytes requires an emitted-product host".to_string()
                    })?;
                    let path = arguments.remove(0);
                    let bytes = arguments.remove(0);
                    builtin_fs_write_bytes(host, path, bytes, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                ("fs", "list") | ("FS", "list") => {
                    let host = self
                        .host
                        .as_deref()
                        .ok_or_else(|| "FS.list requires an emitted-product host".to_string())?;
                    builtin_fs_list(host, one(&mut arguments, operation)?, span(operation))
                        .map_err(|error| format!("{error:?}"))?
                }
                _ => {
                    return Err(format!(
                        "unsupported Stage 1 static call `{receiver}.{}`",
                        operation.call_method
                    ));
                }
            };
        Ok(Flow::Value(RuntimeValue::Data(value)))
    }
}
