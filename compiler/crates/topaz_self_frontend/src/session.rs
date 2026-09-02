use crate::*;

/// Checked embedded compiler package retained across exchange invocations.
pub struct FrontEndSession {
    unit: ResolveOutput,
}

impl FrontEndSession {
    pub fn new() -> Result<Self, String> {
        let unit = resolve_embedded()?;
        let modules = unit
            .modules
            .iter()
            .map(|module| topaz_check::UnitModule {
                identity: module.identity.clone(),
                is_entry: module.is_entry,
                is_extern: module.is_extern,
                is_generated_std: module.is_generated_std,
                extern_replay_error: module.extern_replay_error.clone(),
                src: unit.map.file(module.file).src(),
                program: &module.program,
            })
            .collect::<Vec<_>>();
        let checked = topaz_check::check_unit_typed(&modules);
        if !checked.diagnostics.is_empty() {
            return Err(format!(
                "embedded front end did not type-check: {:?}",
                checked.diagnostics
            ));
        }
        Ok(Self { unit })
    }

    fn invoke_export(&self, name: &str, request: &[u8]) -> Result<Vec<u8>, String> {
        let host = TestHost::new();
        match Machine::run_unit_export(
            &self.unit,
            &host,
            name,
            vec![Value::Bytes(Rc::from(request))],
        )
        .map_err(|error| format!("{}: {}", error.code, error.message))?
        {
            Value::Ok(value) => match value.as_ref() {
                Value::Bytes(bytes) => Ok(bytes.to_vec()),
                other => Err(format!(
                    "{name} returned Ok({}) instead of Ok(Bytes)",
                    topaz_interp::render(other)
                )),
            },
            Value::Err(value) => Err(topaz_interp::render(&value)),
            other => Err(format!(
                "{name} returned {} instead of Result<Bytes, string>",
                topaz_interp::render(&other)
            )),
        }
    }

    /// Invokes the pure `frontEndStep` export with exact request bytes.
    pub fn invoke(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.invoke_export("frontEndStep", request)
    }

    /// Invokes the pure `compilerStep` export with exact request bytes.
    pub fn invoke_stage1(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.invoke_export("compilerStep", request)
    }
}

/// Runs one front-end exchange against a freshly checked embedded package.
pub fn invoke_exchange(request: &[u8]) -> Result<Vec<u8>, String> {
    FrontEndSession::new()?.invoke(request)
}
