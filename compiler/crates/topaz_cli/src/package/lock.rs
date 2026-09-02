use crate::*;

pub(super) fn write_package_lock(root: Option<&str>, version_arg: Option<LangVersion>) -> ExitCode {
    if version_arg.is_some() {
        eprintln!("topaz: `lock` uses topaz.toml [package].language; drop --language-version");
        return ExitCode::FAILURE;
    }
    let root = root.unwrap_or(".");
    let project = match topaz_package::Project::load(root) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("topaz: {e}");
            return ExitCode::FAILURE;
        }
    };
    if project.manifest.lispex.is_some() {
        if let Err(error) = topaz_lispex_product::write_locked_package(&project) {
            eprintln!("topaz: {error}");
            return ExitCode::FAILURE;
        }
    } else if let Err(e) = project.write_lockfile() {
        eprintln!("topaz: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!(
        "topaz: wrote `{}`",
        project.root.join("topaz.lock").to_string_lossy()
    );
    ExitCode::SUCCESS
}
