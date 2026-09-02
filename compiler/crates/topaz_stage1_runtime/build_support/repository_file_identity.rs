/// Returns the current working-file bytes after Git's repository identity rule.
///
/// The canonical rule and its cross-language unit test live in
/// `compiler/scripts/lib/generated-artifacts-validation.mjs`: tracked text
/// inputs replace CRLF pairs with LF while preserving lone CR bytes, and
/// `-text` binary inputs keep their raw bytes. Callers must classify the path
/// with its Git attributes and must not substitute HEAD blob contents when
/// collecting current-source provenance.
pub fn git_stored_bytes(bytes: Vec<u8>, text: bool) -> Vec<u8> {
    if !text {
        return bytes;
    }
    let Some(first) = bytes.windows(2).position(|pair| pair == b"\r\n") else {
        return bytes;
    };
    let mut normalized = Vec::with_capacity(bytes.len() - 1);
    normalized.extend_from_slice(&bytes[..first]);
    let mut input = first;
    while input < bytes.len() {
        if bytes.get(input..input + 2) == Some(b"\r\n") {
            normalized.push(b'\n');
            input += 2;
        } else {
            normalized.push(bytes[input]);
            input += 1;
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_text_crlf_and_preserves_binary_bytes() {
        let working = b"lf\ncrlf\r\nlone-cr\rend\r\n".to_vec();
        assert_eq!(
            git_stored_bytes(working.clone(), true),
            b"lf\ncrlf\nlone-cr\rend\n"
        );
        assert_eq!(git_stored_bytes(working.clone(), false), working);
    }
}
