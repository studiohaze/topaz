use super::*;
use crate::*;

/// Parses and admits one deterministic `topaz.toml` document.
pub fn parse_manifest(text: &str) -> Result<PackageManifest, PackageError> {
    parse_manifest_with_build_policy(text, false)
}

pub(crate) fn parse_manifest_with_build_policy(
    text: &str,
    retain_nondeterministic: bool,
) -> Result<PackageManifest, PackageError> {
    let doc = toml_parse_document(text)
        .map_err(|e| PackageError::new(format!("topaz.toml parse error: {e}")))?;
    let root = expect_table(&doc, "topaz.toml")?;
    reject_unknown_keys(
        root,
        "topaz.toml",
        &[
            "package",
            "build",
            "dependencies",
            "capabilities",
            "extern",
            "exports",
            "web",
            "service",
            "lispex",
        ],
    )?;
    let package = parse_package_section(required_table(root, "package", "topaz.toml")?)?;
    let build = parse_build_section(optional_table(root, "build")?, retain_nondeterministic)?;
    let dependencies = parse_dependencies(optional_table(root, "dependencies")?)?;
    let capabilities_table = optional_table(root, "capabilities")?;
    let web_capabilities_declared = match capabilities_table {
        Some(table) => optional_table(table, "web")?.is_some(),
        None => false,
    };
    let capabilities = parse_capabilities(capabilities_table)?;
    if web_capabilities_declared && build.target != "web-app" {
        return Err(PackageError::new(
            "[capabilities.web] is allowed only when [build].target is `web-app`",
        ));
    }
    let externs = parse_externs(optional_table(root, "extern")?)?;
    let exports = optional_table(root, "exports")?
        .map(parse_exports)
        .transpose()?;
    let web_table = optional_table(root, "web")?;
    if web_table.is_some() && build.target != "web-app" {
        return Err(PackageError::new(
            "[web] is allowed only when [build].target is `web-app`",
        ));
    }
    let web = parse_web_section(web_table)?;
    let service_table = optional_table(root, "service")?;
    if service_table.is_some() && build.target != "http-service" {
        return Err(PackageError::new(
            "[service] is allowed only when [build].target is `http-service`",
        ));
    }
    let service = parse_service_section(service_table)?;
    let lispex = optional_table(root, "lispex")?
        .map(parse_lispex_section)
        .transpose()?;
    let manifest = PackageManifest {
        package,
        build,
        dependencies,
        capabilities,
        externs,
        exports,
        web,
        service,
        lispex,
    };
    validate_lispex_application_binding(&manifest)?;
    Ok(manifest)
}

/// Enforce the exact language and standard-library pair that owns each
/// first-class Lispex application profile. The complete-current-profile route
/// is bound to the current v5.20 public language profile.
pub fn validate_lispex_application_binding(manifest: &PackageManifest) -> Result<(), PackageError> {
    validate_lispex_application_binding_parts(
        manifest.package.language,
        &manifest.dependencies,
        manifest.lispex.as_ref(),
    )
}

pub(crate) fn validate_lispex_application_binding_parts(
    language: LangVersion,
    dependencies: &BTreeMap<String, Dependency>,
    lispex: Option<&LispexConfig>,
) -> Result<(), PackageError> {
    let Some(lispex) = lispex else {
        return Ok(());
    };
    let (application, required_language, required_std, required_profile) =
        match lispex.application.as_deref() {
            Some(LISPEX_APPLICATION_PROFILE_ID) => (
                LISPEX_APPLICATION_PROFILE_ID,
                LISPEX_APPLICATION_LANGUAGE,
                LISPEX_APPLICATION_STD_VERSION,
                LISPEX_BOUNDED_PROFILE_ID,
            ),
            Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID) => (
                LISPEX_COMPLETE_APPLICATION_PROFILE_ID,
                LISPEX_COMPLETE_APPLICATION_LANGUAGE,
                LISPEX_COMPLETE_APPLICATION_STD_VERSION,
                LISPEX_COMPLETE_PROFILE_ID,
            ),
            _ => return Ok(()),
        };
    if lispex.profile != required_profile {
        return Err(PackageError::new(format!(
            "[lispex].application `{application}` requires [lispex].profile `{required_profile}`"
        )));
    }
    if language != required_language {
        return Err(PackageError::new(format!(
            "[lispex].application `{application}` requires [package].language `{required_std}`"
        )));
    }
    let Some(std) = dependencies.get("std") else {
        return Err(PackageError::new(format!(
            "[lispex].application `{application}` requires [dependencies].std = \"{required_std}\""
        )));
    };
    if std.version.as_deref() != Some(required_std) || std.path.is_some() || std.hash.is_some() {
        return Err(PackageError::new(format!(
            "[lispex].application `{application}` requires the exact version dependency [dependencies].std = \"{required_std}\""
        )));
    }
    Ok(())
}

/// Hashes the exact manifest text, including whitespace, and adds the `sha256:` prefix.
pub fn manifest_sha256(text: &str) -> String {
    let digest = sha256(text.as_bytes());
    let mut hex = String::with_capacity(64);
    bytes_to_hex_into(&mut hex, &digest);
    format!("sha256:{hex}")
}
