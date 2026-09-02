use crate::*;

pub(super) const WEB_APP_MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub(super) const WEB_APP_MAX_LIVE_REQUESTS: usize = 16;
pub(super) const WEB_APP_MAX_STATE_VALUE_BYTES: usize = 1024 * 1024;
pub(super) const WEB_APP_MAX_STATE_KEYS: usize = 32;

pub(super) fn web_capabilities_json(
    package_name: &str,
    web: &topaz_package::WebConfig,
    capabilities: &topaz_package::WebCapabilities,
) -> String {
    format!(
        "{{\n  \"schema\": \"topaz.web-capabilities.v1\",\n  \"lifecycle\": \"{}\",\n  \"openText\": {},\n  \"downloadText\": {},\n  \"localState\": {},\n  \"stateNamespace\": \"topaz.web-state.v1:{package_name}:\",\n  \"maxTextBytes\": {},\n  \"maxLiveRequests\": {},\n  \"maxStateValueBytes\": {},\n  \"maxStateKeys\": {}\n}}\n",
        web.lifecycle.as_str(),
        capabilities.open_text,
        capabilities.download_text,
        capabilities.local_state,
        WEB_APP_MAX_TEXT_BYTES,
        WEB_APP_MAX_LIVE_REQUESTS,
        WEB_APP_MAX_STATE_VALUE_BYTES,
        WEB_APP_MAX_STATE_KEYS,
    )
}

pub(super) fn collect_web_input(
    package_root: &Path,
    relative: &Path,
    seen: &mut std::collections::BTreeSet<String>,
    files: &mut Vec<artifact::File>,
) -> Result<(), String> {
    let source = package_root.join(relative);
    let metadata = fs::symlink_metadata(&source)
        .map_err(|error| format!("cannot inspect `{}`: {error}", source.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "declared web input `{}` must not be a symlink",
            relative.display()
        ));
    }
    if metadata.is_dir() {
        let mut entries = fs::read_dir(&source)
            .map_err(|error| format!("cannot read `{}`: {error}", source.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot read `{}`: {error}", source.display()))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            collect_web_input(package_root, &relative.join(entry.file_name()), seen, files)?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!(
            "declared web input `{}` is not a regular file",
            relative.display()
        ));
    }
    let output = web_artifact_path(relative)?;
    if !seen.insert(output.clone()) {
        return Ok(());
    }
    let bytes = fs::read(&source)
        .map_err(|error| format!("cannot read `{}`: {error}", source.display()))?;
    if output.starts_with("styles/") {
        let css = std::str::from_utf8(&bytes)
            .map_err(|_| format!("declared stylesheet `{output}` is not UTF-8"))?;
        let normalized = css
            .to_ascii_lowercase()
            .chars()
            .filter(|ch| !ch.is_ascii_whitespace())
            .collect::<String>();
        if normalized.contains("@import") {
            return Err(format!(
                "declared stylesheet `{output}` uses unsupported CSS @import; declare each stylesheet in [web].styles"
            ));
        }
        let mut remainder = normalized.as_str();
        while let Some(start) = remainder.find("url(") {
            let value = &remainder[start + 4..];
            let Some(end) = value.find(')') else {
                break;
            };
            let token = value[..end].trim_matches(['\'', '"']);
            if token.starts_with("http:")
                || token.starts_with("https:")
                || token.starts_with("//")
                || token.contains('\\')
                || token.contains("/*")
            {
                return Err(format!(
                    "declared stylesheet `{output}` contains a remote or escaped CSS url()"
                ));
            }
            remainder = &value[end + 1..];
        }
    }
    files.push(artifact::File::binary(output, bytes, false));
    Ok(())
}

pub(super) fn web_artifact_path(relative: &Path) -> Result<String, String> {
    Ok(relative
        .to_str()
        .ok_or_else(|| {
            format!(
                "declared web input `{}` cannot be represented as a Unicode artifact path",
                relative.display()
            )
        })?
        .replace('\\', "/"))
}

pub(super) fn web_app_index(web: &topaz_package::WebConfig) -> String {
    let mut styles = String::new();
    for path in &web.styles {
        let escaped = html_escape(path);
        styles.push_str(&format!(
            "    <link rel=\"stylesheet\" href=\"./{escaped}\">\n"
        ));
    }
    let title = html_escape(&web.title);
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{title}</title>\n{styles}</head>\n<body>\n  <main id=\"topaz-app\" aria-live=\"polite\"></main>\n  <script type=\"module\" src=\"./topaz-app.js\"></script>\n</body>\n</html>\n"
    )
}

pub(super) fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(super) const WEB_APP_JS: &str = include_str!("../templates/web/app.js");

pub(super) const WEB_LIB_RS: &str = include_str!("../templates/web/lib.rs");

pub(super) const WEB_LOADER_JS: &str = include_str!("../templates/web/loader.js");

pub(super) const WEB_WORKER_JS: &str = include_str!("../templates/web/worker.js");

pub(super) const WEB_WORKER_CLIENT_JS: &str = include_str!("../templates/web/worker-client.js");
