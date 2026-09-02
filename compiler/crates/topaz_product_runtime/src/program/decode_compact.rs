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

/// Admits one compact compiler image with the caller's exact magic and stage label.
pub fn parse_embedded_program(
    bytes: &[u8],
    expected_magic: &[u8],
    stage: &str,
) -> Result<Program, String> {
    let mut reader = ProgramReader::new(bytes);
    if reader.take(expected_magic.len())? != expected_magic {
        return Err(format!("{stage} compact table has the wrong magic"));
    }
    let operation_count = reader.index()?;
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
            lo,
            hi,
            kind,
            detail,
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
        let entry = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err("Stage 1 compact table boolean is invalid".to_string()),
        };
        let operations = reader.indexes()?;
        if operations
            .iter()
            .any(|operation| *operation >= operation_count)
        {
            return Err("Stage 1 compact table module operation is out of range".to_string());
        }
        modules.push(Module {
            identity,
            entry,
            operations,
        });
    }
    if reader.offset != reader.bytes.len() {
        return Err("Stage 1 compact table has trailing bytes".to_string());
    }
    let program = Program {
        modules,
        operations,
    };
    Ok(program)
}
