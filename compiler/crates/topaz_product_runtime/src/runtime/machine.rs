use super::{environment::*, model::*, nominal::*};
use crate::diagnostic::*;
use crate::program::model::*;
use crate::wire::*;
use crate::*;

pub(crate) struct Machine {
    pub(crate) program: Arc<Program>,
    pub(crate) globals: Environment,
    pub(crate) functions: Rc<BTreeMap<String, usize>>,
    pub(crate) receiver_methods: Rc<BTreeMap<(String, String), usize>>,
    pub(crate) protocol_methods: Rc<BTreeMap<(String, String, String), usize>>,
    pub(crate) nominals: Rc<NominalRegistry>,
    pub(crate) host: Option<Rc<dyn Host>>,
    pub(crate) stdin: Rc<str>,
    pub(crate) call_depth: usize,
    pub(crate) propagating: Option<RuntimeValue>,
    pub(crate) returning: Option<RuntimeValue>,
    pub(crate) loop_control: Option<Flow>,
    pub(crate) steps: Rc<Cell<u64>>,
    pub(crate) cooperative_remaining: Option<usize>,
}

// This ceiling covers the largest admitted compiler workload while bounding malformed images.
pub(crate) const STAGE1_EXECUTION_STEP_LIMIT: u64 = 2_000_000_000;

impl Machine {
    pub(crate) fn new(program: Arc<Program>) -> Self {
        Self {
            program,
            globals: EnvironmentFrame::root(),
            functions: Rc::new(BTreeMap::new()),
            receiver_methods: Rc::new(BTreeMap::new()),
            protocol_methods: Rc::new(BTreeMap::new()),
            nominals: Rc::new(NominalRegistry::default()),
            host: None,
            stdin: Rc::from(""),
            call_depth: 0,
            propagating: None,
            returning: None,
            loop_control: None,
            steps: Rc::new(Cell::new(0)),
            cooperative_remaining: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_facts(
        program: Arc<Program>,
        target_facts: Option<&str>,
    ) -> Result<Self, String> {
        Self::new_with_facts_and_input(program, target_facts, "")
    }

    #[cfg(test)]
    pub(crate) fn new_with_facts_and_input(
        program: Arc<Program>,
        target_facts: Option<&str>,
        stdin: &str,
    ) -> Result<Self, String> {
        Self::new_with_facts_host_and_input(program, target_facts, stdin, None)
    }

    pub(crate) fn new_with_facts_host_and_input(
        program: Arc<Program>,
        target_facts: Option<&str>,
        stdin: &str,
        host: Option<Rc<dyn Host>>,
    ) -> Result<Self, String> {
        let mut machine = Self::new(program);
        if let Some(target_facts) = target_facts {
            machine.nominals = Rc::new(NominalRegistry::parse(target_facts)?);
        }
        machine.host = host;
        machine.stdin = Rc::from(stdin);
        Ok(machine)
    }

    pub(crate) fn compile(mut self, request_bytes: &[u8]) -> Result<Vec<u8>, String> {
        self.register_functions()?;
        self.initialize_modules()?;
        let entry_module_count = self
            .program
            .modules
            .iter()
            .filter(|module| module.entry)
            .count();
        if entry_module_count != 1 {
            return Err(format!(
                "Stage 1 IR requires exactly one entry module, found {entry_module_count}"
            ));
        }
        let entry = self
            .functions
            .get("src.main::stage1Step")
            .or_else(|| self.functions.get("src.main::compilerStep"))
            .or_else(|| self.functions.get("src.main::compileStep"))
            .copied()
            .ok_or_else(|| "Stage 1 compiler entry function is missing".to_string())?;
        let result = self.call_function(
            entry,
            vec![RuntimeValue::Data(Value::Bytes(Rc::from(request_bytes)))],
        )?;
        match data(result)? {
            Value::Ok(value) => match &*value {
                Value::Bytes(bytes) => Ok(bytes.to_vec()),
                other => Err(format!(
                    "Stage 1 compiler returned Ok({}) instead of bytes",
                    other.kind()
                )),
            },
            Value::Err(error) => Err(format!(
                "Stage 1 compiler returned Err({})",
                topaz_value::value::render(&error)
            )),
            Value::Bytes(bytes) => Ok(bytes.to_vec()),
            other => Err(format!(
                "Stage 1 compiler returned `{}` instead of Result<Bytes, string>",
                other.kind()
            )),
        }
    }

    pub(crate) fn register_functions(&mut self) -> Result<(), String> {
        let mut implementation_methods = BTreeSet::new();
        let mut functions = BTreeMap::new();
        let mut receiver_methods = BTreeMap::new();
        let mut protocol_methods = BTreeMap::new();
        for operation in &self.program.operations {
            if operation.kind != "implementation" {
                continue;
            }
            let (protocol, target) = match operation.detail.split_once('<') {
                Some((protocol, target)) => {
                    let Some(target) = target.strip_suffix('>') else {
                        return Err(format!(
                            "self target implementation `{}` has malformed identity",
                            operation.detail
                        ));
                    };
                    if protocol.is_empty() || target.is_empty() || target.contains(['<', '>']) {
                        return Err(format!(
                            "self target implementation `{}` has malformed identity",
                            operation.detail
                        ));
                    }
                    (Some(protocol), target)
                }
                None if !operation.detail.is_empty() => (None, operation.detail.as_str()),
                None => {
                    return Err("self target implementation has no nominal identity".to_string());
                }
            };
            let nominal_identity = format!("{}::{target}", operation.module);
            if self.nominals.get(&nominal_identity).is_none() {
                return Err(format!(
                    "self target implementation `{}` has no nominal fact `{nominal_identity}`",
                    operation.detail
                ));
            }
            for method in operation.operands.iter().copied().filter(|index| {
                self.program.operations[*index].kind == "function"
                    && !self.program.operations[*index].binding_name.is_empty()
            }) {
                implementation_methods.insert(method);
                let definition = &self.program.operations[method];
                if let Some(protocol) = protocol {
                    let key = (
                        format!("builtin::{protocol}"),
                        nominal_identity.clone(),
                        definition.binding_name.clone(),
                    );
                    if protocol_methods.insert(key.clone(), method).is_some() {
                        return Err(format!(
                            "self target has duplicate protocol method `{}.{}` for `{}`",
                            protocol, key.2, nominal_identity
                        ));
                    }
                } else {
                    let key = (nominal_identity.clone(), definition.binding_name.clone());
                    if receiver_methods.insert(key.clone(), method).is_some() {
                        return Err(format!(
                            "self target has duplicate receiver method `{}.{}`",
                            key.0, key.1
                        ));
                    }
                }
            }
        }
        for (index, operation) in self.program.operations.iter().enumerate() {
            if operation.kind == "function"
                && !operation.binding_name.is_empty()
                && !implementation_methods.contains(&index)
            {
                let identity = format!("{}::{}", operation.module, operation.binding_name);
                if functions.insert(identity.clone(), index).is_some() {
                    return Err(format!(
                        "self target has duplicate function identity `{identity}`"
                    ));
                }
            }
        }
        self.functions = Rc::new(functions);
        self.receiver_methods = Rc::new(receiver_methods);
        self.protocol_methods = Rc::new(protocol_methods);
        Ok(())
    }

    pub(crate) fn initialize_modules(&mut self) -> Result<(), String> {
        for module in self.program.modules.clone() {
            let roots = module
                .operations
                .iter()
                .copied()
                .filter(|operation| self.program.operations[*operation].kind == "module")
                .collect::<Vec<_>>();
            let [root] = roots.as_slice() else {
                return Err(format!(
                    "module `{}` has {} initialization roots",
                    module.identity,
                    roots.len()
                ));
            };
            match self.eval(*root, self.globals.clone())? {
                Flow::Value(_) => {}
                _ => {
                    return Err(format!(
                        "module `{}` produced control flow during initialization",
                        module.identity
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn call_function(
        &mut self,
        operation_index: usize,
        arguments: Vec<RuntimeValue>,
    ) -> Result<RuntimeValue, String> {
        let call_span = span(&self.program.operations[operation_index]);
        run_local(self.call_function_with_environment_async(
            operation_index,
            self.globals.clone(),
            EvaluatedCallArguments::positional(arguments),
            call_span,
        ))
    }

    pub(crate) async fn call_function_with_arguments(
        &mut self,
        operation_index: usize,
        arguments: EvaluatedCallArguments,
        call_span: Span,
    ) -> Result<RuntimeValue, String> {
        self.call_function_with_environment_async(
            operation_index,
            self.globals.clone(),
            arguments,
            call_span,
        )
        .await
    }

    pub(crate) async fn call_function_with_environment_async(
        &mut self,
        operation_index: usize,
        parent_environment: Environment,
        arguments: EvaluatedCallArguments,
        call_span: Span,
    ) -> Result<RuntimeValue, String> {
        if self.call_depth >= CALL_DEPTH_LIMIT {
            return Err(runtime_diagnostic(recursion_fault(call_span)));
        }
        self.call_depth += 1;
        let result = self
            .call_function_inner(operation_index, parent_environment, arguments)
            .await;
        self.call_depth -= 1;
        result
    }
}
