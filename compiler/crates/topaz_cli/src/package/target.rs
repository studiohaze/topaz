use crate::*;

#[derive(Debug)]
pub(super) struct PackageTarget {
    pub(super) package_name: String,
    pub(super) package_version: String,
    pub(super) entry: String,
    pub(super) root: PathBuf,
    pub(super) version: LangVersion,
    pub(super) build_target: String,
    pub(super) build_deterministic: bool,
    pub(super) locked: bool,
    pub(super) web: topaz_package::WebConfig,
    pub(super) web_capabilities: topaz_package::WebCapabilities,
    pub(super) service: topaz_package::ServiceConfig,
    pub(super) path_deps: BTreeMap<String, PackageDepMount>,
    pub(super) externs: BTreeMap<String, topaz_package::ExternModule>,
    pub(super) extern_replay: topaz_value::ExternReplayStore,
    pub(super) extern_replay_jsonl: String,
    pub(super) extern_sandbox_policies: Vec<topaz_value::ExternSandboxPolicy>,
    pub(super) extern_replay_errors: BTreeMap<String, String>,
    pub(super) generated_std_modules: BTreeMap<String, topaz_resolve::GeneratedStdModule>,
    pub(super) fs_read_roots: Vec<String>,
    pub(super) fs_write_roots: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct PackageDepMount {
    pub(super) root: PathBuf,
    pub(super) root_module: String,
}

pub(super) fn package_target(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    locked: bool,
) -> Result<PackageTarget, ExitCode> {
    package_target_with_profile_policy(root, version_arg, locked, false)
}

pub(super) fn bootstrap_package_target(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    locked: bool,
) -> Result<PackageTarget, ExitCode> {
    package_target_with_profile_policy(root, version_arg, locked, true)
}

pub(super) fn package_target_with_profile_policy(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    locked: bool,
    retain_nondeterministic: bool,
) -> Result<PackageTarget, ExitCode> {
    let root = root.unwrap_or(".");
    let project = match if retain_nondeterministic {
        topaz_package::Project::load_for_profile(root)
    } else {
        topaz_package::Project::load(root)
    } {
        Ok(project) => project,
        Err(e) => {
            eprintln!("topaz: {e}");
            return Err(ExitCode::FAILURE);
        }
    };
    let mut generated_std_modules = BTreeMap::new();
    if project.manifest.lispex.is_some() {
        if !locked {
            eprintln!("topaz: a package with [lispex] requires --locked");
            return Err(ExitCode::FAILURE);
        }
        let modules = match topaz_lispex_product::application_modules(&project) {
            Ok(modules) => modules,
            Err(error) => {
                eprintln!("topaz: {error}");
                return Err(ExitCode::FAILURE);
            }
        };
        for module in modules {
            generated_std_modules.insert(
                module.identity.to_string(),
                topaz_resolve::GeneratedStdModule {
                    path: module.path.to_string(),
                    source: module.source,
                },
            );
        }
    } else if locked && let Err(e) = project.verify_locked() {
        eprintln!("topaz: {e}");
        return Err(ExitCode::FAILURE);
    }
    if let Some(selected) = version_arg
        && selected != project.manifest.package.language
    {
        eprintln!(
            "topaz: --language-version conflicts with topaz.toml [package].language \
             (manifest {}, CLI {})",
            lang_version_text(project.manifest.package.language),
            lang_version_text(selected)
        );
        return Err(ExitCode::FAILURE);
    }
    let mut path_deps = BTreeMap::new();
    for (name, dep) in &project.manifest.dependencies {
        if name == "std" {
            continue;
        }
        let Some(path) = &dep.path else {
            let Some(version) = &dep.version else {
                continue;
            };
            if !locked {
                eprintln!(
                    "topaz: registry package `{name}` version `{version}` requires `--locked` \
                     and vendored content at `vendor/{name}/{version}`"
                );
                return Err(ExitCode::FAILURE);
            }
            let dep_root = topaz_package::registry_vendor_root(&project.root, name, version);
            let dep_project = match topaz_package::Project::load(&dep_root) {
                Ok(project) => project,
                Err(e) => {
                    eprintln!("topaz: {e}");
                    return Err(ExitCode::FAILURE);
                }
            };
            if dep_project.manifest.package.name != *name
                || dep_project.manifest.package.version != *version
            {
                eprintln!(
                    "topaz: vendored registry package `{name}` version `{version}` points to `{}` \
                     whose [package] is `{}` version `{}`",
                    dep_root.to_string_lossy(),
                    dep_project.manifest.package.name,
                    dep_project.manifest.package.version
                );
                return Err(ExitCode::FAILURE);
            }
            if project.root.join(name).exists() || project.root.join(format!("{name}.tpz")).exists()
            {
                eprintln!(
                    "topaz: registry package dependency `{name}` conflicts with a root module path"
                );
                return Err(ExitCode::FAILURE);
            }
            let root_module = dep_project
                .manifest
                .exports
                .as_ref()
                .map(|exports| exports.module.clone())
                .unwrap_or_else(|| dep_project.manifest.package.entry.clone());
            path_deps.insert(
                name.clone(),
                PackageDepMount {
                    root: dep_root,
                    root_module,
                },
            );
            continue;
        };
        let dep_root = project.root.join(path);
        let dep_project = match topaz_package::Project::load(&dep_root) {
            Ok(project) => project,
            Err(e) => {
                eprintln!("topaz: {e}");
                return Err(ExitCode::FAILURE);
            }
        };
        if dep_project.manifest.package.name != *name {
            eprintln!(
                "topaz: local package `{name}` points to `{}` whose [package].name is `{}`",
                dep_root.to_string_lossy(),
                dep_project.manifest.package.name
            );
            return Err(ExitCode::FAILURE);
        }
        if project.root.join(name).exists() || project.root.join(format!("{name}.tpz")).exists() {
            eprintln!("topaz: local package dependency `{name}` conflicts with a root module path");
            return Err(ExitCode::FAILURE);
        }
        let root_module = dep_project
            .manifest
            .exports
            .as_ref()
            .map(|exports| exports.module.clone())
            .unwrap_or_else(|| dep_project.manifest.package.entry.clone());
        path_deps.insert(
            name.clone(),
            PackageDepMount {
                root: dep_root,
                root_module,
            },
        );
    }
    for module in project.manifest.externs.keys() {
        let root_segment = module.split('.').next().unwrap_or(module);
        if path_deps.contains_key(root_segment)
            || project.root.join(root_segment).exists()
            || project.root.join(format!("{root_segment}.tpz")).exists()
        {
            eprintln!(
                "topaz: extern module `{module}` conflicts with root module path `{root_segment}`"
            );
            return Err(ExitCode::FAILURE);
        }
    }
    let entry = project.manifest.package.entry.clone();
    let externs = project.manifest.externs.clone();
    let (extern_replay, extern_replay_jsonl, extern_sandbox_policies, extern_replay_errors) =
        load_extern_replay_bindings(&project.root, &externs);
    Ok(PackageTarget {
        package_name: project.manifest.package.name.clone(),
        package_version: project.manifest.package.version.clone(),
        entry,
        root: project.root,
        version: project.manifest.package.language,
        build_target: project.manifest.build.target,
        build_deterministic: project.manifest.build.deterministic,
        locked,
        web: project.manifest.web,
        web_capabilities: project.manifest.capabilities.web,
        service: project.manifest.service,
        path_deps,
        externs,
        extern_replay,
        extern_replay_jsonl,
        extern_sandbox_policies,
        extern_replay_errors,
        generated_std_modules,
        fs_read_roots: project.manifest.capabilities.fs.read,
        fs_write_roots: project.manifest.capabilities.fs.write,
    })
}

pub(super) fn load_extern_replay_bindings(
    root: &Path,
    externs: &BTreeMap<String, topaz_package::ExternModule>,
) -> (
    topaz_value::ExternReplayStore,
    String,
    Vec<topaz_value::ExternSandboxPolicy>,
    BTreeMap<String, String>,
) {
    let mut store = topaz_value::ExternReplayStore::empty();
    let mut jsonl = String::new();
    let mut errors = BTreeMap::new();
    let policies = extern_sandbox_policies(externs);
    for (name, module) in externs {
        let fixture = &module.replay.fixture;
        match topaz_package::read_extern_replay_fixture(root, name, module) {
            Ok(bytes) => match std::str::from_utf8(&bytes) {
                Ok(src) => match store.merge_jsonl(src) {
                    Ok(()) => {
                        jsonl.push_str(src);
                        if !src.ends_with('\n') {
                            jsonl.push('\n');
                        }
                    }
                    Err(e) => {
                        errors.insert(name.clone(), format!("`{fixture}` is invalid: {e}"));
                    }
                },
                Err(e) => {
                    errors.insert(name.clone(), format!("`{fixture}` is invalid UTF-8: {e}"));
                }
            },
            Err(e) => {
                errors.insert(name.clone(), e.to_string());
            }
        }
    }
    if let Err(e) = store.set_policies(policies.clone()) {
        errors.insert("<extern-policy>".to_string(), e);
    }
    (store, jsonl, policies, errors)
}

pub(super) fn extern_sandbox_policies(
    externs: &BTreeMap<String, topaz_package::ExternModule>,
) -> Vec<topaz_value::ExternSandboxPolicy> {
    externs
        .iter()
        .map(|(name, module)| {
            topaz_value::ExternSandboxPolicy::new(
                name,
                extern_sandbox_kind(module.sandbox.kind),
                module
                    .artifact
                    .as_ref()
                    .map(|artifact| artifact.path.clone()),
                module.sandbox.fuel,
                module.sandbox.memory_bytes,
            )
        })
        .collect()
}

pub(super) fn extern_sandbox_kind(
    kind: topaz_package::ExternSandboxKind,
) -> topaz_value::ExternSandboxKind {
    match kind {
        topaz_package::ExternSandboxKind::Replay => topaz_value::ExternSandboxKind::Replay,
        topaz_package::ExternSandboxKind::Wasm => topaz_value::ExternSandboxKind::Wasm,
    }
}

pub(super) fn lang_version_text(version: LangVersion) -> &'static str {
    version.as_str()
}

#[derive(Debug)]
pub(super) struct PackageProvider<'a> {
    pub(super) target: &'a PackageTarget,
}

impl<'a> PackageProvider<'a> {
    pub(super) fn new(target: &'a PackageTarget) -> Self {
        Self { target }
    }

    pub(super) fn extern_module_for_path(
        &self,
        path: &str,
    ) -> Option<(&str, &topaz_package::ExternModule)> {
        let path = topaz_resolve::normalize_path(path);
        let module = path.strip_suffix(".tpz")?.replace('/', ".");
        self.target
            .externs
            .get_key_value(module.as_str())
            .map(|(name, module)| (name.as_str(), module))
    }

    pub(super) fn extern_namespace_root(&self, identity: &str) -> bool {
        let Some(root) = identity.split('.').next() else {
            return false;
        };
        self.target
            .externs
            .keys()
            .any(|module| module.split('.').next() == Some(root))
    }

    pub(super) fn extern_entries(&self, dir: &str) -> Vec<(String, bool)> {
        let dir = topaz_resolve::normalize_path(dir);
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut out = Vec::new();
        for module in self.target.externs.keys() {
            let path = module.replace('.', "/") + ".tpz";
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((head, _)) => out.push((head.to_string(), true)),
                None => out.push((rest.to_string(), false)),
            }
        }
        out.sort();
        out.dedup();
        out
    }

    pub(super) fn extern_source(
        &self,
        module_name: &str,
        module: &topaz_package::ExternModule,
    ) -> String {
        let mut src = String::new();
        writeln!(
            &mut src,
            "// generated manifest extern module `{module_name}`"
        )
        .expect("write to string");
        for function in &module.functions {
            let params = function
                .params
                .iter()
                .enumerate()
                .map(|(i, ty)| format!("p{i}: {}", ty.canonical()))
                .collect::<Vec<_>>()
                .join(", ");
            let result = function.result.canonical();
            let body = extern_default_expr(&function.result);
            writeln!(
                &mut src,
                "export function {}({params}) -> {result} {{ {body} }}",
                function.name
            )
            .expect("write to string");
        }
        src
    }

    pub(super) fn physical_path(&self, path: &str) -> PathBuf {
        let path = topaz_resolve::normalize_path(path);
        if let Some(dep_name) = path.strip_suffix(".tpz")
            && !dep_name.contains('/')
            && let Some(dep) = self.target.path_deps.get(dep_name)
        {
            return dep.root.join(&dep.root_module);
        }
        let mut parts = path.splitn(2, '/');
        let first = parts.next().unwrap_or("");
        if let Some(dep) = self.target.path_deps.get(first) {
            return match parts.next() {
                Some(rest) if !rest.is_empty() => dep.root.join(rest),
                _ => dep.root.clone(),
            };
        }
        self.target.root.join(path)
    }

    pub(super) fn read_physical_directory(dir: &Path) -> topaz_resolve::DirectoryRead {
        PhysicalProvider::new(dir).read_directory("")
    }
}

pub(super) fn extern_default_expr(ty: &topaz_package::AbiType) -> String {
    match ty {
        topaz_package::AbiType::Unit => "()".to_string(),
        topaz_package::AbiType::Bool => "false".to_string(),
        topaz_package::AbiType::Int => "0".to_string(),
        topaz_package::AbiType::Float => "0.0".to_string(),
        topaz_package::AbiType::String => "\"\"".to_string(),
        topaz_package::AbiType::Bytes => "Bytes.empty()".to_string(),
        topaz_package::AbiType::Array(_) => "[]".to_string(),
        topaz_package::AbiType::Option(_) => "None".to_string(),
        topaz_package::AbiType::Result(ok, _) => {
            format!("Ok({})", extern_default_expr(ok))
        }
    }
}

pub(super) fn source_fact_from_path(path: &Path) -> topaz_kernel::SourceFact {
    topaz_resolve::read_source_path(path).into()
}

pub(super) fn directory_fact_from_path(path: &Path) -> topaz_kernel::DirectoryFact {
    PhysicalProvider::new(path).read_directory("").into()
}

pub(super) struct PhysicalFactHost {
    pub(super) base: PathBuf,
}

impl PhysicalFactHost {
    pub(super) fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    pub(super) fn path(&self, logical: &str) -> PathBuf {
        let mut path = self.base.clone();
        for segment in topaz_resolve::normalize_path(logical)
            .split('/')
            .filter(|segment| !segment.is_empty())
        {
            path.push(segment);
        }
        path
    }

    pub(super) fn containment(
        &self,
        request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::ContainmentFact {
        if query
            .logical_path()
            .split('/')
            .any(|segment| segment == "..")
        {
            return topaz_kernel::ContainmentFact::Outside;
        }
        let Some(mount) = request
            .mounts()
            .iter()
            .find(|mount| mount.id == query.mount_id())
        else {
            return topaz_kernel::ContainmentFact::Unresolved;
        };
        let Ok(root) = fs::canonicalize(self.path(&mount.logical_root)) else {
            return topaz_kernel::ContainmentFact::Unresolved;
        };
        let target = match fs::canonicalize(self.path(query.logical_path())) {
            Ok(target) => target,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return topaz_kernel::ContainmentFact::Missing;
            }
            Err(_) => return topaz_kernel::ContainmentFact::Unresolved,
        };
        let Ok(relative) = target.strip_prefix(&root) else {
            return topaz_kernel::ContainmentFact::Outside;
        };
        let relative = topaz_resolve::physical_path_identity(relative);
        topaz_kernel::ContainmentFact::Inside {
            alias_class: topaz_kernel::physical_alias_class("root", &relative),
        }
    }
}

impl topaz_kernel::HostFactSource for PhysicalFactHost {
    fn respond(
        &self,
        request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                topaz_kernel::HostFact::Source(source_fact_from_path(&self.path(logical_path)))
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(directory_fact_from_path(
                    &self.path(logical_path),
                ))
            }
            topaz_kernel::HostQuery::PhysicalContainment { .. } => {
                topaz_kernel::HostFact::Containment(self.containment(request, query))
            }
        }
    }
}

pub(super) struct PackageFactHost<'a> {
    pub(super) provider: PackageProvider<'a>,
}

impl<'a> PackageFactHost<'a> {
    pub(super) fn new(target: &'a PackageTarget) -> Self {
        Self {
            provider: PackageProvider::new(target),
        }
    }

    pub(super) fn alias_identity(
        &self,
        logical_path: &str,
        physical: &Path,
    ) -> Option<(String, String)> {
        if self.provider.extern_module_for_path(logical_path).is_some()
            || self
                .provider
                .extern_namespace_root(&logical_path.replace('/', "."))
        {
            return Some(("extern".to_string(), logical_path.to_string()));
        }
        let mut candidates = Vec::new();
        if let Ok(root) = fs::canonicalize(&self.provider.target.root)
            && let Ok(relative) = physical.strip_prefix(root)
        {
            candidates.push((
                "root".to_string(),
                topaz_resolve::physical_path_identity(relative),
            ));
        }
        for (name, dependency) in &self.provider.target.path_deps {
            if let Ok(root) = fs::canonicalize(&dependency.root)
                && let Ok(relative) = physical.strip_prefix(root)
            {
                candidates.push((
                    format!("dep:{name}"),
                    topaz_resolve::physical_path_identity(relative),
                ));
            }
        }
        candidates.sort();
        candidates.into_iter().next()
    }

    pub(super) fn generated_std_source(&self, logical_path: &str) -> Option<String> {
        let logical_path = topaz_resolve::normalize_path(logical_path);
        self.provider
            .target
            .generated_std_modules
            .values()
            .find(|module| topaz_resolve::normalize_path(&module.path) == logical_path)
            .map(|module| module.source.clone())
    }
}

impl topaz_kernel::HostFactSource for PackageFactHost<'_> {
    fn respond(
        &self,
        _request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        match query {
            topaz_kernel::HostQuery::ReadSource { logical_path, .. } => {
                if let Some(source) = self.generated_std_source(logical_path) {
                    topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(source))
                } else if let Some((name, module)) =
                    self.provider.extern_module_for_path(logical_path)
                {
                    topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                        self.provider.extern_source(name, module),
                    ))
                } else {
                    topaz_kernel::HostFact::Source(source_fact_from_path(
                        &self.provider.physical_path(logical_path),
                    ))
                }
            }
            topaz_kernel::HostQuery::ListDirectory { logical_path, .. } => {
                topaz_kernel::HostFact::Directory(self.provider.read_directory(logical_path).into())
            }
            topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                if logical_path.split('/').any(|segment| segment == "..") {
                    return topaz_kernel::HostFact::Containment(
                        topaz_kernel::ContainmentFact::Outside,
                    );
                }
                let physical_path = self.provider.physical_path(logical_path);
                let Some(physical) = self.provider.physical_id(logical_path) else {
                    return topaz_kernel::HostFact::Containment(
                        topaz_kernel::ContainmentFact::Missing,
                    );
                };
                let canonical =
                    fs::canonicalize(&physical_path).unwrap_or_else(|_| physical.into());
                let Some((mount_group, relative)) = self.alias_identity(logical_path, &canonical)
                else {
                    return topaz_kernel::HostFact::Containment(
                        topaz_kernel::ContainmentFact::Outside,
                    );
                };
                topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                    alias_class: topaz_kernel::physical_alias_class(&mount_group, &relative),
                })
            }
        }
    }
}

impl FileProvider for PackageProvider<'_> {
    fn read(&self, path: &str) -> topaz_resolve::SourceRead {
        if let Some((name, module)) = self.extern_module_for_path(path) {
            return topaz_resolve::SourceRead::Present(self.extern_source(name, module));
        }
        topaz_resolve::read_source_path(&self.physical_path(path))
    }

    fn is_extern_file(&self, path: &str) -> bool {
        self.extern_module_for_path(path).is_some()
    }

    fn generated_std_module(&self, identity: &str) -> Option<topaz_resolve::GeneratedStdModule> {
        self.target.generated_std_modules.get(identity).cloned()
    }

    fn is_extern_namespace(&self, identity: &str) -> bool {
        self.extern_namespace_root(identity)
    }

    fn extern_replay_error(&self, identity: &str) -> Option<String> {
        self.target.extern_replay_errors.get(identity).cloned()
    }

    fn read_directory(&self, dir: &str) -> topaz_resolve::DirectoryRead {
        let dir = topaz_resolve::normalize_path(dir);
        let mut virtual_entries = self.extern_entries(&dir);
        if dir.is_empty() {
            for name in self.target.path_deps.keys() {
                virtual_entries.push((name.clone(), true));
                virtual_entries.push((format!("{name}.tpz"), false));
            }
        }
        let mut out = match Self::read_physical_directory(&self.physical_path(&dir)) {
            topaz_resolve::DirectoryRead::Present(entries) => entries,
            topaz_resolve::DirectoryRead::Missing if virtual_entries.is_empty() => {
                return topaz_resolve::DirectoryRead::Missing;
            }
            topaz_resolve::DirectoryRead::Missing => Vec::new(),
            unreadable @ topaz_resolve::DirectoryRead::Unreadable { .. } => return unreadable,
        };
        out.extend(virtual_entries);
        out.sort();
        out.dedup();
        topaz_resolve::DirectoryRead::Present(out)
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        let path = topaz_resolve::normalize_path(path);
        if path.is_empty() {
            return Some(String::new());
        }
        if self.extern_module_for_path(&path).is_some()
            || self.extern_namespace_root(&path.replace('/', "."))
        {
            return Some(topaz_resolve::physical_path_identity(
                &self.target.root.join(".topaz-extern-virtual").join(&path),
            ));
        }
        fs::canonicalize(self.physical_path(&path))
            .ok()
            .map(|path| topaz_resolve::physical_path_identity(&path))
    }
}

pub(super) fn resolve_package_target(target: &PackageTarget) -> topaz_resolve::ResolveOutput {
    let provider = PackageProvider::new(target);
    resolve_with_version(&provider, &target.entry, Some(""), target.version)
}

pub(super) fn package_kernel_facts(target: &PackageTarget) -> topaz_kernel::PackageFacts {
    let mut capabilities = BTreeSet::new();
    capabilities.extend(
        target
            .fs_read_roots
            .iter()
            .map(|root| format!("fs.read:{root}")),
    );
    capabilities.extend(
        target
            .fs_write_roots
            .iter()
            .map(|root| format!("fs.write:{root}")),
    );
    if target.web_capabilities.open_text {
        capabilities.insert("web.open-text".to_string());
    }
    if target.web_capabilities.download_text {
        capabilities.insert("web.download-text".to_string());
    }
    if target.web_capabilities.local_state {
        capabilities.insert("web.local-state".to_string());
    }
    topaz_kernel::PackageFacts {
        identity: Some(format!(
            "{}@{}",
            target.package_name, target.package_version
        )),
        build_role: topaz_kernel::BuildRole::Package,
        deterministic: target.build_deterministic,
        executable_profile: Some(target.build_target.clone()),
        dependency_mount_ids: target
            .path_deps
            .keys()
            .map(|name| format!("dep:{name}"))
            .collect(),
        extern_modules: target.externs.keys().cloned().collect(),
        extern_replay_errors: target.extern_replay_errors.clone(),
        generated_std_modules: target.generated_std_modules.clone(),
        capabilities,
        locked: target.locked,
    }
}
