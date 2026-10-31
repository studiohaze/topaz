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
                let slot = environment
                    .slot_any(&keys)
                    .or_else(|| self.globals.slot_any(&keys))
                    .ok_or_else(|| {
                        format!(
                            "assignment target `{}` is not bound",
                            target.reference_identity
                        )
                    })?;
                let old = slot.borrow().clone();
                if operation.detail == "coalesce"
                    && !matches!(old, RuntimeValue::Data(Value::None | Value::Null))
                {
                    return Ok(Flow::Value(old));
                }
                let right = self
                    .eval_value(operation.operands[1], environment.clone())
                    .await?;
                let value = assignment_value(&operation.detail, Some(old), right, operation)?;
                *slot.borrow_mut() = value.clone();
                Ok(Flow::Value(value))
            }
            "expression/index" => {
                let base = data(
                    self.eval_value(target.operands[0], environment.clone())
                        .await?,
                )?;
                let index_value = data(
                    self.eval_value(target.operands[1], environment.clone())
                        .await?,
                )?;
                let (store, index) =
                    topaz_value::value::index_slot(&base, &index_value, span(operation))
                        .map_err(runtime_diagnostic)?;
                let old = RuntimeValue::Data(store.borrow()[index].clone());
                if operation.detail == "coalesce"
                    && !matches!(old, RuntimeValue::Data(Value::None | Value::Null))
                {
                    return Ok(Flow::Value(old));
                }
                let right = self.eval_value(operation.operands[1], environment).await?;
                let value = assignment_value(&operation.detail, Some(old), right, operation)?;
                let (store, index) =
                    topaz_value::value::index_slot(&base, &index_value, span(operation))
                        .map_err(runtime_diagnostic)?;
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
            module_environments: self.module_environments.clone(),
            functions: self.functions.clone(),
            receiver_methods: self.receiver_methods.clone(),
            protocol_methods: self.protocol_methods.clone(),
            nominals: self.nominals.clone(),
            explicit_main: self.explicit_main,
            host: self.host.clone(),
            stdin: self.stdin.clone(),
            call_depth: self.call_depth,
            propagating: None,
            returning: None,
            loop_control: None,
            steps: self.steps.clone(),
            cooperative_remaining: Some(CONCURRENT_STEP_QUANTUM),
            literals: RefCell::new(Vec::new()),
            layouts: RefCell::new(Vec::new()),
            empty_payloads: self.empty_payloads.clone(),
            record_sites: RefCell::new(Vec::new()),
            match_arms: RefCell::new(BTreeMap::new()),
            call_routes: RefCell::new(Vec::new()),
            names: RefCell::new(BTreeMap::new()),
            record_shapes: RefCell::new(BTreeMap::new()),
            operation_indexes: RefCell::new(BTreeMap::new()),
            globals_settled: Cell::new(self.globals_settled.get()),
            global_references: RefCell::new(Vec::new()),
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
                let timeout_index = timeout;
                let timeout = &self.program.operations[timeout_index];
                if timeout.kind != "expression/duration" {
                    return Err(runtime_diagnostic(topaz_value::fault(
                        topaz_value::codes::GUARD_TYPE,
                        "`concurrent` timeout takes a duration literal (§15)",
                        span(timeout),
                    )));
                }
                let milliseconds = match self.program.literal_facts[timeout_index].as_ref() {
                    Some(Ok(topaz_value::IrLiteral::Duration(value))) => Some(*value),
                    _ => None,
                }
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

    /// Reads a match's cases from its operand labels once per site.
    ///
    /// Only the cases the labels describe completely are read ahead. A case the
    /// labels leave incomplete is left to the operand walk below, which reaches
    /// it only when no earlier case matched and reports it exactly as before.
    fn match_cases(&self, operation_index: usize, operation: &Operation) -> Rc<MatchCases> {
        if let Some(cases) = self.match_arms.borrow().get(&operation_index) {
            return cases.clone();
        }
        let mut arms = Vec::new();
        let mut index = 1;
        while index < operation.operands.len() {
            let Some(pattern_label) = operation.operand_labels.get(index) else {
                break;
            };
            if !pattern_label.contains("/pattern:") {
                break;
            }
            let pattern = operation.operands[index];
            let mut next = index + 1;
            let guard = if operation
                .operand_labels
                .get(next)
                .is_some_and(|label| label.contains("/guard:"))
            {
                let guard = operation.operands[next];
                next += 1;
                Some(guard)
            } else {
                None
            };
            let Some(body_label) = operation.operand_labels.get(next) else {
                break;
            };
            if !body_label.contains("/body:") {
                break;
            }
            arms.push(MatchArm {
                pattern,
                guard,
                body: operation.operands[next],
                returns: body_label.contains("match-case-return"),
            });
            index = next + 1;
        }
        let cases = Rc::new(MatchCases {
            arms,
            remainder: index,
        });
        self.match_arms
            .borrow_mut()
            .insert(operation_index, cases.clone());
        cases
    }

    pub(crate) async fn eval_match(
        &mut self,
        operation_index: usize,
        operation: &Operation,
        environment: Environment,
    ) -> Result<Flow, String> {
        let scrutinee = self
            .eval_value(operation.operands[0], environment.clone())
            .await?;
        let cases = self.match_cases(operation_index, operation);
        let mut spare: Option<Environment> = None;
        for arm in cases.arms.iter() {
            // A case that does not match leaves nothing behind, so its frame is
            // reused when this loop is still its only owner.
            let case_environment = match spare.take() {
                Some(frame) if Rc::strong_count(&frame) == 1 => {
                    frame.reset();
                    frame
                }
                _ => EnvironmentFrame::child(environment.clone()),
            };
            let pattern_matches = self
                .match_pattern(arm.pattern, scrutinee.clone(), case_environment.clone())
                .await?;
            let guard_matches = match arm.guard {
                Some(guard) if pattern_matches => {
                    let guard = data(self.eval_value(guard, case_environment.clone()).await?)?;
                    let Value::Bool(guard) = guard else {
                        return Err(format!("{} match guard is not bool", operation.id));
                    };
                    guard
                }
                Some(_) => false,
                None => pattern_matches,
            };
            if guard_matches {
                let flow = self.eval_async(arm.body, case_environment).await?;
                if arm.returns {
                    return match flow {
                        Flow::Value(value) => Ok(Flow::Return(value)),
                        flow => Ok(flow),
                    };
                }
                return Ok(flow);
            }
            spare = Some(case_environment);
        }
        let mut index = cases.remainder;
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
