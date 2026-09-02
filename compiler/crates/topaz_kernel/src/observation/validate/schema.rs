use super::super::*;

pub(in super::super) fn validate_schema_registry() -> Result<(), String> {
    let values = crate::canonical::validate(SCHEMA_REGISTRY, false)?;
    exact_fields(&values[0], &["canonicalJson", "schemas"])?;
    if string_field(&values[0], "canonicalJson")? != "topaz.canonical-json/v1" {
        return Err("compiler schema registry has the wrong canonical JSON identity".to_string());
    }
    let actual = array_field(&values[0], "schemas")?
        .iter()
        .map(|value| string_field(value, "identity").map(str::to_string))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let expected = [
        crate::REQUEST_SCHEMA,
        crate::RESPONSE_SCHEMA,
        SOURCE_SET_SCHEMA,
        TOKENS_SCHEMA,
        AST_SCHEMA,
        RESOLVED_SCHEMA,
        "topaz.compiler.typed/v1",
        "topaz.compiler.lowered/v1",
        DIAGNOSTICS_SCHEMA,
        "topaz.compiler.rust-source/v1",
        crate::PROVENANCE_SCHEMA,
        STAGE1_PRODUCT_SCHEMA,
        STAGE2_PRODUCT_SCHEMA,
        STAGE2_FIXED_POINT_SCHEMA,
        "topaz.self-compilation-product/v1",
        "topaz.self-product-adapter-contract/v1",
        "topaz.self-product-partition-ledger/v1",
        BUNDLE_SCHEMA,
        "topaz.compiler.comparison/v1",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err("compiler schema registry identity set drifted".to_string());
    }
    Ok(())
}

pub(in super::super) fn object_fields(
    value: &JsonValue,
) -> Result<&BTreeMap<std::rc::Rc<str>, JsonValue>, String> {
    let JsonValue::Object(fields) = value else {
        return Err("canonical schema row must be an object".to_string());
    };
    Ok(fields)
}

pub(super) fn exact_fields(value: &JsonValue, expected: &[&str]) -> Result<(), String> {
    let fields = object_fields(value)?;
    let actual = fields.keys().map(|key| key.as_ref()).collect::<Vec<_>>();
    if actual != expected {
        return Err(format!(
            "schema fields differ: expected {expected:?}, found {actual:?}"
        ));
    }
    Ok(())
}

fn allowed_fields(value: &JsonValue, required: &[&str], allowed: &[&str]) -> Result<(), String> {
    let fields = object_fields(value)?;
    for required in required {
        if !fields.contains_key(*required) {
            return Err(format!("schema row is missing `{required}`"));
        }
    }
    for key in fields.keys() {
        if !allowed.contains(&key.as_ref()) {
            return Err(format!("schema row contains unknown field `{key}`"));
        }
    }
    Ok(())
}

pub(in super::super) fn string_field<'a>(
    value: &'a JsonValue,
    field: &str,
) -> Result<&'a str, String> {
    let Some(JsonValue::String(value)) = object_fields(value)?.get(field) else {
        return Err(format!("schema field `{field}` must be a string"));
    };
    Ok(value)
}

fn validate_sha256(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field} must use a sha256: identity"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{field} must contain exactly 64 hexadecimal digits"
        ));
    }
    Ok(())
}

pub(super) fn unsigned_field(value: &JsonValue, field: &str) -> Result<u64, String> {
    let Some(JsonValue::Number(value)) = object_fields(value)?.get(field) else {
        return Err(format!("schema field `{field}` must be an integer"));
    };
    value
        .lexeme
        .parse()
        .map_err(|_| format!("schema field `{field}` must be unsigned"))
}

fn boolean_field(value: &JsonValue, field: &str) -> Result<bool, String> {
    let Some(JsonValue::Bool(value)) = object_fields(value)?.get(field) else {
        return Err(format!("schema field `{field}` must be a boolean"));
    };
    Ok(*value)
}

pub(super) fn array_field<'a>(
    value: &'a JsonValue,
    field: &str,
) -> Result<&'a [JsonValue], String> {
    let Some(JsonValue::Array(value)) = object_fields(value)?.get(field) else {
        return Err(format!("schema field `{field}` must be an array"));
    };
    Ok(value)
}

pub(super) fn value_field<'a>(value: &'a JsonValue, field: &str) -> Result<&'a JsonValue, String> {
    object_fields(value)?
        .get(field)
        .ok_or_else(|| format!("schema row is missing `{field}`"))
}

fn validate_semantic_type(value: &JsonValue) -> Result<(), String> {
    let kind = string_field(value, "kind")?;
    match kind {
        "primitive" => {
            exact_fields(value, &["kind", "name"])?;
            if !matches!(
                string_field(value, "name")?,
                "int" | "float" | "string" | "bool" | "unit"
            ) {
                return Err("typed primitive name is unknown".to_string());
            }
        }
        "literal" => {
            exact_fields(value, &["kind", "name", "value"])?;
            let name = string_field(value, "name")?;
            let literal = value_field(value, "value")?;
            let valid = matches!(
                (name, literal),
                ("string" | "int" | "float", JsonValue::String(_))
                    | ("bool", JsonValue::Bool(_))
                    | ("null", JsonValue::Null)
            );
            if !valid {
                return Err("typed literal payload does not match its name".to_string());
            }
        }
        "union" => {
            exact_fields(value, &["kind", "members"])?;
            for member in array_field(value, "members")? {
                validate_semantic_type(member)?;
            }
        }
        "record" => {
            exact_fields(value, &["fields", "kind"])?;
            for field in array_field(value, "fields")? {
                exact_fields(field, &["name", "type"])?;
                let _ = string_field(field, "name")?;
                validate_semantic_type(value_field(field, "type")?)?;
            }
        }
        "constructor" => {
            exact_fields(value, &["arguments", "constructor", "kind"])?;
            if !matches!(
                string_field(value, "constructor")?,
                "Array" | "Map" | "Set" | "Option" | "Result" | "Range"
            ) {
                return Err("typed constructor identity is unknown".to_string());
            }
            for argument in array_field(value, "arguments")? {
                validate_semantic_type(argument)?;
            }
        }
        "function" => {
            exact_fields(value, &["kind", "parameters", "result", "variadic"])?;
            for parameter in array_field(value, "parameters")? {
                validate_semantic_type(parameter)?;
            }
            validate_semantic_type(value_field(value, "result")?)?;
            match value_field(value, "variadic")? {
                JsonValue::Null => {}
                value => validate_semantic_type(value)?,
            }
        }
        "foreign" | "enum" | "nominal-record" | "newtype" => {
            exact_fields(value, &["arguments", "identity", "kind"])?;
            if string_field(value, "identity")?.is_empty() {
                return Err("typed identity must be non-empty".to_string());
            }
            for argument in array_field(value, "arguments")? {
                validate_semantic_type(argument)?;
            }
        }
        "rigid" => {
            exact_fields(value, &["kind", "name", "origin"])?;
            if string_field(value, "name")?.is_empty() || string_field(value, "origin")?.is_empty()
            {
                return Err("typed rigid variable needs a stable name and origin".to_string());
            }
        }
        "inference-variable" => {
            return Err("clean typed projection retained an inference-local variable".to_string());
        }
        "template" | "file" | "json-value" | "bytes" | "byte-buffer" | "path" | "regex"
        | "match" | "toml-value" | "url" | "date" | "big-int" | "decimal" | "rounding-mode"
        | "unknown" => exact_fields(value, &["kind"])?,
        other => return Err(format!("unknown semantic type kind `{other}`")),
    }
    Ok(())
}

fn semantic_type_has_hole(value: &JsonValue) -> Result<bool, String> {
    Ok(match string_field(value, "kind")? {
        "unknown" | "inference-variable" => true,
        "union" => array_field(value, "members")?
            .iter()
            .try_fold(false, |found, value| {
                Ok::<_, String>(found || semantic_type_has_hole(value)?)
            })?,
        "record" => array_field(value, "fields")?
            .iter()
            .try_fold(false, |found, field| {
                Ok::<_, String>(found || semantic_type_has_hole(value_field(field, "type")?)?)
            })?,
        "constructor" | "foreign" | "enum" | "nominal-record" | "newtype" => {
            array_field(value, "arguments")?
                .iter()
                .try_fold(false, |found, value| {
                    Ok::<_, String>(found || semantic_type_has_hole(value)?)
                })?
        }
        "function" => {
            let parameters = array_field(value, "parameters")?
                .iter()
                .try_fold(false, |found, value| {
                    Ok::<_, String>(found || semantic_type_has_hole(value)?)
                })?;
            let result = semantic_type_has_hole(value_field(value, "result")?)?;
            let variadic = match value_field(value, "variadic")? {
                JsonValue::Null => false,
                value => semantic_type_has_hole(value)?,
            };
            parameters || result || variadic
        }
        _ => false,
    })
}

fn validate_call_plan(value: &JsonValue) -> Result<(), String> {
    exact_fields(value, &["arguments", "binding", "callee", "evaluation"])?;
    let arguments = array_field(value, "arguments")?;
    for argument in arguments {
        exact_fields(argument, &["binding", "sourceIndex", "span"])?;
        let binding = value_field(argument, "binding")?;
        match string_field(binding, "kind")? {
            "named" => {
                exact_fields(binding, &["kind", "name"])?;
                let _ = string_field(binding, "name")?;
            }
            "positional" | "spread" | "inserted-lead" => {
                exact_fields(binding, &["kind"])?;
            }
            other => return Err(format!("unknown typed argument binding `{other}`")),
        }
        match value_field(argument, "sourceIndex")? {
            JsonValue::Null => {}
            JsonValue::Number(number) if number.int.is_some_and(|value| value >= 0) => {}
            _ => return Err("typed argument sourceIndex must be unsigned or null".to_string()),
        }
        exact_fields(value_field(argument, "span")?, &["hi", "lo", "sourceId"])?;
    }

    let callee = value_field(value, "callee")?;
    match string_field(callee, "kind")? {
        "value" => exact_fields(callee, &["kind"])?,
        "member" => {
            exact_fields(
                callee,
                &["class", "kind", "method", "optional", "shadowFirst"],
            )?;
            if !matches!(
                string_field(callee, "class")?,
                "higher-order" | "lazy-callback" | "mutator" | "resource" | "other"
            ) {
                return Err("typed member class is unknown".to_string());
            }
            let _ = string_field(callee, "method")?;
            let _ = boolean_field(callee, "optional")?;
            let _ = boolean_field(callee, "shadowFirst")?;
        }
        "pipe" => {
            exact_fields(callee, &["kind", "stageMethod"])?;
            if !matches!(
                value_field(callee, "stageMethod")?,
                JsonValue::Null | JsonValue::String(_)
            ) {
                return Err("typed pipe stageMethod must be a string or null".to_string());
            }
        }
        other => return Err(format!("unknown typed callee plan `{other}`")),
    }

    for (expected_index, step) in array_field(value, "evaluation")?.iter().enumerate() {
        match string_field(step, "kind")? {
            "callee" | "receiver" | "optional-guard" | "pipe-lead" => {
                exact_fields(step, &["kind"])?
            }
            "argument" => {
                exact_fields(step, &["argumentIndex", "kind"])?;
                let index = unsigned_field(step, "argumentIndex")? as usize;
                if index >= arguments.len() {
                    return Err(format!(
                        "typed evaluation step {expected_index} references missing argument {index}"
                    ));
                }
            }
            other => return Err(format!("unknown typed evaluation step `{other}`")),
        }
    }

    let binding = value_field(value, "binding")?;
    match string_field(binding, "kind")? {
        "runtime" => exact_fields(binding, &["kind"])?,
        "unsupported-shape" => {
            exact_fields(binding, &["kind", "reason"])?;
            if string_field(binding, "reason")?.is_empty() {
                return Err("typed unsupported shape needs a reason".to_string());
            }
        }
        other => return Err(format!("unknown typed call binding `{other}`")),
    }
    Ok(())
}

fn validate_stage2_fixed_point_assessment(value: &JsonValue) -> Result<(), String> {
    exact_fields(
        value,
        &[
            "artifactManifest",
            "assessment",
            "compilerSourceSetId",
            "crossRouteGeneratedSourceComparison",
            "nativeBinary",
            "programImageReextraction",
            "schema",
            "semanticProjection",
            "stage0Generation",
            "stage2Derivation",
            "status",
            "storageCorrection",
            "supersedes",
        ],
    )?;
    if string_field(value, "schema")? != STAGE2_FIXED_POINT_SCHEMA
        || string_field(value, "assessment")? != "generated-source-stage2-fixed-point"
        || string_field(value, "status")? != "not-established"
    {
        return Err("Stage 2 assessment identity is invalid".to_string());
    }
    validate_sha256(
        string_field(value, "compilerSourceSetId")?,
        "Stage 2 assessment compiler source set",
    )?;

    let artifact = value_field(value, "artifactManifest")?;
    exact_fields(artifact, &["path", "schema", "sha256"])?;
    if string_field(artifact, "schema")? != "topaz.compiler.generated-artifacts/v1"
        || string_field(artifact, "path")?
            != "compiler/generated/topaz_compiler_generated_artifacts.json"
    {
        return Err("Stage 2 artifact manifest identity is invalid".to_string());
    }
    validate_sha256(
        string_field(artifact, "sha256")?,
        "Stage 2 artifact manifest",
    )?;

    let supersedes = value_field(value, "supersedes")?;
    exact_fields(
        supersedes,
        &["artifactPath", "artifactSha256", "reason", "schema"],
    )?;
    if string_field(supersedes, "schema")? != "topaz.compiler.stage2-fixed-point/v1"
        || string_field(supersedes, "artifactPath")?
            != "compiler/generated/stage2/topaz_stage2_fixed_point.json"
        || string_field(supersedes, "reason")?
            != "The v1 assessment treated a Stage2-tagged alias of the C1 payload as evidence of an independently derived C2."
    {
        return Err("Stage 2 assessment supersession is invalid".to_string());
    }
    validate_sha256(
        string_field(supersedes, "artifactSha256")?,
        "superseded Stage 2 assessment",
    )?;

    let storage = value_field(value, "storageCorrection")?;
    exact_fields(storage, &["reason"])?;
    if string_field(storage, "reason")?
        != "The undeployed v2 record now identifies generated Rust by manifest digest and byte length because the generated source is no longer stored in Git."
    {
        return Err("Stage 2 storage correction is invalid".to_string());
    }

    let stage0 = value_field(value, "stage0Generation")?;
    exact_fields(
        stage0,
        &[
            "executionDisposition",
            "manifestIdentityDisposition",
            "outputBytes",
            "outputSha256",
            "producer",
        ],
    )?;
    if string_field(stage0, "executionDisposition")? != "fresh"
        || string_field(stage0, "producer")? != "rust-stage0-direct"
        || string_field(stage0, "manifestIdentityDisposition")? != "match"
        || unsigned_field(stage0, "outputBytes")? == 0
    {
        return Err("Stage 0 manifest-identity assessment is invalid".to_string());
    }
    validate_sha256(
        string_field(stage0, "outputSha256")?,
        "Stage 0 generated Rust",
    )?;

    let cross_route = value_field(value, "crossRouteGeneratedSourceComparison")?;
    exact_fields(
        cross_route,
        &[
            "comparison",
            "compilationRequestSha256",
            "disposition",
            "executionDisposition",
            "generatedRustBytes",
            "generatedRustSha256",
            "leftProducer",
            "normalization",
            "rightProducer",
        ],
    )?;
    if string_field(cross_route, "executionDisposition")? != "fresh"
        || string_field(cross_route, "leftProducer")? != "rust-stage0-direct"
        || string_field(cross_route, "rightProducer")? != "checked-in-program-image"
        || string_field(cross_route, "comparison")? != "raw-byte-equality"
        || string_field(cross_route, "normalization")? != "none"
        || string_field(cross_route, "disposition")? != "equal"
        || unsigned_field(cross_route, "generatedRustBytes")?
            != unsigned_field(stage0, "outputBytes")?
        || string_field(cross_route, "generatedRustSha256")?
            != string_field(stage0, "outputSha256")?
    {
        return Err("cross-route generated-source assessment is invalid".to_string());
    }
    validate_sha256(
        string_field(cross_route, "compilationRequestSha256")?,
        "cross-route compilation request",
    )?;
    validate_sha256(
        string_field(cross_route, "generatedRustSha256")?,
        "cross-route generated Rust",
    )?;

    let reextraction = value_field(value, "programImageReextraction")?;
    exact_fields(
        reextraction,
        &[
            "checkedInProgramImageSha256",
            "checkedInTargetFactsSha256",
            "comparison",
            "disposition",
            "executionDisposition",
            "inputGeneratedRustSha256",
            "normalization",
            "outputProgramImageSha256",
            "outputTargetFactsSha256",
        ],
    )?;
    if string_field(reextraction, "executionDisposition")? != "fresh"
        || string_field(reextraction, "comparison")? != "raw-byte-equality"
        || string_field(reextraction, "normalization")? != "none"
        || string_field(reextraction, "disposition")? != "equal"
        || string_field(reextraction, "inputGeneratedRustSha256")?
            != string_field(stage0, "outputSha256")?
        || string_field(reextraction, "outputProgramImageSha256")?
            != string_field(reextraction, "checkedInProgramImageSha256")?
        || string_field(reextraction, "outputTargetFactsSha256")?
            != string_field(reextraction, "checkedInTargetFactsSha256")?
    {
        return Err("program-image re-extraction assessment is invalid".to_string());
    }
    for field in [
        "checkedInProgramImageSha256",
        "checkedInTargetFactsSha256",
        "inputGeneratedRustSha256",
        "outputProgramImageSha256",
        "outputTargetFactsSha256",
    ] {
        validate_sha256(string_field(reextraction, field)?, field)?;
    }

    let derivation = value_field(value, "stage2Derivation")?;
    exact_fields(
        derivation,
        &[
            "declaredBuildStep",
            "declaredBuildStepDisposition",
            "independentC2Derivation",
            "note",
            "stage2ExecutionDisposition",
        ],
    )?;
    if string_field(derivation, "declaredBuildStep")? != "rust_build(R1)"
        || string_field(derivation, "declaredBuildStepDisposition")? != "not-performed"
        || string_field(derivation, "stage2ExecutionDisposition")? != "not-performed"
        || string_field(derivation, "independentC2Derivation")? != "not-established"
        || string_field(derivation, "note")?
            != "The current verifier does not implement or evidence a fresh build-from-R1 -> C2 -> R2 derivation."
    {
        return Err("Stage 2 derivation assessment is invalid".to_string());
    }

    let semantic = value_field(value, "semanticProjection")?;
    exact_fields(semantic, &["disposition", "note"])?;
    if string_field(semantic, "disposition")? != "not-performed"
        || string_field(semantic, "note")?
            != "No independently derived C1/C2 canonical observation records were compared."
    {
        return Err("Stage 2 semantic-projection assessment is invalid".to_string());
    }
    let native = value_field(value, "nativeBinary")?;
    exact_fields(native, &["disposition"])?;
    if string_field(native, "disposition")? != "not-compared" {
        return Err("Stage 2 native-binary disposition is invalid".to_string());
    }
    Ok(())
}

pub(super) fn validate_schema_rows(
    path: &str,
    schema: &str,
    values: &[JsonValue],
) -> Result<(), String> {
    match path {
        "request.json" => {
            exact_fields(
                &values[0],
                &[
                    "budgets",
                    "entry",
                    "facts",
                    "languageMode",
                    "mounts",
                    "package",
                    "requestedSchemas",
                    "schema",
                    "terminalPhase",
                ],
            )?;
        }
        "response.json" => {
            exact_fields(
                &values[0],
                &[
                    "highestCompletedPhase",
                    "phases",
                    "projectionDigests",
                    "requestDigest",
                    "schema",
                    "status",
                ],
            )?;
        }
        "provenance.json" => {
            exact_fields(
                &values[0],
                &[
                    "buildInputs",
                    "defaultEngine",
                    "engine",
                    "generatedSourceFixedPoint",
                    "languageMode",
                    "nativeBinaryReproducibility",
                    "producerStage",
                    "productVersion",
                    "resultStage",
                    "schema",
                    "semanticFixedPoint",
                ],
            )?;
            let build = value_field(&values[0], "buildInputs")?;
            exact_fields(
                build,
                &[
                    "bootstrapProfileSha256",
                    "buildProfile",
                    "buildTarget",
                    "cargoLockSha256",
                    "compilerSourceFiles",
                    "compilerSourceSetId",
                    "runtimeTemplates",
                    "rustToolchainSha256",
                    "schemaRegistrySha256",
                    "stage0Seed",
                    "vendorPackages",
                    "vendorSetId",
                ],
            )?;
            for field in [
                "bootstrapProfileSha256",
                "cargoLockSha256",
                "compilerSourceSetId",
                "rustToolchainSha256",
                "schemaRegistrySha256",
                "vendorSetId",
            ] {
                validate_sha256(string_field(build, field)?, field)?;
            }
            if string_field(build, "buildProfile")?.is_empty()
                || string_field(build, "buildTarget")?.is_empty()
            {
                return Err("provenance build profile and target must be nonempty".to_string());
            }
            let mut previous_source = None::<&str>;
            for source in array_field(build, "compilerSourceFiles")? {
                exact_fields(source, &["byteLength", "path", "sha256"])?;
                let path = string_field(source, "path")?;
                if previous_source.is_some_and(|previous| previous >= path) {
                    return Err(
                        "provenance compiler source files are not strictly ordered".to_string()
                    );
                }
                previous_source = Some(path);
                validate_sha256(string_field(source, "sha256")?, "compiler source sha256")?;
                let _ = unsigned_field(source, "byteLength")?;
            }
            if previous_source.is_none() {
                return Err("provenance compiler source set is empty".to_string());
            }
            let mut previous_template = None::<&str>;
            for template in array_field(build, "runtimeTemplates")? {
                exact_fields(template, &["identity", "sha256"])?;
                let identity = string_field(template, "identity")?;
                if previous_template.is_some_and(|previous| previous >= identity) {
                    return Err("provenance runtime templates are not strictly ordered".to_string());
                }
                previous_template = Some(identity);
                validate_sha256(string_field(template, "sha256")?, "runtime template sha256")?;
            }
            let seed = value_field(build, "stage0Seed")?;
            exact_fields(seed, &["compilerSourceSetId", "recoverySchema"])?;
            validate_sha256(
                string_field(seed, "compilerSourceSetId")?,
                "Stage 0 compiler source-set identity",
            )?;
            if string_field(seed, "recoverySchema")? != "topaz.stage0-recovery/v1" {
                return Err("provenance Stage 0 recovery schema is not v1".to_string());
            }
            let mut previous_vendor = None::<&str>;
            for package in array_field(build, "vendorPackages")? {
                exact_fields(package, &["fileCount", "identity", "sha256"])?;
                let identity = string_field(package, "identity")?;
                if previous_vendor.is_some_and(|previous| previous >= identity) {
                    return Err("provenance vendor packages are not strictly ordered".to_string());
                }
                previous_vendor = Some(identity);
                validate_sha256(string_field(package, "sha256")?, "vendor package sha256")?;
                if unsigned_field(package, "fileCount")? == 0 {
                    return Err("provenance vendor package has no files".to_string());
                }
            }
            if previous_vendor.is_none() {
                return Err("provenance vendor set is empty".to_string());
            }
        }
        "stage1-product.json" => {
            exact_fields(
                &values[0],
                &[
                    "comparison",
                    "compilerFiles",
                    "compilerSourceSetId",
                    "defaultEngine",
                    "exchangeSchema",
                    "fixedPoint",
                    "generatedC1ManifestSha256",
                    "generatedC1RustBytes",
                    "generatedC1RustSha256",
                    "irSchema",
                    "languageMode",
                    "producerStage",
                    "productVersion",
                    "recovery",
                    "resultStage",
                    "runtimeTemplate",
                    "schema",
                    "selectedEngine",
                    "target",
                    "targetCompilerFallback",
                ],
            )?;
            if string_field(&values[0], "schema")? != STAGE1_PRODUCT_SCHEMA
                || string_field(&values[0], "selectedEngine")? != "topaz-stage1"
                || string_field(&values[0], "defaultEngine")? != "rust-stage0"
                || unsigned_field(&values[0], "producerStage")? != 1
                || unsigned_field(&values[0], "resultStage")? != 1
                || string_field(&values[0], "fixedPoint")? != "not-run"
                || boolean_field(&values[0], "targetCompilerFallback")?
            {
                return Err("Stage 1 product identity is invalid".to_string());
            }
            for field in [
                "compilerSourceSetId",
                "generatedC1ManifestSha256",
                "generatedC1RustSha256",
            ] {
                validate_sha256(string_field(&values[0], field)?, field)?;
            }
            let _ = unsigned_field(&values[0], "generatedC1RustBytes")?;
            let mut previous = None::<&str>;
            for file in array_field(&values[0], "compilerFiles")? {
                exact_fields(file, &["byteLength", "path", "sha256"])?;
                let path = string_field(file, "path")?;
                if previous.is_some_and(|value| value >= path) {
                    return Err("Stage 1 compiler files are not strictly ordered".to_string());
                }
                previous = Some(path);
                let _ = unsigned_field(file, "byteLength")?;
                validate_sha256(string_field(file, "sha256")?, "Stage 1 compiler file")?;
            }
            if previous.is_none() {
                return Err("Stage 1 compiler source set is empty".to_string());
            }
            let recovery = value_field(&values[0], "recovery")?;
            exact_fields(
                recovery,
                &["manifestSha256", "sourceArchiveSha256", "version"],
            )?;
            validate_sha256(
                string_field(recovery, "manifestSha256")?,
                "Stage 0 recovery manifest",
            )?;
            validate_sha256(
                string_field(recovery, "sourceArchiveSha256")?,
                "Stage 0 recovery archive",
            )?;
            let runtime = value_field(&values[0], "runtimeTemplate")?;
            exact_fields(runtime, &["identity", "sha256"])?;
            validate_sha256(string_field(runtime, "sha256")?, "Stage 1 runtime template")?;
            let target = value_field(&values[0], "target")?;
            exact_fields(
                target,
                &["generatedRustBytes", "generatedRustSha256", "sourceSetId"],
            )?;
            validate_sha256(string_field(target, "sourceSetId")?, "target source set")?;
            validate_sha256(
                string_field(target, "generatedRustSha256")?,
                "target generated Rust",
            )?;
            let _ = unsigned_field(target, "generatedRustBytes")?;
            let comparison = value_field(&values[0], "comparison")?;
            exact_fields(comparison, &["generatedSource", "provenance", "semantic"])?;
        }
        "stage2-product.json" => {
            exact_fields(
                &values[0],
                &[
                    "c1",
                    "c2",
                    "compilerFiles",
                    "compilerSourceSetId",
                    "defaultEngine",
                    "exchangeSchema",
                    "fixedPoint",
                    "generatedPayloadSchema",
                    "irSchema",
                    "languageMode",
                    "producerStage",
                    "productVersion",
                    "recoveryEngine",
                    "resultStage",
                    "runtimeTemplate",
                    "schema",
                    "selectedEngine",
                    "selfSource",
                    "target",
                    "targetCompilerFallback",
                ],
            )?;
            let self_source = boolean_field(&values[0], "selfSource")?;
            if string_field(&values[0], "schema")? != STAGE2_PRODUCT_SCHEMA
                || string_field(&values[0], "selectedEngine")? != "topaz-stage2"
                || string_field(&values[0], "defaultEngine")? != "rust-stage0"
                || string_field(&values[0], "recoveryEngine")? != "rust-stage0"
                || unsigned_field(&values[0], "producerStage")? != 2
                || unsigned_field(&values[0], "resultStage")? != 2
                || string_field(&values[0], "fixedPoint")?
                    != if self_source {
                        "not-established"
                    } else {
                        "not-run"
                    }
                || boolean_field(&values[0], "targetCompilerFallback")?
            {
                return Err("Stage 2 product identity is invalid".to_string());
            }
            validate_sha256(
                string_field(&values[0], "compilerSourceSetId")?,
                "Stage 2 compiler source set",
            )?;
            let mut previous = None::<&str>;
            for file in array_field(&values[0], "compilerFiles")? {
                exact_fields(file, &["byteLength", "path", "sha256"])?;
                let path = string_field(file, "path")?;
                if previous.is_some_and(|value| value >= path) {
                    return Err("Stage 2 compiler files are not strictly ordered".to_string());
                }
                previous = Some(path);
                let _ = unsigned_field(file, "byteLength")?;
                validate_sha256(string_field(file, "sha256")?, "Stage 2 compiler file")?;
            }
            if previous.is_none() {
                return Err("Stage 2 compiler source set is empty".to_string());
            }
            for (field, source_producer, executable, source_stage, executable_stage) in [
                ("c1", "topaz-interpreted-bootstrap", "topaz-stage1", 0, 1),
                ("c2", "topaz-stage1", "topaz-stage2", 1, 2),
            ] {
                let image = value_field(&values[0], field)?;
                exact_fields(
                    image,
                    &[
                        "executable",
                        "executableStage",
                        "generatedRustBytes",
                        "generatedRustSha256",
                        "manifestSha256",
                        "programImageSha256",
                        "sourceProducer",
                        "sourceProducerStage",
                    ],
                )?;
                if string_field(image, "sourceProducer")? != source_producer
                    || string_field(image, "executable")? != executable
                    || unsigned_field(image, "sourceProducerStage")? != source_stage
                    || unsigned_field(image, "executableStage")? != executable_stage
                {
                    return Err(format!("Stage 2 product {field} identity is invalid"));
                }
                let _ = unsigned_field(image, "generatedRustBytes")?;
                for digest in [
                    "generatedRustSha256",
                    "manifestSha256",
                    "programImageSha256",
                ] {
                    validate_sha256(string_field(image, digest)?, digest)?;
                }
            }
            let runtime = value_field(&values[0], "runtimeTemplate")?;
            exact_fields(runtime, &["identity", "sha256"])?;
            if string_field(runtime, "identity")? != "compiler-ir-table/v2" {
                return Err("Stage 2 runtime template identity is invalid".to_string());
            }
            validate_sha256(string_field(runtime, "sha256")?, "Stage 2 runtime template")?;
            let target = value_field(&values[0], "target")?;
            exact_fields(
                target,
                &["generatedRustBytes", "generatedRustSha256", "sourceSetId"],
            )?;
            let _ = unsigned_field(target, "generatedRustBytes")?;
            validate_sha256(
                string_field(target, "generatedRustSha256")?,
                "Stage 2 target generated Rust",
            )?;
            validate_sha256(
                string_field(target, "sourceSetId")?,
                "Stage 2 target source set",
            )?;
        }
        "stage2-fixed-point.json" => validate_stage2_fixed_point_assessment(&values[0])?,
        "source-set.jsonl" => {
            let mut expected_source_ordinal = 0_u64;
            let mut host_facts_started = false;
            let mut previous_host_key = None::<(u8, String, String)>;
            for value in values {
                match string_field(value, "rowKind")? {
                    "source" => {
                        if host_facts_started {
                            return Err(
                                "source-set source row appears after host facts".to_string()
                            );
                        }
                        exact_fields(
                            value,
                            &[
                                "byteLength",
                                "contentSha256",
                                "entry",
                                "member",
                                "module",
                                "originRole",
                                "path",
                                "rowKind",
                                "schema",
                                "sourceId",
                                "sourceOrdinal",
                            ],
                        )?;
                        if unsigned_field(value, "sourceOrdinal")? != expected_source_ordinal {
                            return Err("source-set ordinals are not contiguous".to_string());
                        }
                        expected_source_ordinal += 1;
                    }
                    "host-fact" => {
                        host_facts_started = true;
                        exact_fields(
                            value,
                            &[
                                "kind",
                                "logicalPath",
                                "mountId",
                                "rowKind",
                                "schema",
                                "value",
                            ],
                        )?;
                        let rank = match string_field(value, "kind")? {
                            "read-source" => 0,
                            "list-directory" => 1,
                            "physical-containment" => 2,
                            other => {
                                return Err(format!("unknown host-fact query kind `{other}`"));
                            }
                        };
                        let key = (
                            rank,
                            string_field(value, "mountId")?.to_string(),
                            string_field(value, "logicalPath")?.to_string(),
                        );
                        if previous_host_key
                            .as_ref()
                            .is_some_and(|previous| previous >= &key)
                        {
                            return Err("source-set host facts are not strictly sorted".to_string());
                        }
                        previous_host_key = Some(key);
                    }
                    other => return Err(format!("unknown source-set row kind `{other}`")),
                }
            }
        }
        "tokens.jsonl" => {
            let mut previous_group = None::<(u64, u8)>;
            let mut expected_ordinal = 0_u64;
            for value in values {
                exact_fields(
                    value,
                    &[
                        "kind",
                        "ordinal",
                        "schema",
                        "sourceId",
                        "sourceOrdinal",
                        "span",
                        "spelling",
                        "stream",
                        "synthetic",
                    ],
                )?;
                let stream_rank = match string_field(value, "stream")? {
                    "raw" => 0,
                    "layout" => 1,
                    other => return Err(format!("unknown token stream `{other}`")),
                };
                let group = (unsigned_field(value, "sourceOrdinal")?, stream_rank);
                match previous_group {
                    Some(previous) if group < previous => {
                        return Err("token groups are not in canonical order".to_string());
                    }
                    Some(previous) if group == previous => {}
                    _ => {
                        expected_ordinal = 0;
                        previous_group = Some(group);
                    }
                }
                if unsigned_field(value, "ordinal")? != expected_ordinal {
                    return Err("token ordinals are not contiguous".to_string());
                }
                expected_ordinal += 1;
            }
        }
        "ast.jsonl" => {
            for value in values {
                allowed_fields(
                    value,
                    &[
                        "field",
                        "index",
                        "kind",
                        "nodeId",
                        "parentNodeId",
                        "schema",
                        "sourceId",
                        "span",
                        "spelling",
                    ],
                    &[
                        "collection",
                        "exported",
                        "field",
                        "floatBits",
                        "inclusive",
                        "index",
                        "kind",
                        "mutable",
                        "nodeId",
                        "operator",
                        "parentNodeId",
                        "schema",
                        "sourceId",
                        "span",
                        "spelling",
                        "unit",
                        "value",
                        "valueDecimal",
                        "variadic",
                    ],
                )?;
            }
        }
        "resolved.jsonl" => {
            let mut previous_rank = 0_u8;
            let mut expected_module = 0_u64;
            let mut expected_edge = 0_u64;
            for value in values {
                let row_kind = string_field(value, "rowKind")?;
                let rank = match row_kind {
                    "module" => 0,
                    "import-edge" => 1,
                    "scope" => 2,
                    "declaration" => 3,
                    "reference" => 4,
                    "export" => 5,
                    _ => u8::MAX,
                };
                if rank < previous_rank {
                    return Err("resolved row kinds are not in canonical order".to_string());
                }
                previous_rank = rank;
                match row_kind {
                    "module" => exact_fields(
                        value,
                        &[
                            "entry",
                            "extern",
                            "externIdentity",
                            "generatedStd",
                            "identity",
                            "initializationOrdinal",
                            "path",
                            "rowKind",
                            "schema",
                            "sourceId",
                        ],
                    )
                    .and_then(|()| {
                        if unsigned_field(value, "initializationOrdinal")? != expected_module {
                            return Err("resolved module ordinals are not contiguous".to_string());
                        }
                        expected_module += 1;
                        Ok(())
                    })?,
                    "import-edge" => {
                        exact_fields(value, &["from", "ordinal", "rowKind", "schema", "to"])?;
                        if unsigned_field(value, "ordinal")? != expected_edge {
                            return Err(
                                "resolved import-edge ordinals are not contiguous".to_string()
                            );
                        }
                        expected_edge += 1;
                    }
                    "scope" => exact_fields(
                        value,
                        &[
                            "kind",
                            "ownerNodeId",
                            "parentScopeId",
                            "rowKind",
                            "schema",
                            "scopeId",
                            "sourceId",
                            "span",
                        ],
                    )?,
                    "declaration" => exact_fields(
                        value,
                        &[
                            "declarationKind",
                            "declarationNodeId",
                            "exported",
                            "name",
                            "namespace",
                            "nominalIdentity",
                            "rowKind",
                            "schema",
                            "scopeId",
                            "sourceId",
                            "span",
                            "symbolId",
                            "targetModule",
                            "targetName",
                        ],
                    )?,
                    "reference" => exact_fields(
                        value,
                        &[
                            "name",
                            "namespace",
                            "referenceNodeId",
                            "role",
                            "rowKind",
                            "schema",
                            "scopeId",
                            "sourceId",
                            "span",
                            "targetModule",
                            "targetName",
                            "targetNamespace",
                            "targetSymbolId",
                        ],
                    )?,
                    "export" => exact_fields(
                        value,
                        &[
                            "name",
                            "namespace",
                            "rowKind",
                            "schema",
                            "sourceId",
                            "symbolId",
                        ],
                    )?,
                    other => return Err(format!("unknown resolved row kind `{other}`")),
                }
            }
        }
        "typed.jsonl" => {
            let mut previous_rank = 0_u8;
            for value in values {
                let row_kind = string_field(value, "rowKind")?;
                let rank = match row_kind {
                    "node" => 0,
                    "call" => 1,
                    "capture" => 2,
                    other => return Err(format!("unknown typed row kind `{other}`")),
                };
                if rank < previous_rank {
                    return Err("typed row kinds are not in canonical order".to_string());
                }
                previous_rank = rank;
                match row_kind {
                    "node" => {
                        exact_fields(
                            value,
                            &[
                                "ambient", "nodeId", "nodeKind", "rowKind", "schema", "sourceId",
                                "span", "type",
                            ],
                        )?;
                        let ty = value_field(value, "type")?;
                        validate_semantic_type(ty)?;
                        if boolean_field(value, "ambient")? != semantic_type_has_hole(ty)? {
                            return Err(
                                "typed node ambient marker does not match its semantic type"
                                    .to_string(),
                            );
                        }
                    }
                    "call" => {
                        exact_fields(
                            value,
                            &[
                                "ambient",
                                "callNodeId",
                                "calleeNodeId",
                                "calleeSpan",
                                "calleeType",
                                "plan",
                                "resultType",
                                "rowKind",
                                "schema",
                                "sourceId",
                                "span",
                                "targetIdentity",
                            ],
                        )?;
                        let callee = value_field(value, "calleeType")?;
                        let result = value_field(value, "resultType")?;
                        validate_semantic_type(callee)?;
                        validate_semantic_type(result)?;
                        validate_call_plan(value_field(value, "plan")?)?;
                        let ambient = boolean_field(value, "ambient")?;
                        let callee_hole = semantic_type_has_hole(callee)?;
                        let result_hole = semantic_type_has_hole(result)?;
                        if ambient != (callee_hole || result_hole) {
                            return Err(format!(
                                "typed call ambient marker does not match its semantic types at {} {:?} ({}, target={}): ambient={ambient}, calleeHole={callee_hole}, resultHole={result_hole}",
                                string_field(value, "sourceId")?,
                                value_field(value, "span")?,
                                string_field(value, "callNodeId")?,
                                string_field(value, "targetIdentity")?,
                            ));
                        }
                    }
                    "capture" => {
                        exact_fields(
                            value,
                            &[
                                "ambient",
                                "closureNodeId",
                                "declarationNodeId",
                                "name",
                                "referenceNodeId",
                                "rowKind",
                                "schema",
                                "sourceId",
                                "span",
                                "type",
                            ],
                        )?;
                        let ty = value_field(value, "type")?;
                        validate_semantic_type(ty)?;
                        if boolean_field(value, "ambient")? != semantic_type_has_hole(ty)? {
                            return Err(
                                "typed capture ambient marker does not match its semantic type"
                                    .to_string(),
                            );
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        "lowered.jsonl" => {
            let mut previous_rank = 0_u8;
            let mut expected_module = 0_u64;
            for value in values {
                let row_kind = string_field(value, "rowKind")?;
                let rank = match row_kind {
                    "module" => 0,
                    "runtime-leaf" => 1,
                    "runtime-template" => 2,
                    "operation" => 3,
                    other => return Err(format!("unknown lowered row kind `{other}`")),
                };
                if rank < previous_rank {
                    return Err("lowered row kinds are not in canonical order".to_string());
                }
                previous_rank = rank;
                match row_kind {
                    "module" => {
                        exact_fields(
                            value,
                            &[
                                "entry",
                                "extern",
                                "identity",
                                "initializationOrdinal",
                                "operationIds",
                                "path",
                                "rowKind",
                                "schema",
                                "sourceId",
                            ],
                        )?;
                        if unsigned_field(value, "initializationOrdinal")? != expected_module {
                            return Err("lowered module ordinals are not contiguous".to_string());
                        }
                        expected_module += 1;
                    }
                    "runtime-leaf" => {
                        exact_fields(value, &["deterministic", "identity", "rowKind", "schema"])?;
                        let _ = boolean_field(value, "deterministic")?;
                    }
                    "runtime-template" => {
                        exact_fields(value, &["identity", "rowKind", "schema", "sha256"])?;
                    }
                    "operation" => {
                        exact_fields(
                            value,
                            &[
                                "binding",
                                "call",
                                "control",
                                "detail",
                                "kind",
                                "module",
                                "operands",
                                "operationId",
                                "parentOperationId",
                                "representation",
                                "role",
                                "rowKind",
                                "runtimeLeaf",
                                "schema",
                                "semanticType",
                                "span",
                            ],
                        )?;
                        if let value @ JsonValue::Object(_) = value_field(value, "call")? {
                            validate_call_plan(value)?;
                        }
                        if let value @ JsonValue::Object(_) = value_field(value, "semanticType")? {
                            validate_semantic_type(value)?;
                        }
                        if !matches!(
                            value_field(value, "representation")?,
                            JsonValue::Null | JsonValue::String(_)
                        ) {
                            return Err(
                                "lowered representation must be a string or null".to_string()
                            );
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        "rust-source.jsonl" => {
            if values.len() != 1 {
                return Err("Rust-source projection must contain one row".to_string());
            }
            let value = &values[0];
            exact_fields(
                value,
                &["byteLength", "rowKind", "schema", "sha256", "source"],
            )?;
            if string_field(value, "rowKind")? != "generated-source" {
                return Err("Rust-source row kind is invalid".to_string());
            }
            let source = string_field(value, "source")?;
            if unsigned_field(value, "byteLength")? != source.len() as u64
                || string_field(value, "sha256")? != sha256(source.as_bytes())
            {
                return Err("Rust-source bytes do not match their recorded digest".to_string());
            }
        }
        "diagnostics.jsonl" => {
            for (expected_ordinal, value) in values.iter().enumerate() {
                exact_fields(
                    value,
                    &[
                        "code",
                        "message",
                        "notes",
                        "ordinal",
                        "primary",
                        "producerPhase",
                        "profileRule",
                        "schema",
                        "secondary",
                        "severity",
                    ],
                )?;
                if unsigned_field(value, "ordinal")? != expected_ordinal as u64 {
                    return Err("diagnostic ordinals are not contiguous".to_string());
                }
            }
        }
        other => return Err(format!("unknown observation JSON member `{other}`")),
    }
    for value in values {
        if string_field(value, "schema")? != schema {
            return Err(format!("schema identity drifted in `{path}`"));
        }
    }
    Ok(())
}
