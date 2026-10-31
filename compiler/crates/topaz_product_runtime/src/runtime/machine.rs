use super::{environment::*, model::*, nominal::*};
use crate::diagnostic::*;
use crate::program::model::*;
use crate::wire::*;
use crate::*;

pub(crate) struct Machine {
    pub(crate) program: Arc<Program>,
    pub(crate) globals: Environment,
    pub(crate) module_environments: Rc<BTreeMap<String, Environment>>,
    pub(crate) functions: Rc<BTreeMap<String, usize>>,
    /// Receiver methods keyed by nominal identity, then by method name, so a
    /// dispatch looks its method up from borrowed names.
    pub(crate) receiver_methods: Rc<BTreeMap<String, BTreeMap<String, usize>>>,
    /// Protocol methods keyed by protocol, then nominal identity, then method
    /// name, on the same borrowed-name principle.
    pub(crate) protocol_methods: Rc<BTreeMap<String, BTreeMap<String, BTreeMap<String, usize>>>>,
    pub(crate) nominals: Arc<NominalRegistry>,
    /// The entry-point judgement carried by this program's target facts, absent
    /// when the caller supplied none. The runtime reads it; it never decides it.
    pub(crate) explicit_main: Option<bool>,
    pub(crate) host: Option<Rc<dyn Host>>,
    pub(crate) stdin: Option<Rc<str>>,
    pub(crate) call_depth: usize,
    pub(crate) propagating: Option<RuntimeValue>,
    pub(crate) returning: Option<RuntimeValue>,
    pub(crate) loop_control: Option<Flow>,
    pub(crate) steps: Rc<Cell<u64>>,
    pub(crate) cooperative_remaining: Option<usize>,
    /// Literal values decoded from the wire text, established on first use and
    /// indexed by operation.
    pub(crate) literals: RefCell<Vec<Option<Value>>>,
    /// Parameter, default, and body layout of each called function, established
    /// on first call and indexed by its operation.
    pub(crate) layouts: RefCell<Vec<Option<Rc<FunctionLayout>>>>,
    /// Set once module initialization has defined every global, after which a
    /// reference that no enclosing frame binds always reaches the same global.
    pub(crate) globals_settled: Cell<bool>,
    /// The payload list every payload-less enum value carries.
    pub(crate) empty_payloads: Rc<[Value]>,
    /// The record declaration a record literal builds, established on first
    /// execution and indexed by its operation.
    pub(crate) record_sites: RefCell<Vec<Option<Option<Rc<RecordShape>>>>>,
    /// Case layout of a match, established on first execution and indexed by
    /// its operation.
    pub(crate) match_arms: RefCell<BTreeMap<usize, Rc<MatchCases>>>,
    /// Static dispatch of a call site, established on first execution and
    /// indexed by its operation.
    pub(crate) call_routes: RefCell<Vec<Option<CallRoute>>>,
    /// Nominal names shared by every value that carries them.
    pub(crate) names: RefCell<BTreeMap<String, Rc<str>>>,
    /// Declaration-ordered field names of a nominal record, shared by every
    /// value the program builds from that declaration.
    pub(crate) record_shapes: RefCell<BTreeMap<String, Rc<RecordShape>>>,
    /// Operation index by wire id, established on first lookup.
    pub(crate) operation_indexes: RefCell<BTreeMap<String, usize>>,
    /// Settled global resolution of a reference, indexed by its operation. A
    /// reference consults its enclosing frames first, so a local binding of the
    /// same name still shadows the entry.
    pub(crate) global_references: RefCell<Vec<Option<Rc<GlobalReference>>>>,
}

/// The cases of one match that its operand labels describe completely, and the
/// operand the walk resumes from for anything they leave incomplete.
pub(crate) struct MatchCases {
    pub(crate) arms: Vec<MatchArm>,
    pub(crate) remainder: usize,
}

/// One case of a match, as its operand labels declare it.
pub(crate) struct MatchArm {
    pub(crate) pattern: usize,
    pub(crate) guard: Option<usize>,
    pub(crate) body: usize,
    pub(crate) returns: bool,
}

/// How a call site reaches its callee, as far as the operation decides it.
#[derive(Clone, Copy)]
pub(crate) enum CallRoute {
    /// The call target names a program function and nothing else intervenes.
    Function(usize),
    /// The call target is one of the intrinsics the runtime answers directly,
    /// or the callee is a value the call has to evaluate.
    Dispatched,
}

/// One nominal record declaration as values are built from it.
pub(crate) struct RecordShape {
    pub(crate) name: Rc<str>,
    pub(crate) identity: Rc<str>,
    pub(crate) members: Vec<(Rc<str>, Option<Rc<str>>)>,
}

/// What a reference resolves to once no enclosing frame binds its name.
pub(crate) enum GlobalReference {
    Slot(Slot),
    Function(usize),
    Builtin(Builtin),
    Type(String),
    Module(String),
}

/// The call-shape of one function operation.
pub(crate) struct FunctionLayout {
    pub(crate) parameters: Vec<usize>,
    pub(crate) fixed_count: usize,
    pub(crate) variadic: Option<usize>,
    pub(crate) projected_defaults: BTreeMap<usize, usize>,
    pub(crate) body: Option<usize>,
}

// Bound malformed images while keeping more than twice the largest admitted workload:
// the compiler image compiling itself on the program-image route, measured at about
// 2.125e9 steps in one run on 2026-10-27 (2e27d9030 with compact image and JSON schema fixes).
pub(crate) const STAGE1_EXECUTION_STEP_LIMIT: u64 = 5_000_000_000;

impl Machine {
    pub(crate) fn new(program: Arc<Program>) -> Self {
        let globals = EnvironmentFrame::root();
        let module_environments = program
            .modules
            .iter()
            .map(|module| {
                (
                    module.identity.clone(),
                    EnvironmentFrame::child(globals.clone()),
                )
            })
            .collect();
        Self {
            program,
            globals,
            module_environments: Rc::new(module_environments),
            functions: Rc::new(BTreeMap::new()),
            receiver_methods: Rc::new(BTreeMap::new()),
            protocol_methods: Rc::new(BTreeMap::new()),
            nominals: Arc::new(NominalRegistry::default()),
            explicit_main: None,
            host: None,
            stdin: Some(Rc::from("")),
            call_depth: 0,
            propagating: None,
            returning: None,
            loop_control: None,
            steps: Rc::new(Cell::new(0)),
            cooperative_remaining: None,
            literals: RefCell::new(Vec::new()),
            layouts: RefCell::new(Vec::new()),
            empty_payloads: Rc::from([]),
            record_sites: RefCell::new(Vec::new()),
            match_arms: RefCell::new(BTreeMap::new()),
            call_routes: RefCell::new(Vec::new()),
            names: RefCell::new(BTreeMap::new()),
            record_shapes: RefCell::new(BTreeMap::new()),
            operation_indexes: RefCell::new(BTreeMap::new()),
            globals_settled: Cell::new(false),
            global_references: RefCell::new(Vec::new()),
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

    pub(crate) fn new_with_facts_host_and_input<'a>(
        program: Arc<Program>,
        target_facts: Option<&str>,
        stdin: impl Into<Option<&'a str>>,
        host: Option<Rc<dyn Host>>,
    ) -> Result<Self, String> {
        let mut machine = Self::new(program);
        if let Some(target_facts) = target_facts {
            let facts = parse_target_facts(target_facts)?;
            machine.nominals = Arc::new(facts.nominals);
            machine.explicit_main = facts.explicit_main;
        }
        machine.host = host;
        machine.stdin = stdin.into().map(Rc::from);
        Ok(machine)
    }

    pub(crate) fn input(&self) -> Rc<str> {
        // Host::input owns the per-run cache, just as on the interpreter route.
        // A supplied snapshot (even empty) takes precedence over host input.
        self.stdin.clone().unwrap_or_else(|| {
            Rc::from(
                self.host
                    .as_ref()
                    .map_or_else(String::new, |host| host.input()),
            )
        })
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

    /// Returns the shared handle for one nominal name, so the values that
    /// carry it hold one copy of the text between them.
    pub(crate) fn interned(&self, text: &str) -> Rc<str> {
        if let Some(name) = self.names.borrow().get(text) {
            return name.clone();
        }
        let name: Rc<str> = Rc::from(text);
        self.names
            .borrow_mut()
            .insert(text.to_string(), name.clone());
        name
    }

    /// Returns the shared field-name shape of one nominal record declaration.
    pub(crate) fn record_shape(&self, fact: &NominalFact) -> Rc<RecordShape> {
        if let Some(shape) = self.record_shapes.borrow().get(&fact.identity) {
            return shape.clone();
        }
        let shape = Rc::new(RecordShape {
            name: self.interned(&fact.name),
            identity: self.interned(&fact.identity),
            members: fact
                .members
                .iter()
                .map(|member| {
                    (
                        self.interned(&member.name),
                        member
                            .default_operation_id
                            .as_deref()
                            .map(|identity| self.interned(identity)),
                    )
                })
                .collect(),
        });
        self.record_shapes
            .borrow_mut()
            .insert(fact.identity.clone(), shape.clone());
        shape
    }

    /// Returns the operation carrying one wire id.
    pub(crate) fn operation_index(&self, operation_id: &str) -> Option<usize> {
        if let Some(index) = self.operation_indexes.borrow().get(operation_id) {
            return Some(*index);
        }
        let mut indexes = self.operation_indexes.borrow_mut();
        if indexes.is_empty() {
            for (index, operation) in self.program.operations.iter().enumerate() {
                indexes.insert(operation.id.clone(), index);
            }
        }
        indexes.get(operation_id).copied()
    }

    pub(crate) fn register_functions(&mut self) -> Result<(), String> {
        let local_functions = self
            .program
            .operations
            .iter()
            .filter(|parent| {
                !matches!(parent.kind.as_str(), "module" | "export" | "implementation")
            })
            .flat_map(|parent| parent.operands.iter().copied())
            .filter(|index| self.program.kinds[*index] == OpKind::Function)
            .collect::<BTreeSet<_>>();
        let mut implementation_methods = BTreeSet::new();
        let mut functions = BTreeMap::new();
        let mut receiver_methods: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        let mut protocol_methods: BTreeMap<String, BTreeMap<String, BTreeMap<String, usize>>> =
            BTreeMap::new();
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
                    let replaced = protocol_methods
                        .entry(format!("builtin::{protocol}"))
                        .or_default()
                        .entry(nominal_identity.clone())
                        .or_default()
                        .insert(definition.binding_name.clone(), method);
                    if replaced.is_some() {
                        return Err(format!(
                            "self target has duplicate protocol method `{}.{}` for `{}`",
                            protocol, definition.binding_name, nominal_identity
                        ));
                    }
                } else {
                    let replaced = receiver_methods
                        .entry(nominal_identity.clone())
                        .or_default()
                        .insert(definition.binding_name.clone(), method);
                    if replaced.is_some() {
                        return Err(format!(
                            "self target has duplicate receiver method `{}.{}`",
                            nominal_identity, definition.binding_name
                        ));
                    }
                }
            }
        }
        for (index, operation) in self.program.operations.iter().enumerate() {
            if operation.kind == "function"
                && !operation.binding_name.is_empty()
                && !implementation_methods.contains(&index)
                && !local_functions.contains(&index)
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

    pub(crate) fn initialize_modules(&mut self) -> Result<RuntimeValue, String> {
        let mut entry_value = unit();
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
            let environment = self
                .module_environments
                .get(&module.identity)
                .cloned()
                .unwrap_or_else(|| self.globals.clone());
            match self.eval(*root, environment)? {
                Flow::Value(value) => {
                    if module.entry {
                        entry_value = value;
                    }
                }
                _ => {
                    return Err(format!(
                        "module `{}` produced control flow during initialization",
                        module.identity
                    ));
                }
            }
        }
        self.globals_settled.set(true);
        Ok(entry_value)
    }

    pub(crate) fn call_function(
        &mut self,
        operation_index: usize,
        arguments: Vec<RuntimeValue>,
    ) -> Result<RuntimeValue, String> {
        let call_span = span(&self.program.operations[operation_index]);
        run_local(self.call_function_with_environment_async(
            operation_index,
            self.function_environment(operation_index),
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
            self.function_environment(operation_index),
            arguments,
            call_span,
        )
        .await
    }

    pub(crate) fn function_environment(&self, operation: usize) -> Environment {
        self.module_environments
            .get(&self.program.operations[operation].module)
            .cloned()
            .unwrap_or_else(|| self.globals.clone())
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
