use crate::*;

impl Project {
    /// Loads and validates a package from its canonical root manifest.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, PackageError> {
        Self::load_with_build_policy(root, false)
    }

    /// Load a root manifest while retaining a false deterministic flag so a
    /// stricter usage profile can report its own stable machine rule. Ordinary
    /// package loading stays strict; this is not an execution/build bypass.
    pub fn load_for_profile(root: impl AsRef<Path>) -> Result<Self, PackageError> {
        Self::load_with_build_policy(root, true)
    }

    fn load_with_build_policy(
        root: impl AsRef<Path>,
        retain_nondeterministic: bool,
    ) -> Result<Self, PackageError> {
        let requested_root = root.as_ref();
        let root = std::fs::canonicalize(requested_root).map_err(|e| {
            PackageError::new(format!(
                "cannot resolve package root `{}`: {e}",
                requested_root.to_string_lossy()
            ))
        })?;
        let manifest_text = read_package_text_strict(&root, "topaz.toml", "package manifest")?;
        let manifest = parse_manifest_with_build_policy(&manifest_text, retain_nondeterministic)?;
        Ok(Self {
            root,
            manifest_text,
            manifest,
        })
    }

    /// Checks the package's current lockfile and all referenced locked inputs.
    pub fn verify_locked(&self) -> Result<(), PackageError> {
        check_lock(&self.root, &self.manifest_text, &self.manifest)
    }

    /// Produces the canonical lockfile text for the loaded project.
    pub fn render_lockfile(&self) -> Result<String, PackageError> {
        render_lockfile(self)
    }

    /// Replaces `topaz.lock` through the package's strict file boundary.
    pub fn write_lockfile(&self) -> Result<(), PackageError> {
        let text = self.render_lockfile()?;
        replace_package_file_strict(&self.root, "topaz.lock", text.as_bytes())
    }
}
