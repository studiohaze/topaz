use super::validate::schema::{object_fields, string_field};
use super::*;

#[derive(Clone, Copy)]
pub(super) struct ComparedMember<'a> {
    pub(super) path: &'a str,
    pub(super) bytes: &'a [u8],
}

fn member_map(bundle: &ObservationBundle) -> BTreeMap<&str, &[u8]> {
    bundle
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.bytes.as_slice()))
        .collect()
}

fn first_row_identity(left: &[u8], right: &[u8]) -> Option<String> {
    let left = left.split(|byte| *byte == b'\n');
    let right = right.split(|byte| *byte == b'\n');
    for (left, right) in left.zip(right) {
        if left == right || (left.is_empty() && right.is_empty()) {
            continue;
        }
        for row in [left, right] {
            let Ok(values) = crate::canonical::validate(row, false) else {
                continue;
            };
            let Some(value) = values.first() else {
                continue;
            };
            let Ok(fields) = object_fields(value) else {
                continue;
            };
            for key in [
                "operationId",
                "nodeId",
                "symbolId",
                "scopeId",
                "referenceNodeId",
                "declarationNodeId",
                "sourceId",
                "rowKind",
            ] {
                if let Some(JsonValue::String(value)) = fields.get(key) {
                    return Some(value.to_string());
                }
            }
        }
        return None;
    }
    None
}

fn mismatch_row(phase: &str, path: &str, left: Option<&[u8]>, right: Option<&[u8]>) -> JsonValue {
    let kind = match (left, right) {
        (None, Some(_)) => "missing-left",
        (Some(_), None) => "missing-right",
        (Some(_), Some(_)) => "bytes",
        (None, None) => unreachable!("a mismatch has at least one member"),
    };
    object([
        (
            "leftByteLength",
            left.map_or(JsonValue::Null, |bytes| unsigned(bytes.len() as u64)),
        ),
        (
            "leftSha256",
            left.map_or(JsonValue::Null, |bytes| string(sha256(bytes))),
        ),
        ("kind", string(kind)),
        ("path", string(path)),
        ("phase", string(phase)),
        (
            "rightByteLength",
            right.map_or(JsonValue::Null, |bytes| unsigned(bytes.len() as u64)),
        ),
        (
            "rightSha256",
            right.map_or(JsonValue::Null, |bytes| string(sha256(bytes))),
        ),
        (
            "stableId",
            match (left, right) {
                (Some(left), Some(right)) if path.ends_with(".jsonl") => {
                    first_row_identity(left, right).map_or(JsonValue::Null, string)
                }
                _ => JsonValue::Null,
            },
        ),
    ])
}

fn comparison_record(
    layer: ComparisonLayer,
    phase: Option<&str>,
    mismatches: Vec<JsonValue>,
    total: usize,
) -> ComparisonRecord {
    let equal = total == 0;
    let bytes = encode(&object([
        ("equal", boolean(equal)),
        ("firstFailingPhase", phase.map_or(JsonValue::Null, string)),
        ("layer", string(layer.as_str())),
        ("mismatchCount", unsigned(total as u64)),
        ("mismatches", array(mismatches)),
        ("schema", string(COMPARISON_SCHEMA)),
        ("truncated", boolean(total > COMPARISON_MISMATCH_LIMIT)),
    ]));
    ComparisonRecord {
        equal,
        first_failing_phase: phase.map(str::to_string),
        mismatch_count: total,
        bytes,
    }
}

pub(super) fn compare_phase<'a>(
    layer: ComparisonLayer,
    phase: &str,
    left: impl IntoIterator<Item = ComparedMember<'a>>,
    right: impl IntoIterator<Item = ComparedMember<'a>>,
) -> Option<ComparisonRecord> {
    let left = left
        .into_iter()
        .map(|member| (member.path, member.bytes))
        .collect::<BTreeMap<_, _>>();
    let right = right
        .into_iter()
        .map(|member| (member.path, member.bytes))
        .collect::<BTreeMap<_, _>>();
    let paths = left
        .keys()
        .chain(right.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut mismatches = Vec::new();
    let mut total = 0;
    for path in paths {
        let left = left.get(path).copied();
        let right = right.get(path).copied();
        if left == right {
            continue;
        }
        total += 1;
        if mismatches.len() < COMPARISON_MISMATCH_LIMIT {
            mismatches.push(mismatch_row(phase, path, left, right));
        }
    }
    (total > 0).then(|| comparison_record(layer, Some(phase), mismatches, total))
}

fn selected_members<'a>(
    files: &'a BTreeMap<&'a str, &'a [u8]>,
    exact: &[&str],
    prefix: Option<&str>,
) -> Vec<ComparedMember<'a>> {
    files
        .iter()
        .filter(|(path, _)| {
            exact.contains(path) || prefix.is_some_and(|prefix| path.starts_with(prefix))
        })
        .map(|(path, bytes)| ComparedMember { path, bytes })
        .collect()
}

fn semantic_outcome(files: &BTreeMap<&str, &[u8]>) -> Result<Vec<u8>, String> {
    let response = files
        .get("response.json")
        .ok_or_else(|| "observation bundle is missing response.json".to_string())?;
    let values = crate::canonical::validate(response, false)?;
    let value = values
        .first()
        .ok_or_else(|| "response.json is empty".to_string())?;
    Ok(encode(&object([
        (
            "highestCompletedPhase",
            string(string_field(value, "highestCompletedPhase")?),
        ),
        ("status", string(string_field(value, "status")?)),
    ])))
}

/// Compares two admitted observation bundles at the requested product layers.
pub fn compare_observations(
    left: &ObservationBundle,
    right: &ObservationBundle,
    layer: ComparisonLayer,
) -> Result<ComparisonRecord, String> {
    if layer == ComparisonLayer::NativeBinary {
        return Err(
            "native-binary comparison requires separately supplied binary bytes".to_string(),
        );
    }
    left.validate()
        .map_err(|error| format!("left observation is invalid: {error}"))?;
    right
        .validate()
        .map_err(|error| format!("right observation is invalid: {error}"))?;
    let left = member_map(left);
    let right = member_map(right);

    if layer == ComparisonLayer::GeneratedSource {
        let left_members = selected_members(&left, &["rust-source.jsonl"], Some("rust/"));
        let right_members = selected_members(&right, &["rust-source.jsonl"], Some("rust/"));
        let record = compare_phase(layer, "rust-source", left_members, right_members);
        return Ok(record.unwrap_or_else(|| comparison_record(layer, None, Vec::new(), 0)));
    }
    if layer == ComparisonLayer::Provenance {
        let record = compare_phase(
            layer,
            "provenance",
            selected_members(&left, &["provenance.json"], None),
            selected_members(&right, &["provenance.json"], None),
        );
        return Ok(record.unwrap_or_else(|| comparison_record(layer, None, Vec::new(), 0)));
    }

    for (phase, exact, prefix) in [
        ("source-set", &["source-set.jsonl"][..], Some("sources/")),
        ("tokens", &["tokens.jsonl"][..], None),
        ("ast", &["ast.jsonl"][..], None),
        ("resolved", &["resolved.jsonl"][..], None),
        ("typed", &["typed.jsonl"][..], None),
        ("lowered", &["lowered.jsonl"][..], None),
        ("diagnostics", &["diagnostics.jsonl"][..], None),
    ] {
        if let Some(record) = compare_phase(
            layer,
            phase,
            selected_members(&left, exact, prefix),
            selected_members(&right, exact, prefix),
        ) {
            return Ok(record);
        }
    }
    let left_outcome = semantic_outcome(&left)?;
    let right_outcome = semantic_outcome(&right)?;
    if let Some(record) = compare_phase(
        layer,
        "outcome",
        [ComparedMember {
            path: "response/outcome",
            bytes: &left_outcome,
        }],
        [ComparedMember {
            path: "response/outcome",
            bytes: &right_outcome,
        }],
    ) {
        return Ok(record);
    }
    Ok(comparison_record(layer, None, Vec::new(), 0))
}

/// Compares native product bytes and records the first bounded mismatch set.
pub fn compare_native_binaries(left: &[u8], right: &[u8]) -> ComparisonRecord {
    if left == right {
        return comparison_record(ComparisonLayer::NativeBinary, None, Vec::new(), 0);
    }
    comparison_record(
        ComparisonLayer::NativeBinary,
        Some("native-binary"),
        vec![mismatch_row(
            "native-binary",
            "binary",
            Some(left),
            Some(right),
        )],
        1,
    )
}

#[cfg(test)]
pub(crate) fn refresh_test_manifest(bundle: &mut ObservationBundle) {
    let files = bundle
        .files
        .iter()
        .filter(|file| file.path != "topaz-observation.json")
        .map(|file| (file.path.clone(), (file.schema.clone(), file.bytes.clone())))
        .collect::<BTreeMap<_, _>>();
    let entries = files
        .iter()
        .map(|(path, (schema, bytes))| {
            object([
                ("byteLength", unsigned(bytes.len() as u64)),
                ("path", string(path)),
                ("schema", string(schema)),
                ("sha256", string(sha256(bytes))),
            ])
        })
        .collect::<Vec<_>>();
    let manifest = bundle
        .files
        .iter_mut()
        .find(|file| file.path == "topaz-observation.json")
        .expect("test bundle has a manifest");
    manifest.bytes = encode(&object([
        ("files", array(entries)),
        ("rootDigest", string(super::bundle::root_digest(&files))),
        ("schema", string(BUNDLE_SCHEMA)),
    ]));
}
