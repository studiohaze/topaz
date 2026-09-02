use super::{environment::*, machine::*, model::*};
use crate::diagnostic::*;
use crate::program::model::*;
use crate::wire::*;
use crate::*;

impl Machine {
    pub(crate) async fn eval_if(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let condition = data(
            self.eval_value(operation.operands[0], environment.clone())
                .await?,
        )?;
        let Value::Bool(condition) = condition else {
            return Err("Stage 1 if condition is not bool".to_string());
        };
        if condition {
            self.eval_async(operation.operands[1], environment).await
        } else if let Some(otherwise) = operation.operands.get(2) {
            self.eval_async(*otherwise, environment).await
        } else {
            Ok(Flow::Value(unit()))
        }
    }

    pub(crate) async fn eval_assignment(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let target = self.program.operations[operation.operands[0]].clone();
        match target.kind.as_str() {
            "expression/identifier" => {
                let keys = [target.reference_identity.as_str(), target.detail.as_str()];
                let slot = keys
                    .into_iter()
                    .filter(|key| !key.is_empty())
                    .find_map(|key| environment.slot(key))
                    .or_else(|| {
                        keys.into_iter()
                            .filter(|key| !key.is_empty())
                            .find_map(|key| self.globals.slot(key))
                    })
                    .ok_or_else(|| {
                        format!(
                            "assignment target `{}` is not bound",
                            target.reference_identity
                        )
                    })?;
                let right = self
                    .eval_value(operation.operands[1], environment.clone())
                    .await?;
                let value = assignment_value(
                    &operation.detail,
                    Some(slot.borrow().clone()),
                    right,
                    operation,
                )?;
                *slot.borrow_mut() = value.clone();
                Ok(Flow::Value(value))
            }
            "expression/index" => {
                let base = data(
                    self.eval_value(target.operands[0], environment.clone())
                        .await?,
                )?;
                let index = data(
                    self.eval_value(target.operands[1], environment.clone())
                        .await?,
                )?;
                let (store, index) = topaz_value::value::index_slot(&base, &index, span(operation))
                    .map_err(|error| format!("{error:?}"))?;
                let right = self.eval_value(operation.operands[1], environment).await?;
                let value = assignment_value(
                    &operation.detail,
                    Some(RuntimeValue::Data(store.borrow()[index].clone())),
                    right,
                    operation,
                )?;
                store.borrow_mut()[index] = data(value.clone())?;
                Ok(Flow::Value(value))
            }
            other => Err(format!("unsupported Stage 1 assignment target `{other}`")),
        }
    }

    pub(crate) fn cooperative_arm(&self) -> Self {
        Self {
            program: self.program.clone(),
            globals: self.globals.clone(),
            functions: self.functions.clone(),
            receiver_methods: self.receiver_methods.clone(),
            protocol_methods: self.protocol_methods.clone(),
            nominals: self.nominals.clone(),
            host: self.host.clone(),
            stdin: self.stdin.clone(),
            call_depth: self.call_depth,
            propagating: None,
            returning: None,
            loop_control: None,
            steps: self.steps.clone(),
            cooperative_remaining: Some(CONCURRENT_STEP_QUANTUM),
        }
    }

    pub(crate) async fn eval_concurrent(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let mut timeout = None;
        let mut else_block = None;
        let mut arms = BTreeMap::new();
        let mut names = BTreeSet::new();
        for (operand, label) in operation.operands.iter().zip(&operation.operand_labels) {
            if label.starts_with("timeout:") {
                if timeout.replace(*operand).is_some() {
                    return Err(format!(
                        "{} has more than one concurrent timeout",
                        operation.id
                    ));
                }
                continue;
            }
            if label.starts_with("else:") {
                if else_block.replace(*operand).is_some() {
                    return Err(format!(
                        "{} has more than one concurrent else",
                        operation.id
                    ));
                }
                continue;
            }
            let (index, name) = concurrent_arm_label(label).ok_or_else(|| {
                format!(
                    "{} has unrecognized concurrent operand `{label}`",
                    operation.id
                )
            })?;
            if !names.insert(name.to_string()) {
                return Err(format!(
                    "{} duplicates concurrent arm `{name}`",
                    operation.id
                ));
            }
            if arms.insert(index, (name.to_string(), *operand)).is_some() {
                return Err(format!(
                    "{} duplicates concurrent arm index {index}",
                    operation.id
                ));
            }
        }
        if arms.is_empty() {
            return Err(format!("{} has no concurrent arms", operation.id));
        }
        for (expected, actual) in arms.keys().copied().enumerate() {
            if expected != actual {
                return Err(format!(
                    "{} has non-contiguous concurrent arm index {actual}",
                    operation.id
                ));
            }
        }
        let timeout_route = match (timeout, else_block) {
            (None, None) => None,
            (Some(timeout), Some(else_block)) => {
                let timeout = &self.program.operations[timeout];
                if timeout.kind != "expression/duration" {
                    return Err(runtime_diagnostic(topaz_value::fault(
                        topaz_value::codes::GUARD_TYPE,
                        "`concurrent` timeout takes a duration literal (§15)",
                        span(timeout),
                    )));
                }
                let milliseconds = topaz_value::parse_duration_milliseconds(&timeout.detail)
                    .ok_or_else(|| {
                        runtime_diagnostic(topaz_value::fault(
                            topaz_value::codes::GUARD_TYPE,
                            "`concurrent` timeout duration must fit in u64 milliseconds (§15)",
                            span(timeout),
                        ))
                    })?;
                let host = self.host.as_deref().ok_or_else(|| {
                    "concurrent timeout requires an admitted product host".to_string()
                })?;
                Some((host.now_millis().saturating_add(milliseconds), else_block))
            }
            _ => {
                return Err(format!(
                    "{} must carry concurrent timeout and else together",
                    operation.id
                ));
            }
        };

        let mut pending = arms
            .into_values()
            .map(|(name, arm_operation)| {
                let mut arm = self.cooperative_arm();
                let arm_environment = environment.clone();
                let future: LocalFuture<'_, Result<RuntimeValue, String>> = Box::pin(async move {
                    match arm.eval_async(arm_operation, arm_environment).await {
                        Ok(Flow::Value(value)) => Ok(value),
                        Ok(Flow::Return(_)) => Err(runtime_diagnostic(topaz_value::fault(
                            topaz_value::codes::GUARD_TYPE,
                            "`return` outside a function",
                            span(&arm.program.operations[arm_operation]),
                        ))),
                        Err(error) if error == RETURN_SIGNAL || error == PROPAGATE_SIGNAL => {
                            Err(runtime_diagnostic(topaz_value::fault(
                                topaz_value::codes::GUARD_TYPE,
                                "`return` outside a function",
                                span(&arm.program.operations[arm_operation]),
                            )))
                        }
                        Ok(Flow::Break { .. }) => Err(runtime_diagnostic(topaz_value::fault(
                            topaz_value::codes::GUARD_TYPE,
                            "`break` outside a loop",
                            span(&arm.program.operations[arm_operation]),
                        ))),
                        Ok(Flow::Continue { .. }) => Err(runtime_diagnostic(topaz_value::fault(
                            topaz_value::codes::GUARD_TYPE,
                            "`continue` outside a loop",
                            span(&arm.program.operations[arm_operation]),
                        ))),
                        Err(error) if error == BREAK_SIGNAL || error == CONTINUE_SIGNAL => {
                            let loop_control = arm.loop_control.take();
                            let keyword = if matches!(loop_control, Some(Flow::Break { .. })) {
                                "break"
                            } else {
                                "continue"
                            };
                            Err(runtime_diagnostic(topaz_value::fault(
                                topaz_value::codes::GUARD_TYPE,
                                format!("`{keyword}` outside a loop"),
                                span(&arm.program.operations[arm_operation]),
                            )))
                        }
                        Err(error) => Err(error),
                    }
                });
                (name, future)
            })
            .collect::<Vec<_>>();
        let mut done = BTreeMap::new();
        let mut context = Context::from_waker(Waker::noop());
        loop {
            let mut index = 0;
            while index < pending.len() {
                let outcome = pending[index].1.as_mut().poll(&mut context);
                let expired = timeout_route.is_some_and(|(deadline, _)| {
                    self.host
                        .as_deref()
                        .is_some_and(|host| host.now_millis() >= deadline)
                });
                match outcome {
                    Poll::Ready(Ok(value)) => {
                        let (name, _) = pending.remove(index);
                        done.insert(name, data(value)?);
                    }
                    Poll::Ready(Err(error)) if !expired => return Err(error),
                    Poll::Ready(Err(_)) | Poll::Pending => index += 1,
                }
                if expired && !pending.is_empty() {
                    pending.clear();
                    done.clear();
                    let else_block = timeout_route
                        .map(|(_, else_block)| else_block)
                        .ok_or_else(|| format!("{} lost its concurrent else", operation.id))?;
                    return self.eval_async(else_block, environment).await;
                }
            }
            if pending.is_empty() {
                return Ok(Flow::Value(RuntimeValue::Data(Value::record(done))));
            }
            YieldOnce(false).await;
        }
    }

    pub(crate) async fn eval_match(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let scrutinee = self
            .eval_value(operation.operands[0], environment.clone())
            .await?;
        let mut index = 1;
        while index < operation.operands.len() {
            let pattern_label = operation
                .operand_labels
                .get(index)
                .ok_or_else(|| format!("{} omitted a match-case operand label", operation.id))?;
            if !pattern_label.contains("/pattern:") {
                return Err(format!(
                    "{} expected a match-case pattern at operand {index}, found `{pattern_label}`",
                    operation.id
                ));
            }
            let case_environment = EnvironmentFrame::child(environment.clone());
            let pattern_matches = self
                .match_pattern(
                    operation.operands[index],
                    scrutinee.clone(),
                    case_environment.clone(),
                )
                .await?;
            index += 1;

            let guard_matches = if operation
                .operand_labels
                .get(index)
                .is_some_and(|label| label.contains("/guard:"))
            {
                let guard_matches = if pattern_matches {
                    let guard = data(
                        self.eval_value(operation.operands[index], case_environment.clone())
                            .await?,
                    )?;
                    let Value::Bool(guard) = guard else {
                        return Err(format!("{} match guard is not bool", operation.id));
                    };
                    guard
                } else {
                    false
                };
                index += 1;
                guard_matches
            } else {
                pattern_matches
            };

            let body_label = operation
                .operand_labels
                .get(index)
                .ok_or_else(|| format!("{} omitted a match-case body", operation.id))?;
            if !body_label.contains("/body:") {
                return Err(format!(
                    "{} expected a match-case body at operand {index}, found `{body_label}`",
                    operation.id
                ));
            }
            let body = operation.operands[index];
            index += 1;

            if guard_matches {
                let flow = self.eval_async(body, case_environment).await?;
                if body_label.contains("match-case-return") {
                    return match flow {
                        Flow::Value(value) => Ok(Flow::Return(value)),
                        flow => Ok(flow),
                    };
                }
                return Ok(flow);
            }
        }
        Err(format!(
            "{} has no matching case for {}",
            operation.id,
            runtime_value_kind(&scrutinee)
        ))
    }

    pub(crate) async fn eval_for(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let iterator = data(
            self.eval_value(operation.operands[0], environment.clone())
                .await?,
        )?;
        let items = for_items(&iterator, span(operation)).map_err(runtime_diagnostic)?;
        let mut values = Vec::with_capacity(items.len());
        let statement_position =
            operation.semantic_type.is_empty() || operation.semantic_type == "unit";
        for item in items {
            let loop_environment = EnvironmentFrame::child(environment.clone());
            if !self
                .match_pattern(
                    operation.operands[1],
                    RuntimeValue::Data(item),
                    loop_environment.clone(),
                )
                .await?
            {
                return Err(format!("{} iterator pattern did not match", operation.id));
            }
            let outcome = self
                .eval_async(operation.operands[2], loop_environment)
                .await;
            match self.recover_loop_control(outcome)? {
                Flow::Value(value) => {
                    if !statement_position {
                        values.push(data(value)?);
                    }
                }
                Flow::Break { target, value } => {
                    if !Self::targets_loop(operation, &target) {
                        return Ok(Flow::Break { target, value });
                    }
                    if !statement_position {
                        return Err(
                            "`break`/`continue` cannot target a value-collecting `for` (§5)"
                                .to_string(),
                        );
                    }
                    return Ok(Flow::Value(unit()));
                }
                Flow::Continue { target } => {
                    if !Self::targets_loop(operation, &target) {
                        return Ok(Flow::Continue { target });
                    }
                    if !statement_position {
                        return Err(
                            "`break`/`continue` cannot target a value-collecting `for` (§5)"
                                .to_string(),
                        );
                    }
                }
                flow @ Flow::Return(_) => return Ok(flow),
            }
        }
        if statement_position {
            Ok(Flow::Value(unit()))
        } else {
            Ok(Flow::Value(RuntimeValue::Data(Value::array(values))))
        }
    }

    pub(crate) async fn eval_while(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        loop {
            let condition = data(
                self.eval_value(operation.operands[0], environment.clone())
                    .await?,
            )?;
            let Value::Bool(condition) = condition else {
                return Err("Stage 1 while condition is not bool".to_string());
            };
            if !condition {
                break;
            }
            let outcome = self
                .eval_async(
                    operation.operands[1],
                    EnvironmentFrame::child(environment.clone()),
                )
                .await;
            match self.recover_loop_control(outcome)? {
                Flow::Value(_) => {}
                Flow::Continue { target } => {
                    if !Self::targets_loop(operation, &target) {
                        return Ok(Flow::Continue { target });
                    }
                }
                Flow::Break { target, value } => {
                    if !Self::targets_loop(operation, &target) {
                        return Ok(Flow::Break { target, value });
                    }
                    break;
                }
                flow @ Flow::Return(_) => return Ok(flow),
            }
        }
        Ok(Flow::Value(unit()))
    }

    pub(crate) async fn eval_loop(
        &mut self,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let body = *operation
            .operands
            .first()
            .ok_or_else(|| format!("{} has no loop body", operation.id))?;
        loop {
            let outcome = self
                .eval_async(body, EnvironmentFrame::child(environment.clone()))
                .await;
            match self.recover_loop_control(outcome)? {
                Flow::Value(_) => {}
                Flow::Continue { target } => {
                    if !Self::targets_loop(operation, &target) {
                        return Ok(Flow::Continue { target });
                    }
                }
                Flow::Break { target, value } => {
                    if !Self::targets_loop(operation, &target) {
                        return Ok(Flow::Break { target, value });
                    }
                    return Ok(Flow::Value(value));
                }
                flow @ Flow::Return(_) => return Ok(flow),
            }
        }
    }

    pub(crate) fn targets_loop(operation: &Operation, target: &str) -> bool {
        target.is_empty()
            || target == operation.id
            || (!operation.control_target.is_empty() && target == operation.control_target)
    }

    pub(crate) fn recover_loop_control(
        &mut self,
        outcome: Result<Flow, String>,
    ) -> Result<Flow, String> {
        match outcome {
            Err(error) if error == BREAK_SIGNAL || error == CONTINUE_SIGNAL => self
                .loop_control
                .take()
                .ok_or_else(|| "Stage 1 loop-control value is missing".to_string()),
            other => other,
        }
    }
}
