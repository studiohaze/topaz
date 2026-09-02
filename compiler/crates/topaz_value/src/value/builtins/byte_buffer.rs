use super::super::*;

pub(in crate::value) fn byte_buffer_arg<'a>(
    value: &'a Value,
    operation: &str,
    span: Span,
) -> Result<&'a Rc<RefCell<Vec<u8>>>, RtError> {
    match value {
        Value::ByteBuffer(bytes) => Ok(bytes),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`ByteBuffer.{operation}` requires `ByteBuffer`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn byte_buffer_int(
    value: Value,
    label: &str,
    span: Span,
) -> Result<i64, RtError> {
    match value {
        Value::Int(value) => Ok(value),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`ByteBuffer` {label} must be `int`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn byte_buffer_index(
    value: Value,
    label: &str,
    span: Span,
) -> Result<usize, RtError> {
    let value = byte_buffer_int(value, label, span)?;
    byte_buffer_index_i64(value, label, span)
}

pub(in crate::value) fn byte_buffer_index_i64(
    value: i64,
    label: &str,
    span: Span,
) -> Result<usize, RtError> {
    usize::try_from(value).map_err(|_| {
        fault(
            codes::FAULT_INDEX,
            format!("`ByteBuffer` {label} must be non-negative"),
            span,
        )
    })
}

pub(in crate::value) fn byte_buffer_byte(value: Value, span: Span) -> Result<u8, RtError> {
    let value = byte_buffer_int(value, "byte value", span)?;
    byte_buffer_byte_i64(value, span)
}

pub(in crate::value) fn byte_buffer_byte_i64(value: i64, span: Span) -> Result<u8, RtError> {
    u8::try_from(value).map_err(|_| {
        fault(
            codes::GUARD_TYPE,
            "`ByteBuffer` byte value must be in 0..255",
            span,
        )
    })
}

pub(in crate::value) fn byte_buffer_range(
    start: Value,
    length: Value,
    buffer_length: usize,
    span: Span,
) -> Result<std::ops::Range<usize>, RtError> {
    let start = byte_buffer_int(start, "start", span)?;
    let length = byte_buffer_int(length, "length", span)?;
    byte_buffer_range_i64(start, length, buffer_length, span)
}

pub(in crate::value) fn byte_buffer_range_i64(
    start: i64,
    length: i64,
    buffer_length: usize,
    span: Span,
) -> Result<std::ops::Range<usize>, RtError> {
    let start = byte_buffer_index_i64(start, "start", span)?;
    let length = byte_buffer_index_i64(length, "length", span)?;
    if start > buffer_length || length > buffer_length.saturating_sub(start) {
        return Err(fault(
            codes::FAULT_INDEX,
            "`ByteBuffer` range is out of bounds",
            span,
        ));
    }
    Ok(start..start + length)
}

pub fn builtin_byte_buffer_allocate(
    length: Value,
    value: Option<Value>,
    span: Span,
) -> Result<Value, RtError> {
    let length = byte_buffer_index(length, "length", span)?;
    let value = byte_buffer_byte(value.unwrap_or(Value::Int(0)), span)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| {
        fault(
            codes::GUARD_TYPE,
            "`ByteBuffer.allocate` length cannot be allocated",
            span,
        )
    })?;
    bytes.resize(length, value);
    Ok(Value::ByteBuffer(Rc::new(RefCell::new(bytes))))
}

pub fn builtin_byte_buffer_from_bytes(value: Value, span: Span) -> Result<Value, RtError> {
    let bytes = bytes_arg(&value, "fromBytes", span)?;
    Ok(Value::ByteBuffer(Rc::new(RefCell::new(bytes.to_vec()))))
}

pub fn builtin_byte_buffer_length(recv: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Int(builtin_byte_buffer_length_i64(&recv, span)?))
}

/// Direct checked `ByteBuffer.length` leaf.
pub fn builtin_byte_buffer_length_i64(recv: &Value, span: Span) -> Result<i64, RtError> {
    let bytes = byte_buffer_arg(recv, "length", span)?;
    Ok(bytes.borrow().len() as i64)
}

/// Allocation-free semantic leaf for a proven `ByteBuffer.get` call.
///
/// The boxed emitter uses this after its exact-variant runtime gate so a hot
/// read does not clone the buffer handle or return through generic builtin
/// dispatch. Keeping the index validation and bounds fault here also keeps the
/// ordinary tagged wrapper and the direct lane byte-identical on every error.
pub fn builtin_byte_buffer_get_i64(recv: &Value, index: Value, span: Span) -> Result<i64, RtError> {
    let _ = byte_buffer_arg(recv, "get", span)?;
    let index = byte_buffer_index(index, "index", span)?;
    builtin_byte_buffer_get_raw_i64(recv, index as i64, span)
}

/// Allocation-free direct leaf over an already-proved integer index.
pub fn builtin_byte_buffer_get_raw_i64(
    recv: &Value,
    index: i64,
    span: Span,
) -> Result<i64, RtError> {
    let bytes = byte_buffer_arg(recv, "get", span)?;
    let index = byte_buffer_index_i64(index, "index", span)?;
    let value = bytes.borrow().get(index).copied().ok_or_else(|| {
        fault(
            codes::FAULT_INDEX,
            "`ByteBuffer.get` index is out of bounds",
            span,
        )
    })?;
    Ok(value as i64)
}

pub fn builtin_byte_buffer_get(recv: Value, index: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Int(builtin_byte_buffer_get_i64(&recv, index, span)?))
}

pub fn builtin_byte_buffer_set(
    recv: Value,
    index: Value,
    value: Value,
    span: Span,
) -> Result<Value, RtError> {
    let _ = byte_buffer_arg(&recv, "set", span)?;
    let index = byte_buffer_index(index, "index", span)?;
    let value = byte_buffer_byte(value, span)?;
    builtin_byte_buffer_set_i64(&recv, index as i64, value as i64, span)?;
    Ok(Value::Unit)
}

/// Direct checked `ByteBuffer.set` leaf. All validation precedes mutation.
pub fn builtin_byte_buffer_set_i64(
    recv: &Value,
    index: i64,
    value: i64,
    span: Span,
) -> Result<(), RtError> {
    let bytes = byte_buffer_arg(recv, "set", span)?;
    let index = byte_buffer_index_i64(index, "index", span)?;
    let value = byte_buffer_byte_i64(value, span)?;
    if index >= bytes.borrow().len() {
        return Err(fault(
            codes::FAULT_INDEX,
            "`ByteBuffer.set` index is out of bounds",
            span,
        ));
    }
    bytes.borrow_mut()[index] = value;
    Ok(())
}

pub fn builtin_byte_buffer_fill(
    recv: Value,
    start: Value,
    length: Value,
    value: Value,
    span: Span,
) -> Result<Value, RtError> {
    let bytes = byte_buffer_arg(&recv, "fill", span)?;
    let range = byte_buffer_range(start, length, bytes.borrow().len(), span)?;
    let value = byte_buffer_byte(value, span)?;
    builtin_byte_buffer_fill_i64(
        &recv,
        range.start as i64,
        range.len() as i64,
        value as i64,
        span,
    )?;
    Ok(Value::Unit)
}

/// Direct checked `ByteBuffer.fill` leaf. Range and byte validation are
/// complete before the first write.
pub fn builtin_byte_buffer_fill_i64(
    recv: &Value,
    start: i64,
    length: i64,
    value: i64,
    span: Span,
) -> Result<(), RtError> {
    let bytes = byte_buffer_arg(recv, "fill", span)?;
    let range = byte_buffer_range_i64(start, length, bytes.borrow().len(), span)?;
    let value = byte_buffer_byte_i64(value, span)?;
    bytes.borrow_mut()[range].fill(value);
    Ok(())
}

pub fn builtin_byte_buffer_copy(
    target: Value,
    source: Value,
    source_start: Value,
    target_start: Value,
    length: Value,
    span: Span,
) -> Result<Value, RtError> {
    let target_bytes = byte_buffer_arg(&target, "copy", span)?;
    let source_bytes = byte_buffer_arg(&source, "copy", span)?;
    let length = byte_buffer_index(length, "length", span)?;
    let source_start = byte_buffer_index(source_start, "source start", span)?;
    let target_start = byte_buffer_index(target_start, "target start", span)?;
    let source_length = source_bytes.borrow().len();
    let target_length = target_bytes.borrow().len();
    if source_start > source_length
        || length > source_length.saturating_sub(source_start)
        || target_start > target_length
        || length > target_length.saturating_sub(target_start)
    {
        return Err(fault(
            codes::FAULT_INDEX,
            "`ByteBuffer.copy` range is out of bounds",
            span,
        ));
    }
    builtin_byte_buffer_copy_i64(
        &target,
        &source,
        source_start as i64,
        target_start as i64,
        length as i64,
        span,
    )?;
    Ok(Value::Unit)
}

/// Direct checked `ByteBuffer.copy` leaf. The source range is snapshotted
/// before target mutation, preserving self-copy and overlap semantics.
pub fn builtin_byte_buffer_copy_i64(
    target: &Value,
    source: &Value,
    source_start: i64,
    target_start: i64,
    length: i64,
    span: Span,
) -> Result<(), RtError> {
    let target = byte_buffer_arg(target, "copy", span)?;
    let source = byte_buffer_arg(source, "copy", span)?;
    let count = byte_buffer_index_i64(length, "length", span)?;
    let source_start = byte_buffer_index_i64(source_start, "source start", span)?;
    let target_start = byte_buffer_index_i64(target_start, "target start", span)?;
    let source_length = source.borrow().len();
    let target_length = target.borrow().len();
    if source_start > source_length
        || count > source_length.saturating_sub(source_start)
        || target_start > target_length
        || count > target_length.saturating_sub(target_start)
    {
        return Err(fault(
            codes::FAULT_INDEX,
            "`ByteBuffer.copy` range is out of bounds",
            span,
        ));
    }
    let snapshot = source.borrow()[source_start..source_start + count].to_vec();
    target.borrow_mut()[target_start..target_start + count].copy_from_slice(&snapshot);
    Ok(())
}

pub fn builtin_byte_buffer_to_bytes(recv: Value, span: Span) -> Result<Value, RtError> {
    builtin_byte_buffer_to_bytes_ref(&recv, span)
}

/// Direct checked immutable snapshot leaf.
pub fn builtin_byte_buffer_to_bytes_ref(recv: &Value, span: Span) -> Result<Value, RtError> {
    let bytes = byte_buffer_arg(recv, "toBytes", span)?;
    let snapshot = bytes.borrow().clone();
    Ok(Value::Bytes(Rc::from(snapshot.as_slice())))
}
