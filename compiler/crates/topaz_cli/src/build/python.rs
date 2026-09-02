use crate::*;

pub(super) fn install_python_build(
    destination: artifact::Destination,
    dir: &Path,
    entry: &str,
    version: LangVersion,
    generated: &str,
    compiler: artifact::CompilerProvenance,
) -> ExitCode {
    let plan = artifact::Plan {
        target: artifact::Target::Python,
        language_version: version,
        entry: logical_entry(entry),
        runtime_requirements: vec!["Python 3.11 or newer".into()],
        invocation: "python program.py".into(),
        compiler: Some(compiler),
        files: vec![
            artifact::File::text("program.py", python_program(generated, "build")),
            artifact::File::text("topaz_py_rt.py", topaz_emit_py::PY_RT),
        ],
    };
    match destination.commit(plan) {
        Ok(()) => {
            eprintln!(
                "topaz: wrote Python deployment bundle to `{}` (`program.py` + `topaz_py_rt.py`)",
                dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!(
                "topaz: could not install Python artifact in `{}`: {e}",
                dir.display()
            );
            ExitCode::FAILURE
        }
    }
}

pub(super) fn install_self_python_build(
    destination: artifact::Destination,
    dir: &Path,
    entry: &str,
    version: LangVersion,
    generated: &str,
    compiler: artifact::CompilerProvenance,
) -> ExitCode {
    let plan = artifact::Plan {
        target: artifact::Target::Python,
        language_version: version,
        entry: logical_entry(entry),
        runtime_requirements: vec!["Python 3.11 or newer".into()],
        invocation: "python program.py".into(),
        compiler: Some(compiler),
        files: vec![
            artifact::File::text("program.py", python_program(generated, "build")),
            artifact::File::text("topaz_py_rt.py", topaz_emit_py::PY_RT),
        ],
    };
    match destination.commit(plan) {
        Ok(()) => {
            eprintln!(
                "topaz: wrote self Python deployment bundle to `{}` (`program.py` + target runtimes)",
                dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "topaz: could not install self Python artifact in `{}`: {error}",
                dir.display()
            );
            ExitCode::FAILURE
        }
    }
}
