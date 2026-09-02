use super::super::*;

pub(in crate::value) fn codec_bytes_arg(
    arg: &Value,
    name: &str,
    span: Span,
) -> Result<Rc<[u8]>, RtError> {
    match arg {
        Value::Bytes(b) => Ok(b.clone()),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Codec.{name}` takes `Bytes`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub(in crate::value) fn codec_err(message: &'static str) -> Value {
    Value::Err(Rc::new(Value::str(message)))
}

pub(in crate::value) fn crc32_iso_hdlc(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

pub(in crate::value) fn push_u16_le(out: &mut Vec<u8>, n: u16) {
    out.extend_from_slice(&n.to_le_bytes());
}

pub(in crate::value) fn push_u32_le(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}

pub(in crate::value) fn push_u64_le(out: &mut Vec<u8>, n: u64) {
    out.extend_from_slice(&n.to_le_bytes());
}

pub(in crate::value) fn push_u24_le(out: &mut Vec<u8>, n: u32) {
    out.push((n & 0xff) as u8);
    out.push(((n >> 8) & 0xff) as u8);
    out.push(((n >> 16) & 0xff) as u8);
}

pub(in crate::value) fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

pub(in crate::value) fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ]))
}

pub(in crate::value) fn read_u64_le(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
        *bytes.get(offset + 4)?,
        *bytes.get(offset + 5)?,
        *bytes.get(offset + 6)?,
        *bytes.get(offset + 7)?,
    ]))
}

/// §15 `Codec.gzipCompress(bytes) -> Result<Bytes, string>` — canonical gzip over
/// DEFLATE stored blocks. This is a deterministic subset, not a compression-ratio
/// optimizer: fixed gzip header (`mtime=0`, XFL=0, OS=255), no optional fields, and
/// byte-for-byte stable output for the same input.
pub fn builtin_codec_gzip_compress(arg: Value, span: Span) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "gzipCompress", span)?;
    if data.len() > u32::MAX as usize {
        return Ok(codec_err(
            "Codec.gzipCompress: input exceeds gzip ISIZE limit",
        ));
    }
    let blocks = if data.is_empty() {
        1
    } else {
        data.len().div_ceil(65_535)
    };
    let mut out = Vec::with_capacity(10 + data.len() + blocks * 5 + 8);
    out.extend_from_slice(&[0x1f, 0x8b, 0x08, 0x00]);
    push_u32_le(&mut out, 0);
    out.extend_from_slice(&[0x00, 0xff]);
    if data.is_empty() {
        out.push(0x01);
        push_u16_le(&mut out, 0);
        push_u16_le(&mut out, 0xffff);
    } else {
        for (i, chunk) in data.chunks(65_535).enumerate() {
            let final_block = i + 1 == blocks;
            out.push(if final_block { 0x01 } else { 0x00 });
            let len = chunk.len() as u16;
            push_u16_le(&mut out, len);
            push_u16_le(&mut out, !len);
            out.extend_from_slice(chunk);
        }
    }
    push_u32_le(&mut out, crc32_iso_hdlc(&data));
    push_u32_le(&mut out, data.len() as u32);
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

/// §15 `Codec.gzipDecompress(bytes) -> Result<Bytes, string>` — decodes the same
/// canonical gzip stored-block subset that `gzipCompress` emits, validating the
/// wrapper, LEN/NLEN pairs, CRC32, and ISIZE. Ordinary data errors are `Err`.
pub fn builtin_codec_gzip_decompress(arg: Value, span: Span) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "gzipDecompress", span)?;
    if data.len() < 18 {
        return Ok(codec_err("Codec.gzipDecompress: truncated gzip stream"));
    }
    if data[0] != 0x1f || data[1] != 0x8b || data[2] != 0x08 {
        return Ok(codec_err("Codec.gzipDecompress: invalid gzip header"));
    }
    if data[3] != 0 {
        return Ok(codec_err(
            "Codec.gzipDecompress: unsupported gzip header flags",
        ));
    }
    if data[4..8] != [0, 0, 0, 0] {
        return Ok(codec_err("Codec.gzipDecompress: non-canonical gzip mtime"));
    }
    if data[8] != 0 || data[9] != 0xff {
        return Ok(codec_err("Codec.gzipDecompress: non-canonical gzip header"));
    }
    let trailer = data.len() - 8;
    let mut p = 10usize;
    let mut out = Vec::new();
    loop {
        if p + 5 > trailer {
            return Ok(codec_err("Codec.gzipDecompress: truncated deflate block"));
        }
        let header = data[p];
        p += 1;
        if header & 0b0000_0110 != 0 {
            return Ok(codec_err(
                "Codec.gzipDecompress: unsupported deflate block type",
            ));
        }
        if header & 0b1111_1000 != 0 {
            return Ok(codec_err(
                "Codec.gzipDecompress: non-canonical stored block header",
            ));
        }
        let final_block = header & 1 == 1;
        let (Some(len), Some(nlen)) = (read_u16_le(&data, p), read_u16_le(&data, p + 2)) else {
            return Ok(codec_err("Codec.gzipDecompress: truncated deflate block"));
        };
        let len = len as usize;
        p += 4;
        if (len as u16) ^ nlen != 0xffff {
            return Ok(codec_err(
                "Codec.gzipDecompress: invalid stored block length",
            ));
        }
        if !final_block && len != 65_535 {
            return Ok(codec_err(
                "Codec.gzipDecompress: non-canonical stored block length",
            ));
        }
        let Some(end) = p.checked_add(len) else {
            return Ok(codec_err("Codec.gzipDecompress: truncated stored block"));
        };
        if end > trailer {
            return Ok(codec_err("Codec.gzipDecompress: truncated stored block"));
        }
        out.extend_from_slice(&data[p..end]);
        p = end;
        if final_block {
            break;
        }
    }
    if p != trailer {
        return Ok(codec_err(
            "Codec.gzipDecompress: trailing deflate data before gzip trailer",
        ));
    }
    let (Some(expected_crc), Some(expected_size)) =
        (read_u32_le(&data, trailer), read_u32_le(&data, trailer + 4))
    else {
        return Ok(codec_err("Codec.gzipDecompress: truncated gzip trailer"));
    };
    if out.len() > u32::MAX as usize || out.len() as u32 != expected_size {
        return Ok(codec_err("Codec.gzipDecompress: ISIZE mismatch"));
    }
    if crc32_iso_hdlc(&out) != expected_crc {
        return Ok(codec_err("Codec.gzipDecompress: CRC32 mismatch"));
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

/// §15 `Codec.deflateCompress(bytes) -> Result<Bytes, string>` — canonical raw
/// DEFLATE stored blocks. This is the same deterministic subset used inside gzip,
/// exposed for tools that need a raw deflate stream.
pub fn builtin_codec_deflate_compress(arg: Value, span: Span) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "deflateCompress", span)?;
    let blocks = if data.is_empty() {
        1
    } else {
        data.len().div_ceil(65_535)
    };
    let mut out = Vec::with_capacity(data.len() + blocks * 5);
    if data.is_empty() {
        out.push(0x01);
        push_u16_le(&mut out, 0);
        push_u16_le(&mut out, 0xffff);
    } else {
        for (i, chunk) in data.chunks(65_535).enumerate() {
            let final_block = i + 1 == blocks;
            out.push(if final_block { 0x01 } else { 0x00 });
            let len = chunk.len() as u16;
            push_u16_le(&mut out, len);
            push_u16_le(&mut out, !len);
            out.extend_from_slice(chunk);
        }
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

pub(in crate::value) const FIXED_DEFLATE_MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
pub(in crate::value) const FIXED_DEFLATE_WINDOW_BYTES: usize = 32_768;
pub(in crate::value) const FIXED_DEFLATE_WINDOW_MASK: usize = FIXED_DEFLATE_WINDOW_BYTES - 1;
pub(in crate::value) const FIXED_DEFLATE_HASH_SIZE: usize = 1 << 16;
pub(in crate::value) const FIXED_DEFLATE_MAX_MATCH_BYTES: usize = 258;
pub(in crate::value) const FIXED_DEFLATE_MIN_MATCH_BYTES: usize = 3;
pub(in crate::value) const FIXED_DEFLATE_MAX_CHAIN_SEARCHES: usize = 128;

pub(in crate::value) fn fixed_deflate_input_too_large(len: usize) -> bool {
    len > FIXED_DEFLATE_MAX_INPUT_BYTES
}

pub(in crate::value) const FIXED_DEFLATE_LENGTH_BASES: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
pub(in crate::value) const FIXED_DEFLATE_LENGTH_EXTRA_BITS: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
pub(in crate::value) const FIXED_DEFLATE_DISTANCE_BASES: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1_025, 1_537,
    2_049, 3_073, 4_097, 6_145, 8_193, 12_289, 16_385, 24_577,
];
pub(in crate::value) const FIXED_DEFLATE_DISTANCE_EXTRA_BITS: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

pub(in crate::value) struct FixedDeflateBitWriter {
    bytes: Vec<u8>,
    pending_value: u32,
    pending_bits: u8,
}

impl FixedDeflateBitWriter {
    fn with_prefix(input_len: usize, prefix: &[u8]) -> Self {
        let capacity = input_len
            .checked_add(input_len / 8)
            .and_then(|n| n.checked_add(16))
            .and_then(|n| n.checked_add(prefix.len()))
            .unwrap_or(input_len);
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(prefix);
        Self {
            bytes,
            pending_value: 0,
            pending_bits: 0,
        }
    }

    fn write_bits(&mut self, value: u32, bit_count: u8) {
        self.pending_value |= value << self.pending_bits;
        self.pending_bits += bit_count;
        while self.pending_bits >= 8 {
            self.bytes.push((self.pending_value & 0xff) as u8);
            self.pending_value >>= 8;
            self.pending_bits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.pending_bits > 0 {
            self.bytes.push((self.pending_value & 0xff) as u8);
        }
        self.bytes
    }
}

pub(in crate::value) fn fixed_deflate_reverse_bits(mut value: u16, bit_count: u8) -> u16 {
    let mut reversed = 0u16;
    for _ in 0..bit_count {
        reversed = (reversed << 1) | (value & 1);
        value >>= 1;
    }
    reversed
}

pub(in crate::value) fn fixed_deflate_symbol_bits(symbol: usize) -> (u16, u8) {
    match symbol {
        0..=143 => (fixed_deflate_reverse_bits(0x30 + symbol as u16, 8), 8),
        144..=255 => (
            fixed_deflate_reverse_bits(0x190 + (symbol - 144) as u16, 9),
            9,
        ),
        256..=279 => (fixed_deflate_reverse_bits((symbol - 256) as u16, 7), 7),
        280..=287 => (
            fixed_deflate_reverse_bits(0xc0 + (symbol - 280) as u16, 8),
            8,
        ),
        _ => unreachable!("fixed DEFLATE symbol is outside the fixed tree"),
    }
}

pub(in crate::value) fn fixed_deflate_write_symbol(
    writer: &mut FixedDeflateBitWriter,
    symbol: usize,
) {
    let (bits, bit_count) = fixed_deflate_symbol_bits(symbol);
    writer.write_bits(bits as u32, bit_count);
}

pub(in crate::value) fn fixed_deflate_length_code_index(length: usize) -> usize {
    FIXED_DEFLATE_LENGTH_BASES
        .iter()
        .rposition(|base| length >= *base)
        .expect("match length is at least the fixed DEFLATE minimum")
}

pub(in crate::value) fn fixed_deflate_distance_code_index(distance: usize) -> usize {
    FIXED_DEFLATE_DISTANCE_BASES
        .iter()
        .rposition(|base| distance >= *base)
        .expect("match distance is at least one")
}

pub(in crate::value) fn fixed_deflate_match_bit_count(length: usize, distance: usize) -> usize {
    let length_index = fixed_deflate_length_code_index(length);
    let distance_index = fixed_deflate_distance_code_index(distance);
    let (_, symbol_bits) = fixed_deflate_symbol_bits(257 + length_index);
    symbol_bits as usize
        + FIXED_DEFLATE_LENGTH_EXTRA_BITS[length_index] as usize
        + 5
        + FIXED_DEFLATE_DISTANCE_EXTRA_BITS[distance_index] as usize
}

pub(in crate::value) fn fixed_deflate_literal_bit_count(
    input: &[u8],
    offset: usize,
    length: usize,
) -> usize {
    input[offset..offset + length]
        .iter()
        .map(|byte| if *byte <= 143 { 8 } else { 9 })
        .sum()
}

pub(in crate::value) fn fixed_deflate_write_match(
    writer: &mut FixedDeflateBitWriter,
    length: usize,
    distance: usize,
) {
    let length_index = fixed_deflate_length_code_index(length);
    fixed_deflate_write_symbol(writer, 257 + length_index);
    let length_extra_bits = FIXED_DEFLATE_LENGTH_EXTRA_BITS[length_index];
    if length_extra_bits > 0 {
        writer.write_bits(
            (length - FIXED_DEFLATE_LENGTH_BASES[length_index]) as u32,
            length_extra_bits,
        );
    }

    let distance_index = fixed_deflate_distance_code_index(distance);
    writer.write_bits(
        fixed_deflate_reverse_bits(distance_index as u16, 5) as u32,
        5,
    );
    let distance_extra_bits = FIXED_DEFLATE_DISTANCE_EXTRA_BITS[distance_index];
    if distance_extra_bits > 0 {
        writer.write_bits(
            (distance - FIXED_DEFLATE_DISTANCE_BASES[distance_index]) as u32,
            distance_extra_bits,
        );
    }
}

pub(in crate::value) fn fixed_deflate_hash_three_bytes(input: &[u8], offset: usize) -> usize {
    let value = ((input[offset] as u32) << 16)
        | ((input[offset + 1] as u32) << 8)
        | input[offset + 2] as u32;
    (value.wrapping_mul(0x1e35_a7bd) >> 16) as usize
}

pub(in crate::value) fn deterministic_fixed_deflate_with_prefix(
    input: &[u8],
    prefix: &[u8],
) -> Vec<u8> {
    let mut writer = FixedDeflateBitWriter::with_prefix(input.len(), prefix);
    let mut hash_heads = vec![-1i32; FIXED_DEFLATE_HASH_SIZE];
    let mut previous_positions = vec![-1i32; FIXED_DEFLATE_WINDOW_BYTES];
    let mut slot_positions = vec![-1i32; FIXED_DEFLATE_WINDOW_BYTES];

    writer.write_bits(1, 1);
    writer.write_bits(0b01, 2);

    let insert_position = |position: usize,
                           hash_heads: &mut [i32],
                           previous_positions: &mut [i32],
                           slot_positions: &mut [i32]| {
        if position + FIXED_DEFLATE_MIN_MATCH_BYTES > input.len() {
            return;
        }
        let hash = fixed_deflate_hash_three_bytes(input, position);
        let slot = position & FIXED_DEFLATE_WINDOW_MASK;
        previous_positions[slot] = hash_heads[hash];
        slot_positions[slot] = position as i32;
        hash_heads[hash] = position as i32;
    };

    let mut position = 0usize;
    while position < input.len() {
        let found_match = if position + FIXED_DEFLATE_MIN_MATCH_BYTES > input.len() {
            None
        } else {
            let hash = fixed_deflate_hash_three_bytes(input, position);
            let minimum_position = position.saturating_sub(FIXED_DEFLATE_WINDOW_BYTES) as i32;
            let maximum_length = FIXED_DEFLATE_MAX_MATCH_BYTES.min(input.len() - position);
            let mut candidate = hash_heads[hash];
            let mut searches = 0usize;
            let mut best_length = 0usize;
            let mut best_distance = 0usize;

            while candidate >= minimum_position
                && candidate >= 0
                && searches < FIXED_DEFLATE_MAX_CHAIN_SEARCHES
            {
                let candidate_position = candidate as usize;
                let slot = candidate_position & FIXED_DEFLATE_WINDOW_MASK;
                if slot_positions[slot] != candidate {
                    break;
                }
                if best_length == 0
                    || best_length == maximum_length
                    || input[candidate_position + best_length] == input[position + best_length]
                {
                    let mut length = 0usize;
                    while length < maximum_length
                        && input[candidate_position + length] == input[position + length]
                    {
                        length += 1;
                    }
                    if length > best_length {
                        best_length = length;
                        best_distance = position - candidate_position;
                        if best_length == maximum_length {
                            break;
                        }
                    }
                }
                candidate = previous_positions[slot];
                searches += 1;
            }

            if best_length < FIXED_DEFLATE_MIN_MATCH_BYTES
                || fixed_deflate_match_bit_count(best_length, best_distance)
                    >= fixed_deflate_literal_bit_count(input, position, best_length)
            {
                None
            } else {
                Some((best_length, best_distance))
            }
        };

        if let Some((length, distance)) = found_match {
            fixed_deflate_write_match(&mut writer, length, distance);
            let match_end = position + length;
            while position < match_end {
                insert_position(
                    position,
                    &mut hash_heads,
                    &mut previous_positions,
                    &mut slot_positions,
                );
                position += 1;
            }
        } else {
            fixed_deflate_write_symbol(&mut writer, input[position] as usize);
            insert_position(
                position,
                &mut hash_heads,
                &mut previous_positions,
                &mut slot_positions,
            );
            position += 1;
        }
    }

    fixed_deflate_write_symbol(&mut writer, 256);
    writer.finish()
}

pub(in crate::value) fn deterministic_fixed_deflate(input: &[u8]) -> Vec<u8> {
    deterministic_fixed_deflate_with_prefix(input, &[])
}

/// `Codec.deflateFixedCompress(bytes) -> Result<Bytes, string>` — one final
/// deterministic raw fixed-Huffman DEFLATE block with a bounded LZ77 search.
pub fn builtin_codec_deflate_fixed_compress(arg: Value, span: Span) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "deflateFixedCompress", span)?;
    if fixed_deflate_input_too_large(data.len()) {
        return Ok(codec_err(
            "Codec.deflateFixedCompress: input exceeds 256 MiB",
        ));
    }
    let out = deterministic_fixed_deflate(&data);
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

pub(in crate::value) fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let mut low = 1u32;
    let mut high = 0u32;
    for chunk in bytes.chunks(5_552) {
        for &byte in chunk {
            low += u32::from(byte);
            high += low;
        }
        low %= MODULUS;
        high %= MODULUS;
    }
    (high << 16) | low
}

/// `Codec.zlibFixedCompress(bytes) -> Result<Bytes, string>` — RFC 1950
/// framing around the exact deterministic fixed-Huffman DEFLATE stream.
pub fn builtin_codec_zlib_fixed_compress(arg: Value, span: Span) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "zlibFixedCompress", span)?;
    if fixed_deflate_input_too_large(data.len()) {
        return Ok(codec_err("Codec.zlibFixedCompress: input exceeds 256 MiB"));
    }
    let mut out = deterministic_fixed_deflate_with_prefix(&data, &[0x78, 0x01]);
    out.extend_from_slice(&adler32(&data).to_be_bytes());
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(
        out.into_boxed_slice(),
    )))))
}

pub(in crate::value) const REED_SOLOMON_DATA_BYTES: usize = 223;
pub(in crate::value) const REED_SOLOMON_PARITY_BYTES: usize = 32;
pub(in crate::value) const REED_SOLOMON_CODEWORD_BYTES: usize = 255;
pub(in crate::value) const REED_SOLOMON_MAX_SHARDS: usize = 65_535;
pub(in crate::value) const REED_SOLOMON_MAX_INPUT_BYTES: usize =
    REED_SOLOMON_DATA_BYTES * REED_SOLOMON_MAX_SHARDS;
pub(in crate::value) const REED_SOLOMON_GENERATOR: [u8; REED_SOLOMON_PARITY_BYTES + 1] = [
    1, 116, 64, 52, 174, 54, 126, 16, 194, 162, 33, 33, 157, 176, 197, 225, 12, 59, 55, 253, 228,
    148, 47, 179, 185, 24, 138, 253, 20, 142, 55, 172, 88,
];

pub(in crate::value) fn reed_solomon_galois_tables() -> ([u8; 512], [u8; 256]) {
    let mut exponent = [0u8; 512];
    let mut logarithm = [0u8; 256];
    let mut value = 1u16;
    for (index, exponent_entry) in exponent.iter_mut().take(255).enumerate() {
        *exponent_entry = value as u8;
        logarithm[value as usize] = index as u8;
        value <<= 1;
        if value & 0x100 != 0 {
            value ^= 0x11d;
        }
    }
    for index in 255..exponent.len() {
        exponent[index] = exponent[index - 255];
    }
    (exponent, logarithm)
}

pub(in crate::value) fn reed_solomon_multiply(
    left: u8,
    right: u8,
    exponent: &[u8; 512],
    logarithm: &[u8; 256],
) -> u8 {
    if left == 0 || right == 0 {
        0
    } else {
        exponent[logarithm[left as usize] as usize + logarithm[right as usize] as usize]
    }
}

pub(in crate::value) fn reed_solomon_255_223_parity(
    data: &[u8],
    exponent: &[u8; 512],
    logarithm: &[u8; 256],
) -> [u8; REED_SOLOMON_PARITY_BYTES] {
    debug_assert_eq!(data.len(), REED_SOLOMON_DATA_BYTES);
    let mut remainder = [0u8; REED_SOLOMON_PARITY_BYTES];
    for byte in data {
        let coefficient = *byte ^ remainder[0];
        remainder.copy_within(1.., 0);
        remainder[REED_SOLOMON_PARITY_BYTES - 1] = 0;
        if coefficient != 0 {
            for index in 0..REED_SOLOMON_PARITY_BYTES {
                remainder[index] ^= reed_solomon_multiply(
                    REED_SOLOMON_GENERATOR[index + 1],
                    coefficient,
                    exponent,
                    logarithm,
                );
            }
        }
    }
    remainder
}

/// `Codec.reedSolomon255223Protect(bytes) -> Result<Bytes, string>` —
/// systematic RS(255,223) codewords over GF(256), with a zero-padded final
/// data shard.
pub fn builtin_codec_reed_solomon_255_223_protect(
    arg: Value,
    span: Span,
) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "reedSolomon255223Protect", span)?;
    if data.is_empty() {
        return Ok(codec_err(
            "Codec.reedSolomon255223Protect: input must not be empty",
        ));
    }
    if data.len() > REED_SOLOMON_MAX_INPUT_BYTES {
        return Ok(codec_err(
            "Codec.reedSolomon255223Protect: input requires more than 65535 shards",
        ));
    }

    let shard_count = data.len().div_ceil(REED_SOLOMON_DATA_BYTES);
    let output_len = shard_count
        .checked_mul(REED_SOLOMON_CODEWORD_BYTES)
        .expect("the validated Reed-Solomon shard count fits usize");
    let mut output = vec![0u8; output_len];
    let (exponent, logarithm) = reed_solomon_galois_tables();
    for shard in 0..shard_count {
        let input_start = shard * REED_SOLOMON_DATA_BYTES;
        let input_end = (input_start + REED_SOLOMON_DATA_BYTES).min(data.len());
        let output_start = shard * REED_SOLOMON_CODEWORD_BYTES;
        output[output_start..output_start + input_end - input_start]
            .copy_from_slice(&data[input_start..input_end]);
        let data_end = output_start + REED_SOLOMON_DATA_BYTES;
        let parity =
            reed_solomon_255_223_parity(&output[output_start..data_end], &exponent, &logarithm);
        output[data_end..data_end + REED_SOLOMON_PARITY_BYTES].copy_from_slice(&parity);
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(
        output.into_boxed_slice(),
    )))))
}

/// §15 `Codec.deflateDecompress(bytes) -> Result<Bytes, string>` — validates and
/// decodes the canonical raw DEFLATE stored-block subset.
pub fn builtin_codec_deflate_decompress(arg: Value, span: Span) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "deflateDecompress", span)?;
    let mut p = 0usize;
    let mut out = Vec::new();
    loop {
        if p + 5 > data.len() {
            return Ok(codec_err(
                "Codec.deflateDecompress: truncated deflate block",
            ));
        }
        let header = data[p];
        p += 1;
        if header & 0b0000_0110 != 0 {
            return Ok(codec_err(
                "Codec.deflateDecompress: unsupported deflate block type",
            ));
        }
        if header & 0b1111_1000 != 0 {
            return Ok(codec_err(
                "Codec.deflateDecompress: non-canonical stored block header",
            ));
        }
        let final_block = header & 1 == 1;
        let (Some(len), Some(nlen)) = (read_u16_le(&data, p), read_u16_le(&data, p + 2)) else {
            return Ok(codec_err(
                "Codec.deflateDecompress: truncated deflate block",
            ));
        };
        let len = len as usize;
        p += 4;
        if (len as u16) ^ nlen != 0xffff {
            return Ok(codec_err(
                "Codec.deflateDecompress: invalid stored block length",
            ));
        }
        if !final_block && len != 65_535 {
            return Ok(codec_err(
                "Codec.deflateDecompress: non-canonical stored block length",
            ));
        }
        let Some(end) = p.checked_add(len) else {
            return Ok(codec_err("Codec.deflateDecompress: truncated stored block"));
        };
        if end > data.len() {
            return Ok(codec_err("Codec.deflateDecompress: truncated stored block"));
        }
        out.extend_from_slice(&data[p..end]);
        p = end;
        if final_block {
            break;
        }
    }
    if p != data.len() {
        return Ok(codec_err(
            "Codec.deflateDecompress: trailing data after final block",
        ));
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

pub(in crate::value) const ZSTD_BLOCK_MAX: usize = 128 * 1024;

pub(in crate::value) fn zstd_level_arg(arg: &Value, span: Span) -> Result<Option<Value>, RtError> {
    match arg {
        Value::Int(level) if (1..=22).contains(level) => Ok(None),
        Value::Int(_) => Ok(Some(codec_err(
            "Codec.zstdCompress: level must be between 1 and 22",
        ))),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`Codec.zstdCompress` level must be `int`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn push_zstd_frame_content_size(out: &mut Vec<u8>, len: usize) {
    if len <= u8::MAX as usize {
        out.push(0x20);
        out.push(len as u8);
    } else if len <= 65_791 {
        out.push(0x60);
        push_u16_le(out, (len - 256) as u16);
    } else if let Ok(n) = u32::try_from(len) {
        out.push(0xa0);
        push_u32_le(out, n);
    } else {
        out.push(0xe0);
        push_u64_le(out, len as u64);
    }
}

pub(in crate::value) fn push_zstd_raw_block_header(
    out: &mut Vec<u8>,
    len: usize,
    final_block: bool,
) {
    let header = ((len as u32) << 3) | u32::from(final_block);
    push_u24_le(out, header);
}

/// §15 `Codec.zstdCompress(bytes, level = 3) -> Result<Bytes, string>` — canonical
/// Zstandard frame using the RFC 8878 single-segment, no-checksum, raw-block
/// subset. `level` is accepted for API compatibility and range-checked, but the
/// deterministic store-only subset has exactly one encoding.
pub fn builtin_codec_zstd_compress(
    bytes: Value,
    level: Value,
    span: Span,
) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&bytes, "zstdCompress", span)?;
    if let Some(err) = zstd_level_arg(&level, span)? {
        return Ok(err);
    }
    let blocks = if data.is_empty() {
        1
    } else {
        data.len().div_ceil(ZSTD_BLOCK_MAX)
    };
    let Some(cap) = data
        .len()
        .checked_add(4 + 1 + 8)
        .and_then(|n| n.checked_add(blocks.checked_mul(3)?))
    else {
        return Ok(codec_err("Codec.zstdCompress: input too large"));
    };
    let mut out = Vec::with_capacity(cap);
    out.extend_from_slice(&[0x28, 0xb5, 0x2f, 0xfd]);
    push_zstd_frame_content_size(&mut out, data.len());
    if data.is_empty() {
        push_zstd_raw_block_header(&mut out, 0, true);
    } else {
        for (i, chunk) in data.chunks(ZSTD_BLOCK_MAX).enumerate() {
            push_zstd_raw_block_header(&mut out, chunk.len(), i + 1 == blocks);
            out.extend_from_slice(chunk);
        }
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

/// §15 `Codec.zstdDecompress(bytes) -> Result<Bytes, string>` — decodes exactly the
/// canonical raw-block Zstandard subset emitted by `zstdCompress`. Full compressed,
/// RLE, dictionary, checksum, and skippable-frame support remain outside this
/// deterministic stdlib slice.
pub fn builtin_codec_zstd_decompress(arg: Value, span: Span) -> Result<Value, RtError> {
    let data = codec_bytes_arg(&arg, "zstdDecompress", span)?;
    if data.len() < 9 {
        return Ok(codec_err("Codec.zstdDecompress: truncated zstd frame"));
    }
    if data[0..4] != [0x28, 0xb5, 0x2f, 0xfd] {
        return Ok(codec_err("Codec.zstdDecompress: invalid zstd magic"));
    }
    let descriptor = data[4];
    let mut p = 5usize;
    let expected = match descriptor {
        0x20 => {
            let Some(n) = data.get(p).copied() else {
                return Ok(codec_err("Codec.zstdDecompress: truncated frame header"));
            };
            p += 1;
            n as u64
        }
        0x60 => {
            let Some(n) = read_u16_le(&data, p) else {
                return Ok(codec_err("Codec.zstdDecompress: truncated frame header"));
            };
            p += 2;
            n as u64 + 256
        }
        0xa0 => {
            let Some(n) = read_u32_le(&data, p) else {
                return Ok(codec_err("Codec.zstdDecompress: truncated frame header"));
            };
            p += 4;
            n as u64
        }
        0xe0 => {
            let Some(n) = read_u64_le(&data, p) else {
                return Ok(codec_err("Codec.zstdDecompress: truncated frame header"));
            };
            p += 8;
            n
        }
        _ => {
            return Ok(codec_err(
                "Codec.zstdDecompress: unsupported zstd frame header",
            ));
        }
    };
    if descriptor == 0xa0 && expected <= u8::MAX as u64 {
        return Ok(codec_err(
            "Codec.zstdDecompress: non-canonical frame content size",
        ));
    }
    if descriptor == 0xa0 && expected <= 65_791 {
        return Ok(codec_err(
            "Codec.zstdDecompress: non-canonical frame content size",
        ));
    }
    if descriptor == 0xe0 && expected <= u32::MAX as u64 {
        return Ok(codec_err(
            "Codec.zstdDecompress: non-canonical frame content size",
        ));
    }
    let Ok(expected_len) = usize::try_from(expected) else {
        return Ok(codec_err(
            "Codec.zstdDecompress: frame content size exceeds platform limit",
        ));
    };
    let block_max = expected_len.min(ZSTD_BLOCK_MAX);
    let mut out = Vec::new();
    loop {
        if p + 3 > data.len() {
            return Ok(codec_err("Codec.zstdDecompress: truncated block header"));
        }
        let header = (data[p] as u32) | ((data[p + 1] as u32) << 8) | ((data[p + 2] as u32) << 16);
        p += 3;
        let final_block = header & 1 == 1;
        let block_type = (header >> 1) & 0b11;
        let block_size = (header >> 3) as usize;
        if block_type == 3 {
            return Ok(codec_err("Codec.zstdDecompress: reserved zstd block type"));
        }
        if block_type != 0 {
            return Ok(codec_err(
                "Codec.zstdDecompress: unsupported zstd block type",
            ));
        }
        if block_size > block_max {
            return Ok(codec_err(
                "Codec.zstdDecompress: raw block exceeds maximum size",
            ));
        }
        if !final_block && block_size != ZSTD_BLOCK_MAX {
            return Ok(codec_err(
                "Codec.zstdDecompress: non-canonical raw block length",
            ));
        }
        let Some(new_len) = out.len().checked_add(block_size) else {
            return Ok(codec_err("Codec.zstdDecompress: content size mismatch"));
        };
        if new_len > expected_len {
            return Ok(codec_err("Codec.zstdDecompress: content size mismatch"));
        }
        let Some(end) = p.checked_add(block_size) else {
            return Ok(codec_err("Codec.zstdDecompress: truncated raw block"));
        };
        if end > data.len() {
            return Ok(codec_err("Codec.zstdDecompress: truncated raw block"));
        }
        out.extend_from_slice(&data[p..end]);
        p = end;
        if final_block {
            break;
        }
    }
    if p != data.len() {
        return Ok(codec_err(
            "Codec.zstdDecompress: trailing data after zstd frame",
        ));
    }
    if out.len() != expected_len {
        return Ok(codec_err("Codec.zstdDecompress: content size mismatch"));
    }
    Ok(Value::Ok(Rc::new(Value::Bytes(Rc::from(out.as_slice())))))
}

// §15 (v5.4) the `Hash` builtin namespace — SHA-256, SHA-512 (FIPS 180-4) and
// HMAC-SHA256 (RFC 2104), all IN-HOUSE pure-SAFE Rust (the workspace forbids
// `unsafe_code`, and `topaz build` emits an OFFLINE, locked crate — a crypto
// crate dependency would complicate that offline emitted-crate build). The hash
// CORE below (`sha256`/`sha512`/`hmac_sha256` over `&[u8] -> [u8; N]`) is value-
// free + deterministic; the three Topaz leaves pull their `Bytes` argument(s) and
// call the SAME core, so the digest is byte-identical run≡build by construction.
//
// ★ CORRECTNESS is pinned to OFFICIAL test vectors (run≡build does NOT catch a
// wrong-but-consistent hash): SHA-256("abc")/("") + SHA-512("abc") (FIPS 180-4)
// and HMAC-SHA256 RFC 4231 Test Case 2 are asserted as unit tests below AND as
// difftest fixtures, alongside a multi-block (>64/128 byte) input that exercises
// the padding + block loop.

/// The SHA-256 round constants (FIPS 180-4 §4.2.2): the first 32 bits of the
/// fractional parts of the cube roots of the first 64 primes.
#[rustfmt::skip]
pub(in crate::value) const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// §15 SHA-256 (FIPS 180-4): hash `data` to its 32-byte digest. Pure + total —
/// processes the input in 512-bit (64-byte) blocks with the standard `1` bit +
/// zero padding + 64-bit big-endian length, then runs the 64-round compression.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    // Initial hash value (FIPS 180-4 §5.3.3): fractional parts of the square
    // roots of the first 8 primes.
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    // Padding (§5.1.1): append `0x80`, then zeros, then the 64-bit big-endian bit
    // length, so the total is a multiple of 64 bytes.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks_exact(64) {
        // Message schedule (§6.2.2 step 1): 16 big-endian words, extended to 64.
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let j = i * 4;
            *word = u32::from_be_bytes([block[j], block[j + 1], block[j + 2], block[j + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        // Compression (§6.2.2 step 3): 64 rounds over the working variables.
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (hi, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *hi = hi.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// The SHA-512 round constants (FIPS 180-4 §4.2.3): the first 64 bits of the
/// fractional parts of the cube roots of the first 80 primes.
#[rustfmt::skip]
pub(in crate::value) const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
    0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
    0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
    0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

/// §15 SHA-512 (FIPS 180-4): hash `data` to its 64-byte digest. Pure + total —
/// processes the input in 1024-bit (128-byte) blocks with the `1` bit + zero
/// padding + 128-bit big-endian length, then runs the 80-round compression. The
/// bit length fits in `u64` for any realistic input, so the high 64 length bits
/// are zero (the same assumption SHA-256 makes for its 64-bit length).
pub fn sha512(data: &[u8]) -> [u8; 64] {
    // Initial hash value (FIPS 180-4 §5.3.5): fractional parts of the square
    // roots of the first 8 primes (64-bit).
    let mut h: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];
    let bit_len = (data.len() as u128).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks_exact(128) {
        let mut w = [0u64; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let j = i * 8;
            *word = u64::from_be_bytes([
                block[j],
                block[j + 1],
                block[j + 2],
                block[j + 3],
                block[j + 4],
                block[j + 5],
                block[j + 6],
                block[j + 7],
            ]);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA512_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (hi, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *hi = hi.wrapping_add(v);
        }
    }
    let mut out = [0u8; 64];
    for (i, word) in h.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// §15 HMAC-SHA256 (RFC 2104): the 32-byte keyed MAC of `message` under `key`,
/// using SHA-256's 64-byte block. A key LONGER than the block is first hashed to
/// 32 bytes (RFC 2104 step "if K is longer than B…"); a SHORTER key is zero-
/// padded. Pure + total.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    // Normalize the key to exactly one SHA-256 block.
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    // inner = SHA256( (K ^ ipad) || message ); outer = SHA256( (K ^ opad) || inner ).
    let mut inner = Vec::with_capacity(BLOCK + message.len());
    inner.extend(k.iter().map(|b| b ^ 0x36));
    inner.extend_from_slice(message);
    let inner_digest = sha256(&inner);
    let mut outer = Vec::with_capacity(BLOCK + 32);
    outer.extend(k.iter().map(|b| b ^ 0x5c));
    outer.extend_from_slice(&inner_digest);
    sha256(&outer)
}

/// §15 `Hash.sha256(data: Bytes) -> Bytes` — the 32-byte SHA-256 digest. Pure +
/// total; both engines share the `sha256` core, so the digest is byte-identical
/// run≡build. The caller does `.toHex()` for a hex string.
pub fn builtin_hash_sha256(arg: Value, span: Span) -> Result<Value, RtError> {
    let b = hash_bytes_arg(&arg, "sha256", span)?;
    Ok(Value::Bytes(Rc::from(sha256(&b).as_slice())))
}

/// §15 `Hash.sha512(data: Bytes) -> Bytes` — the 64-byte SHA-512 digest (pure +
/// total; shared `sha512` core).
pub fn builtin_hash_sha512(arg: Value, span: Span) -> Result<Value, RtError> {
    let b = hash_bytes_arg(&arg, "sha512", span)?;
    Ok(Value::Bytes(Rc::from(sha512(&b).as_slice())))
}

/// §15 `Hash.hmacSha256(key: Bytes, message: Bytes) -> Bytes` — the 32-byte
/// HMAC-SHA256 MAC (pure + total; shared `hmac_sha256` core).
pub fn builtin_hash_hmac_sha256(key: Value, message: Value, span: Span) -> Result<Value, RtError> {
    let k = hash_bytes_arg(&key, "hmacSha256", span)?;
    let m = hash_bytes_arg(&message, "hmacSha256", span)?;
    Ok(Value::Bytes(Rc::from(hmac_sha256(&k, &m).as_slice())))
}

/// §15 `Hash.crc32(data: Bytes) -> int` — unsigned CRC-32/ISO-HDLC.
pub fn builtin_hash_crc32(arg: Value, span: Span) -> Result<Value, RtError> {
    let bytes = hash_bytes_arg(&arg, "crc32", span)?;
    Ok(Value::Int(i64::from(crc32_iso_hdlc(&bytes))))
}

/// Pull the `Rc<[u8]>` out of a `Value::Bytes`, else a GUARD_TYPE fault. The
/// checker already proves every `Hash.x(...)` argument is a `Bytes`, so this fault
/// is reachable only on the `--unchecked` backstop — identical on both engines
/// because both call this one leaf.
pub(in crate::value) fn hash_bytes_arg(
    arg: &Value,
    name: &str,
    span: Span,
) -> Result<Rc<[u8]>, RtError> {
    match arg {
        Value::Bytes(b) => Ok(b.clone()),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Hash.{name}` takes a `Bytes`, found `{}`", other.kind()),
            span,
        )),
    }
}
