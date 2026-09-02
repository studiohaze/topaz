use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(crate) struct OptionValues {
    values: BTreeMap<String, OsString>,
}

impl OptionValues {
    pub(crate) fn parse(
        arguments: impl IntoIterator<Item = OsString>,
        allowed: &[&str],
    ) -> Result<Self, String> {
        let mut arguments = arguments.into_iter();
        let mut values = BTreeMap::new();
        while let Some(argument) = arguments.next() {
            let name = argument
                .to_str()
                .ok_or_else(|| format!("unknown argument `{}`", argument.to_string_lossy()))?;
            if !allowed.contains(&name) {
                return Err(format!("unknown argument `{name}`"));
            }
            let value = arguments
                .next()
                .ok_or_else(|| format!("{name} requires a value"))?;
            if value.to_string_lossy().starts_with("--") {
                return Err(format!("{name} requires a value"));
            }
            if values.insert(name.to_string(), value).is_some() {
                return Err(format!("duplicate {name}"));
            }
        }
        Ok(Self { values })
    }

    pub(crate) fn value(&mut self, name: &str, required: bool) -> Result<Option<OsString>, String> {
        let value = self.values.remove(name);
        if required && value.is_none() {
            return Err(format!("missing {name}"));
        }
        Ok(value)
    }

    pub(crate) fn path(&mut self, name: &str, required: bool) -> Result<Option<PathBuf>, String> {
        Ok(self.value(name, required)?.map(PathBuf::from))
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut hex = String::new();
    topaz_value::bytes_to_hex_into(&mut hex, &digest);
    hex
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let temporary = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("artifact"),
        std::process::id()
    ));
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|error| {
        format!(
            "cannot commit {} as {}: {error}",
            temporary.display(),
            path.display()
        )
    })
}
