use super::{model::*, validate::*};

pub(crate) struct ProgramReader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) offset: usize,
}

impl<'a> ProgramReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| "Stage 1 compact table offset overflowed".to_string())?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "Stage 1 compact table is truncated".to_string())?;
        self.offset = end;
        Ok(value)
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn boolean(&mut self) -> Result<bool, String> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("Stage 1 compact table boolean is invalid".to_string()),
        }
    }

    pub(crate) fn u32(&mut self) -> Result<u32, String> {
        let mut encoded = [0_u8; 4];
        encoded.copy_from_slice(self.take(std::mem::size_of::<u32>())?);
        Ok(u32::from_le_bytes(encoded))
    }

    pub(crate) fn index(&mut self) -> Result<usize, String> {
        usize::try_from(self.u32()?)
            .map_err(|_| "Stage 1 compact table index exceeds usize".to_string())
    }

    pub(crate) fn string(&mut self) -> Result<String, String> {
        let length = self.index()?;
        std::str::from_utf8(self.take(length)?)
            .map(str::to_string)
            .map_err(|error| format!("Stage 1 compact table string is not UTF-8: {error}"))
    }

    pub(crate) fn indexes(&mut self) -> Result<Vec<usize>, String> {
        let length = self.index()?;
        if length > self.remaining() / std::mem::size_of::<u32>() {
            return Err("Stage 1 compact table index vector exceeds remaining bytes".to_string());
        }
        let mut values = Vec::with_capacity(length);
        for _ in 0..length {
            values.push(self.index()?);
        }
        Ok(values)
    }

    pub(crate) fn strings(&mut self) -> Result<Vec<String>, String> {
        let length = self.index()?;
        if length > self.remaining() / std::mem::size_of::<u32>() {
            return Err("Stage 1 compact table string vector exceeds remaining bytes".to_string());
        }
        let mut values = Vec::with_capacity(length);
        for _ in 0..length {
            values.push(self.string()?);
        }
        Ok(values)
    }
}

/// Admits a compact image with the caller's exact eight-byte magic, followed by
/// a supported version byte. Version 1 retains its historical decoding rules.
pub fn parse_embedded_program(
    bytes: &[u8],
    expected_magic: &[u8; 8],
    stage: &str,
) -> Result<Program, String> {
    let mut reader = ProgramReader::new(bytes);
    if reader.take(expected_magic.len())? != expected_magic {
        return Err(format!("{stage} compact table has the wrong magic"));
    }
    match reader.take(1)?[0] {
        1 => parse_embedded_program_v1(bytes, &bytes[..9], stage),
        2 => parse_embedded_program_v2(reader, stage),
        version => Err(format!(
            "{stage} compact table has unsupported version {version}"
        )),
    }
}

fn parse_embedded_program_v1(
    bytes: &[u8],
    expected_magic: &[u8],
    stage: &str,
) -> Result<Program, String> {
    let mut reader = ProgramReader::new(bytes);
    if reader.take(expected_magic.len())? != expected_magic {
        return Err(format!("{stage} compact table has the wrong magic"));
    }
    let first = reader.u32()?;
    let explicit_literals = first == topaz_value::LITERAL_TABLE_MARKER;
    let operation_count = if explicit_literals {
        if reader.remaining() < 8 + topaz_value::LITERAL_TABLE_ENCODING.len() {
            return Err(format!(
                "{stage} compact table operation count exceeds remaining bytes"
            ));
        }
        if reader.string()? != topaz_value::LITERAL_TABLE_ENCODING {
            return Err(format!("{stage} compact literal table encoding mismatch"));
        }
        reader.index()?
    } else {
        first as usize
    };
    const MIN_OPERATION_BYTES: usize = 9 * 4 + 2 * 4 + 2 * 4;
    if operation_count > reader.remaining() / MIN_OPERATION_BYTES {
        return Err(format!(
            "{stage} compact table operation count exceeds remaining bytes"
        ));
    }
    let mut operations = Vec::with_capacity(operation_count);
    let mut operation_ids = std::collections::BTreeSet::new();
    for _ in 0..operation_count {
        let id = reader.string()?;
        if !operation_ids.insert(id.clone()) {
            return Err(format!(
                "{stage} compact table duplicates operation id `{id}`"
            ));
        }
        let module = reader.string()?;
        let kind = reader.string()?;
        let detail = reader.string()?;
        let literal_encoding = if explicit_literals {
            topaz_value::IrLiteralEncoding::parse(&reader.string()?)?
        } else {
            topaz_value::IrLiteralEncoding::LegacySource
        };
        let reference_identity = reader.string()?;
        let binding_name = reader.string()?;
        let declaration_identity = reader.string()?;
        let call_target = reader.string()?;
        let call_method = reader.string()?;
        let lo = reader.u32()?;
        let hi = reader.u32()?;
        let operands = reader.indexes()?;
        if operands.iter().any(|operand| *operand >= operation_count) {
            return Err(format!(
                "{stage} compact table operation operand is out of range"
            ));
        }
        let operand_labels = reader.strings()?;
        if operands.len() != operand_labels.len() {
            return Err("Stage 1 compact table operand labels are misaligned".to_string());
        }
        let operation = Operation {
            id,
            module,
            source_ordinal: 0,
            lo,
            hi,
            kind,
            detail,
            literal_encoding,
            operands,
            operand_labels,
            semantic_type: String::new(),
            pattern_type: None,
            reference_identity,
            binding_name,
            declaration_identity,
            control_target: String::new(),
            call_target,
            call_callee_kind: String::new(),
            call_method,
            call_optional: false,
            call_shadow_first: false,
            call_stage_method: String::new(),
            call_arguments: Vec::new(),
            call_type_arguments: Vec::new(),
            call_evaluations: Vec::new(),
        };
        validate_operation_shape(&operation, stage, false)?;
        operations.push(operation);
    }
    let module_count = reader.index()?;
    const MIN_MODULE_BYTES: usize = 4 + 1 + 4;
    if module_count > reader.remaining() / MIN_MODULE_BYTES {
        return Err(format!(
            "{stage} compact table module count exceeds remaining bytes"
        ));
    }
    let mut modules = Vec::with_capacity(module_count);
    for _ in 0..module_count {
        let identity = reader.string()?;
        let flags = match reader.take(1)?[0] {
            flags @ 0..=3 => flags,
            _ => return Err("Stage 1 compact table module flags are invalid".to_string()),
        };
        let operations = reader.indexes()?;
        if operations
            .iter()
            .any(|operation| *operation >= operation_count)
        {
            return Err("Stage 1 compact table module operation is out of range".to_string());
        }
        modules.push(Module {
            generated_std: flags & 2 != 0,
            identity,
            source_ordinal: 0,
            entry: flags & 1 != 0,
            operations,
        });
    }
    if reader.offset != reader.bytes.len() {
        return Err("Stage 1 compact table has trailing bytes".to_string());
    }
    let program = Program::new(modules, operations);
    validate_length_admission(&program)?;
    Ok(program)
}

fn parse_embedded_program_v2(
    mut reader: ProgramReader<'_>,
    stage: &str,
) -> Result<Program, String> {
    if reader.u32()? != topaz_value::LITERAL_TABLE_MARKER
        || reader.string()? != topaz_value::LITERAL_TABLE_ENCODING
    {
        return Err(format!("{stage} compact literal table encoding mismatch"));
    }
    let operation_count = reader.index()?;
    // 14 strings, two spans, five vectors, and three booleans per operation.
    const MIN_OPERATION_BYTES: usize = 14 * 4 + 2 * 4 + 5 * 4 + 3;
    if operation_count > reader.remaining() / MIN_OPERATION_BYTES {
        return Err(format!(
            "{stage} compact table operation count exceeds remaining bytes"
        ));
    }
    let mut operations = Vec::with_capacity(operation_count);
    let mut namespace_imports = std::collections::BTreeMap::new();
    let mut selected_imports = SelectedImports::new();
    let mut operation_ids = std::collections::BTreeSet::new();
    for _ in 0..operation_count {
        let id = reader.string()?;
        if !operation_ids.insert(id.clone()) {
            return Err(format!(
                "{stage} compact table duplicates operation id `{id}`"
            ));
        }
        let module = reader.string()?;
        let kind = reader.string()?;
        let detail = reader.string()?;
        let literal_encoding = topaz_value::IrLiteralEncoding::parse(&reader.string()?)?;
        let reference_identity = reader.string()?;
        let binding_name = reader.string()?;
        let declaration_identity = reader.string()?;
        let call_target = reader.string()?;
        let call_method = reader.string()?;
        let lo = reader.u32()?;
        let hi = reader.u32()?;
        let operands = reader.indexes()?;
        if operands.iter().any(|operand| *operand >= operation_count) {
            return Err(format!(
                "{stage} compact table operation operand is out of range"
            ));
        }
        let operand_labels = reader.strings()?;
        if operands.len() != operand_labels.len() {
            return Err("Stage 1 compact table operand labels are misaligned".to_string());
        }
        let semantic_type = reader.string()?;
        let pattern_type = if kind == "pattern/typed-binding" && semantic_type.starts_with('[') {
            Some(super::decode_json::parse_runtime_type(
                &semantic_type,
                stage,
            )?)
        } else {
            None
        };
        let control_target = reader.string()?;
        let call_callee_kind = reader.string()?;
        let call_optional = reader.boolean()?;
        let call_shadow_first = reader.boolean()?;
        let call_stage_method = reader.string()?;
        let call_arguments = super::decode_json::parse_call_arguments(reader.strings()?, stage)?;
        let call_type_arguments = reader
            .strings()?
            .into_iter()
            .map(|encoded| super::decode_json::parse_runtime_type(&encoded, stage))
            .collect::<Result<Vec<_>, _>>()?;
        let call_evaluations =
            super::decode_json::parse_call_evaluations(reader.strings()?, stage)?;
        // JSON admission checks this field even though the runtime does not retain it.
        let _binding_mutable = reader.boolean()?;
        if kind == "import" {
            let imported = reader.string()?;
            for binding in reader.strings()? {
                let parts = binding.split('|').collect::<Vec<_>>();
                if let ["namespace-import", _, bound, _, _] = parts.as_slice() {
                    namespace_imports
                        .entry(module.clone())
                        .or_insert_with(std::collections::BTreeMap::new)
                        .insert((*bound).to_string(), imported.clone());
                } else if let ["selected-import", source, bound, _, _] = parts.as_slice() {
                    selected_imports.entry(module.clone()).or_default().insert(
                        (*bound).to_string(),
                        (imported.clone(), (*source).to_string()),
                    );
                }
            }
        }
        let operation = Operation {
            id,
            module,
            source_ordinal: 0,
            lo,
            hi,
            kind,
            detail,
            literal_encoding,
            operands,
            operand_labels,
            semantic_type,
            pattern_type,
            reference_identity,
            binding_name,
            declaration_identity,
            control_target,
            call_target,
            call_callee_kind,
            call_method,
            call_optional,
            call_shadow_first,
            call_stage_method,
            call_arguments,
            call_type_arguments,
            call_evaluations,
        };
        validate_operation_shape(&operation, stage, true)?;
        operations.push(operation);
    }
    super::decode_json::validate_compiler_call_plans(&operations)?;
    let module_count = reader.index()?;
    const MIN_MODULE_BYTES: usize = 4 + 1 + 4 + 4 + 4;
    if module_count > reader.remaining() / MIN_MODULE_BYTES {
        return Err(format!(
            "{stage} compact table module count exceeds remaining bytes"
        ));
    }
    let mut modules = Vec::with_capacity(module_count);
    let mut language_version = None;
    for _ in 0..module_count {
        let identity = reader.string()?;
        let flags = match reader.take(1)?[0] {
            flags @ 0..=3 => flags,
            _ => return Err("Stage 1 compact table module flags are invalid".to_string()),
        };
        let operations = reader.indexes()?;
        if operations
            .iter()
            .any(|operation| *operation >= operation_count)
        {
            return Err("Stage 1 compact table module operation is out of range".to_string());
        }
        let source_ordinal = reader.u32()?;
        let mode = reader.string()?;
        let version = mode
            .strip_prefix("topaz-")
            .and_then(topaz_value::LangVersion::parse_exact)
            .ok_or_else(|| {
                format!("{stage} compact table has unsupported languageMode `{mode}`")
            })?;
        if language_version.is_some_and(|selected| selected != version) {
            return Err("Stage 1 IR modules have different language modes".into());
        }
        language_version = Some(version);
        modules.push(Module {
            generated_std: flags & 2 != 0,
            identity,
            source_ordinal,
            entry: flags & 1 != 0,
            operations,
        });
    }
    if reader.offset != reader.bytes.len() {
        return Err("Stage 1 compact table has trailing bytes".to_string());
    }
    let mut program = Program::new(modules, operations);
    program.language_version = language_version.unwrap_or(topaz_value::LangVersion::V5_20);
    program.namespace_imports = namespace_imports;
    program.selected_imports = selected_imports;
    validate_length_admission(&program)?;
    Ok(program)
}
