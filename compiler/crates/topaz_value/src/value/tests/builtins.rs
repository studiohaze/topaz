use super::*;

#[test]
fn bytes_codecs_pin_rfc4648_vectors() {
    // ★ ENCODING DETERMINISM PIN. The interpreter and the emitted Rust BOTH call
    // these same leaves, so a change to any output here is a change to BOTH engines
    // at once (run≡build), and CI catches an accidental drift. RFC 4648 §10 test
    // vectors anchor the base64 alphabet/padding; the hex is RFC 4648 §8 lowercase.
    let enc = |s: &str| builtin_bytes_encode_utf8(Value::str(s), SP).unwrap();
    let hex = |v: Value| match builtin_bytes_to_hex(v, SP).unwrap() {
        Value::Str(s) => s.to_string(),
        o => panic!("toHex returned {}", render(&o)),
    };
    let b64 = |v: Value| match builtin_bytes_to_base64(v, SP).unwrap() {
        Value::Str(s) => s.to_string(),
        o => panic!("toBase64 returned {}", render(&o)),
    };

    // RFC 4648 §10 base64 vectors over the UTF-8 bytes of the test strings.
    assert_eq!(b64(enc("")), "");
    assert_eq!(b64(enc("f")), "Zg==");
    assert_eq!(b64(enc("fo")), "Zm8=");
    assert_eq!(b64(enc("foo")), "Zm9v");
    assert_eq!(b64(enc("foob")), "Zm9vYg==");
    assert_eq!(b64(enc("fooba")), "Zm9vYmE=");
    assert_eq!(b64(enc("foobar")), "Zm9vYmFy");

    // Hex: lowercase, two digits/byte. `foobar` = 666f6f626172.
    assert_eq!(hex(enc("foobar")), "666f6f626172");
    assert_eq!(hex(enc("")), "");
    // 0x00..0xff round-trips losslessly (a non-UTF-8 byte string built from hex).
    let all = match builtin_bytes_from_hex(Value::str("00ff10"), SP).unwrap() {
        Value::Ok(b) => (*b).clone(),
        o => panic!("fromHex returned {}", render(&o)),
    };
    assert_eq!(hex(all.clone()), "00ff10");
    // The render form is `Bytes(<hex>)` (pinned, lossless).
    assert_eq!(render(&all), "Bytes(00ff10)");
    assert_eq!(render(&enc("foobar")), "Bytes(666f6f626172)");
    assert_eq!(render(&enc("")), "Bytes()");

    // toBase64 → fromBase64 round-trip (canonical), incl. each pad length.
    for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
        let encoded = b64(enc(s));
        let back = match builtin_bytes_from_base64(Value::str(&encoded), SP).unwrap() {
            Value::Ok(b) => (*b).clone(),
            o => panic!("fromBase64({encoded:?}) returned {}", render(&o)),
        };
        assert!(
            values_equal(&back, &enc(s)).unwrap(),
            "base64 round-trip drifted for {s:?}"
        );
    }
    // fromBase64 of an RFC vector yields the original bytes.
    let decoded = match builtin_bytes_from_base64(Value::str("Zm9vYmFy"), SP).unwrap() {
        Value::Ok(b) => (*b).clone(),
        o => panic!("fromBase64 returned {}", render(&o)),
    };
    assert_eq!(
        render(&builtin_bytes_decode_utf8(decoded, SP).unwrap()),
        "Ok(foobar)"
    );

    // fromHex accepts BOTH cases; rejects odd length + non-hex.
    assert!(matches!(
        builtin_bytes_from_hex(Value::str("DEADbeef"), SP).unwrap(),
        Value::Ok(_)
    ));
    assert!(matches!(
        builtin_bytes_from_hex(Value::str("abc"), SP).unwrap(),
        Value::Err(_) // odd length
    ));
    assert!(matches!(
        builtin_bytes_from_hex(Value::str("zz"), SP).unwrap(),
        Value::Err(_) // non-hex digit
    ));

    // fromBase64 rejects bad length / bad char / misplaced padding / non-canonical.
    for bad in ["Zg=", "Zg=a", "Z===", "@@@@", "Zm9=v"] {
        assert!(
            matches!(
                builtin_bytes_from_base64(Value::str(bad), SP).unwrap(),
                Value::Err(_)
            ),
            "fromBase64({bad:?}) must be Err"
        );
    }

    // decodeUtf8 of INVALID UTF-8 (a lone 0xff) → Err.
    let bad_utf8 = match builtin_bytes_from_hex(Value::str("ff"), SP).unwrap() {
        Value::Ok(b) => (*b).clone(),
        o => panic!("fromHex returned {}", render(&o)),
    };
    assert!(matches!(
        builtin_bytes_decode_utf8(bad_utf8, SP).unwrap(),
        Value::Err(_)
    ));

    // length / slice (clamped, never faults) / concat.
    let fb = enc("foobar");
    assert!(matches!(
        builtin_bytes_length(fb.clone(), SP).unwrap(),
        Value::Int(6)
    ));
    assert_eq!(
        hex(builtin_bytes_slice(fb.clone(), Value::Int(1), Value::Int(3), SP).unwrap()),
        "6f6f" // "oo"
    );
    // Out-of-range + inverted slice → empty (clamped, not a fault).
    assert_eq!(
        hex(builtin_bytes_slice(fb.clone(), Value::Int(0), Value::Int(99), SP).unwrap()),
        "666f6f626172"
    );
    assert_eq!(
        hex(builtin_bytes_slice(fb.clone(), Value::Int(4), Value::Int(2), SP).unwrap()),
        ""
    );
    assert_eq!(
        hex(builtin_bytes_concat(enc("foo"), enc("bar"), SP).unwrap()),
        "666f6f626172"
    );

    // Bytes equality is byte-wise; order is lexicographic; Bytes is keyable.
    assert!(values_equal(&enc("foo"), &enc("foo")).unwrap());
    assert!(!values_equal(&enc("foo"), &enc("bar")).unwrap());
    assert_eq!(
        values_compare(&enc("a"), &enc("b")).unwrap(),
        std::cmp::Ordering::Less
    );
    assert!(canonical_key(&enc("foo")).is_ok());
}

#[test]
fn compression_codecs_pin_canonical_stored_blocks() {
    let enc = |s: &str| builtin_bytes_encode_utf8(Value::str(s), SP).unwrap();
    let ok_bytes = |v: Value| match v {
        Value::Ok(b) => match &*b {
            Value::Bytes(bytes) => Value::Bytes(bytes.clone()),
            other => panic!("expected Ok(Bytes), got Ok({})", render(other)),
        },
        o => panic!("expected Ok(Bytes), got {}", render(&o)),
    };
    let hex = |v: Value| match builtin_bytes_to_hex(v, SP).unwrap() {
        Value::Str(s) => s.to_string(),
        o => panic!("toHex returned {}", render(&o)),
    };
    let raw_bytes = |v: Value| match v {
        Value::Bytes(bytes) => bytes,
        o => panic!("expected Bytes, got {}", render(&o)),
    };
    let from_hex = |s: &str| match builtin_bytes_from_hex(Value::str(s), SP).unwrap() {
        Value::Ok(b) => (*b).clone(),
        o => panic!("fromHex returned {}", render(&o)),
    };

    let empty = ok_bytes(builtin_codec_gzip_compress(enc(""), SP).unwrap());
    assert_eq!(hex(empty), "1f8b08000000000000ff010000ffff0000000000000000");

    let hello = ok_bytes(builtin_codec_gzip_compress(enc("hello"), SP).unwrap());
    assert_eq!(
        hex(hello.clone()),
        "1f8b08000000000000ff010500faff68656c6c6f86a6103605000000"
    );
    let roundtrip = ok_bytes(builtin_codec_gzip_decompress(hello, SP).unwrap());
    assert_eq!(
        render(&builtin_bytes_decode_utf8(roundtrip, SP).unwrap()),
        "Ok(hello)"
    );

    let bad = match builtin_bytes_from_hex(Value::str("00"), SP).unwrap() {
        Value::Ok(b) => (*b).clone(),
        o => panic!("fromHex returned {}", render(&o)),
    };
    assert_eq!(
        render(&builtin_codec_gzip_decompress(bad, SP).unwrap()),
        "Err(Codec.gzipDecompress: truncated gzip stream)"
    );
    let mut split_gzip = vec![0x1f, 0x8b, 0x08, 0x00];
    push_u32_le(&mut split_gzip, 0);
    split_gzip.extend_from_slice(&[0x00, 0xff]);
    split_gzip.extend_from_slice(&[
        0x00, 0x02, 0x00, 0xfd, 0xff, b't', b'o', 0x01, 0x03, 0x00, 0xfc, 0xff, b'p', b'a', b'z',
    ]);
    push_u32_le(&mut split_gzip, crc32_iso_hdlc(b"topaz"));
    push_u32_le(&mut split_gzip, 5);
    assert_eq!(
        render(
            &builtin_codec_gzip_decompress(Value::Bytes(Rc::from(split_gzip.as_slice())), SP)
                .unwrap()
        ),
        "Err(Codec.gzipDecompress: non-canonical stored block length)"
    );

    let empty_deflate = ok_bytes(builtin_codec_deflate_compress(enc(""), SP).unwrap());
    assert_eq!(hex(empty_deflate), "010000ffff");
    let hello_deflate = ok_bytes(builtin_codec_deflate_compress(enc("hello"), SP).unwrap());
    assert_eq!(hex(hello_deflate.clone()), "010500faff68656c6c6f");
    let inflated = ok_bytes(builtin_codec_deflate_decompress(hello_deflate, SP).unwrap());
    assert_eq!(
        render(&builtin_bytes_decode_utf8(inflated, SP).unwrap()),
        "Ok(hello)"
    );
    assert_eq!(
        render(
            &builtin_codec_deflate_decompress(from_hex("000200fdff746f010300fcff70617a"), SP)
                .unwrap()
        ),
        "Err(Codec.deflateDecompress: non-canonical stored block length)"
    );

    let empty_fixed = ok_bytes(builtin_codec_deflate_fixed_compress(enc(""), SP).unwrap());
    assert_eq!(hex(empty_fixed), "0300");
    let hello_fixed = ok_bytes(builtin_codec_deflate_fixed_compress(enc("hello"), SP).unwrap());
    assert_eq!(hex(hello_fixed), "cb48cdc9c90700");
    let empty_zlib = ok_bytes(builtin_codec_zlib_fixed_compress(enc(""), SP).unwrap());
    assert_eq!(hex(empty_zlib), "7801030000000001");
    let hello_zlib = ok_bytes(builtin_codec_zlib_fixed_compress(enc("hello"), SP).unwrap());
    assert_eq!(hex(hello_zlib), "7801cb48cdc9c90700062c0215");

    let repeated = Value::Bytes(Rc::from(vec![b'a'; 300].as_slice()));
    let repeated_once =
        ok_bytes(builtin_codec_deflate_fixed_compress(repeated.clone(), SP).unwrap());
    let repeated_twice = ok_bytes(builtin_codec_deflate_fixed_compress(repeated, SP).unwrap());
    let repeated_once_hex = hex(repeated_once);
    let repeated_twice_hex = hex(repeated_twice);
    assert_eq!(repeated_once_hex, repeated_twice_hex);
    assert_eq!(repeated_once_hex, "4b1c05440300");
    let high_bytes = Value::Bytes(Rc::from(vec![0xff; 258].as_slice()));
    let high_fixed = ok_bytes(builtin_codec_deflate_fixed_compress(high_bytes, SP).unwrap());
    assert_eq!(hex(high_fixed), "fb3fe20100");
    let binary = (0..81_920)
        .map(|index| ((index * 17 + index / 251) & 0xff) as u8)
        .collect::<Vec<_>>();
    let binary_value = Value::Bytes(Rc::from(binary.as_slice()));
    let binary_raw = raw_bytes(ok_bytes(
        builtin_codec_deflate_fixed_compress(binary_value.clone(), SP).unwrap(),
    ));
    let binary_zlib = raw_bytes(ok_bytes(
        builtin_codec_zlib_fixed_compress(binary_value, SP).unwrap(),
    ));
    assert_eq!(&binary_zlib[..2], &[0x78, 0x01]);
    assert_eq!(&binary_zlib[2..binary_zlib.len() - 4], binary_raw.as_ref());
    assert_eq!(
        &binary_zlib[binary_zlib.len() - 4..],
        &adler32(&binary).to_be_bytes()
    );

    assert!(!fixed_deflate_input_too_large(
        FIXED_DEFLATE_MAX_INPUT_BYTES
    ));
    assert!(fixed_deflate_input_too_large(
        FIXED_DEFLATE_MAX_INPUT_BYTES + 1
    ));

    let one = Value::Bytes(Rc::from([1u8].as_slice()));
    let one_protected = raw_bytes(ok_bytes(
        builtin_codec_reed_solomon_255_223_protect(one, SP).unwrap(),
    ));
    assert_eq!(one_protected.len(), 255);
    assert_eq!(one_protected[0], 1);
    assert!(one_protected[1..223].iter().all(|byte| *byte == 0));
    assert_eq!(
        hex(Value::Bytes(Rc::from(&one_protected[223..]))),
        "138fb43bdd1d312de70949499f029e88d4da0e71d714bb3789b5cb7161870efb"
    );
    let sequence = Value::Bytes(Rc::from((0u8..=222).collect::<Vec<_>>().as_slice()));
    let sequence_protected = raw_bytes(ok_bytes(
        builtin_codec_reed_solomon_255_223_protect(sequence, SP).unwrap(),
    ));
    assert_eq!(
        hex(Value::Bytes(Rc::from(&sequence_protected[223..]))),
        "41841183b11fdb537421939696cda70e1db5c86684af222564b89cc6069f172e"
    );
    let sequence_again = raw_bytes(ok_bytes(
        builtin_codec_reed_solomon_255_223_protect(
            Value::Bytes(Rc::from((0u8..=222).collect::<Vec<_>>().as_slice())),
            SP,
        )
        .unwrap(),
    ));
    assert_eq!(sequence_protected, sequence_again);
    let two_shards = Value::Bytes(Rc::from(
        (0..224)
            .map(|value| value as u8)
            .collect::<Vec<_>>()
            .as_slice(),
    ));
    let two_shards_protected = raw_bytes(ok_bytes(
        builtin_codec_reed_solomon_255_223_protect(two_shards, SP).unwrap(),
    ));
    assert_eq!(two_shards_protected.len(), 510);
    assert_eq!(two_shards_protected[255], 223);
    assert!(two_shards_protected[256..478].iter().all(|byte| *byte == 0));
    assert_eq!(
        hex(Value::Bytes(Rc::from(&two_shards_protected[478..]))),
        "0d3e675935434cd0b369b0b04fa390195c124e95202af6b4c6b8bc95e4884e2f"
    );
    assert_eq!(
        render(&builtin_codec_reed_solomon_255_223_protect(enc(""), SP).unwrap()),
        "Err(Codec.reedSolomon255223Protect: input must not be empty)"
    );
    assert_eq!(REED_SOLOMON_MAX_INPUT_BYTES, 14_614_305);
    let oversized = Value::Bytes(Rc::from(
        vec![0u8; REED_SOLOMON_MAX_INPUT_BYTES + 1].into_boxed_slice(),
    ));
    assert_eq!(
        render(&builtin_codec_reed_solomon_255_223_protect(oversized, SP).unwrap()),
        "Err(Codec.reedSolomon255223Protect: input requires more than 65535 shards)"
    );

    let empty_zstd = ok_bytes(builtin_codec_zstd_compress(enc(""), Value::Int(3), SP).unwrap());
    assert_eq!(hex(empty_zstd), "28b52ffd2000010000");
    let hello_zstd =
        ok_bytes(builtin_codec_zstd_compress(enc("hello"), Value::Int(3), SP).unwrap());
    assert_eq!(hex(hello_zstd.clone()), "28b52ffd200529000068656c6c6f");
    let decoded_zstd = ok_bytes(builtin_codec_zstd_decompress(hello_zstd, SP).unwrap());
    assert_eq!(
        render(&builtin_bytes_decode_utf8(decoded_zstd, SP).unwrap()),
        "Ok(hello)"
    );
    assert_eq!(
        render(
            &builtin_codec_zstd_decompress(from_hex("28b52ffd200510000068651900006c6c6f"), SP)
                .unwrap()
        ),
        "Err(Codec.zstdDecompress: non-canonical raw block length)"
    );
    assert_eq!(
        render(&builtin_codec_zstd_compress(enc("x"), Value::Int(0), SP).unwrap()),
        "Err(Codec.zstdCompress: level must be between 1 and 22)"
    );
    let long = Value::Bytes(Rc::from(vec![b'a'; 300].as_slice()));
    let long_zstd = ok_bytes(builtin_codec_zstd_compress(long, Value::Int(3), SP).unwrap());
    assert!(
        hex(long_zstd).starts_with("28b52ffd602c00610900"),
        "300-byte zstd frames use the canonical 2-byte FCS header"
    );
}

#[test]
fn hash_leaves_pin_official_vectors() {
    // ★ HASH CORRECTNESS PIN. run≡build only proves both engines agree; these
    // known-answer vectors prove the shared in-house leaf agrees with SHA/HMAC.
    let enc = |s: &str| builtin_bytes_encode_utf8(Value::str(s), SP).unwrap();
    let from_hex = |s: &str| match builtin_bytes_from_hex(Value::str(s), SP).unwrap() {
        Value::Ok(b) => (*b).clone(),
        o => panic!("fromHex returned {}", render(&o)),
    };
    let hex = |v: Value| match builtin_bytes_to_hex(v, SP).unwrap() {
        Value::Str(s) => s.to_string(),
        o => panic!("toHex returned {}", render(&o)),
    };

    // FIPS 180-4 SHA-256 known-answer vectors.
    assert_eq!(
        hex(builtin_hash_sha256(enc(""), SP).unwrap()),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(builtin_hash_sha256(enc("abc"), SP).unwrap()),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(builtin_hash_sha256(
            enc("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            SP
        )
        .unwrap()),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );

    // FIPS 180-4 SHA-512 known-answer vectors, including a 1024-bit-block path.
    assert_eq!(
        hex(builtin_hash_sha512(enc("abc"), SP).unwrap()),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
    assert_eq!(
        hex(builtin_hash_sha512(
            enc("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            SP
        )
        .unwrap()),
        "204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c33596fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445"
    );

    // RFC 4231 HMAC-SHA256 test cases 1 and 2, plus the long-key hash-first path.
    assert_eq!(
        hex(builtin_hash_hmac_sha256(
            from_hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"),
            enc("Hi There"),
            SP
        )
        .unwrap()),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hex(
            builtin_hash_hmac_sha256(enc("Jefe"), enc("what do ya want for nothing?"), SP).unwrap()
        ),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
            hex(builtin_hash_hmac_sha256(from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), enc("Test Using Larger Than Block-Size Key - Hash Key First"), SP).unwrap()),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );

    // CRC-32/ISO-HDLC check values, including unsigned and binary-byte paths.
    let crc = |value: Value| match builtin_hash_crc32(value, SP).unwrap() {
        Value::Int(value) => value,
        other => panic!("crc32 returned {}", render(&other)),
    };
    assert_eq!(crc(enc("")), 0);
    assert_eq!(crc(enc("123456789")), 3_421_780_262);
    assert_eq!(crc(enc("hello")), 907_060_870);
    assert_eq!(crc(from_hex("00ff10")), 1_909_601_284);
    assert_eq!(crc(enc("123456789")), crc(enc("123456789")));

    // The unchecked backstop faults on non-Bytes, keeping both engines aligned.
    assert_eq!(
        builtin_hash_sha256(Value::str("abc"), SP).unwrap_err().code,
        codes::GUARD_TYPE
    );
    assert_eq!(
        builtin_hash_crc32(Value::str("abc"), SP).unwrap_err().code,
        codes::GUARD_TYPE
    );
}

#[test]
fn cli_and_path_leaves_are_pure_and_pinned() {
    // `Cli` is deliberately deterministic: it parses the caller-provided argv
    // vector and never reads ambient process state.
    let argv = Value::array(vec![
        Value::str("--verbose"),
        Value::str("--out"),
        Value::str("file"),
        Value::str("input"),
        Value::str("--include=a"),
        Value::str("--include"),
        Value::str("b"),
        Value::str("--"),
        Value::str("--literal"),
    ]);
    assert_eq!(
        render(&builtin_cli_has_flag(argv.clone(), Value::str("--verbose"), SP).unwrap()),
        "true"
    );
    assert_eq!(
        render(&builtin_cli_option(argv.clone(), Value::str("--out"), SP).unwrap()),
        "Some(file)"
    );
    assert_eq!(
        render(&builtin_cli_options(argv.clone(), Value::str("--include"), SP).unwrap()),
        "[a, b]"
    );
    // The simple argv policy treats an option followed by a non-option as
    // consuming that value; `--` switches the rest to positionals.
    assert_eq!(
        render(&builtin_cli_positionals(argv.clone(), SP).unwrap()),
        "[input, --literal]"
    );
    assert_eq!(
        render(
            &builtin_cli_option(
                Value::array(vec![Value::str("--out"), Value::str("--verbose")]),
                Value::str("--out"),
                SP,
            )
            .unwrap()
        ),
        "None"
    );

    let path_ok = |v: Value| match v {
        Value::Ok(p) => (*p).clone(),
        o => panic!("expected Ok(Path), got {}", render(&o)),
    };
    let path_some = |v: Value| match v {
        Value::Some(p) => (*p).clone(),
        o => panic!("expected Some(...), got {}", render(&o)),
    };

    let main = path_ok(builtin_path_from(Value::str("src//./main.tpz"), SP).unwrap());
    assert_eq!(render(&main), "Path(src/main.tpz)");
    assert_eq!(
        render(&builtin_path_to_string(main.clone(), SP).unwrap()),
        "src/main.tpz"
    );

    let out = path_ok(builtin_path_from(Value::str("src/../out/result.txt"), SP).unwrap());
    assert_eq!(
        render(&builtin_path_parent(out.clone(), SP).unwrap()),
        "Some(Path(out))"
    );
    assert_eq!(
        render(&builtin_path_file_name(out.clone(), SP).unwrap()),
        "Some(result.txt)"
    );
    assert_eq!(
        render(&builtin_path_extension(out.clone(), SP).unwrap()),
        "Some(txt)"
    );
    assert_eq!(
        render(&path_ok(
            builtin_path_with_extension(out.clone(), Value::str("md"), SP).unwrap()
        )),
        "Path(out/result.md)"
    );
    assert_eq!(
        render(&path_ok(
            builtin_path_join(
                path_some(builtin_path_parent(out.clone(), SP).unwrap()),
                Value::str("next/log.txt"),
                SP
            )
            .unwrap()
        )),
        "Path(out/next/log.txt)"
    );

    assert!(matches!(
        builtin_path_from(Value::str("../escape"), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_path_from(Value::str("/abs"), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_path_from(Value::str("C:\\temp"), SP).unwrap(),
        Value::Err(_)
    ));

    let a = path_ok(builtin_path_from(Value::str("a"), SP).unwrap());
    let b = path_ok(builtin_path_from(Value::str("b"), SP).unwrap());
    assert!(values_equal(&a, &a).unwrap());
    assert_eq!(values_compare(&a, &b).unwrap(), std::cmp::Ordering::Less);
    assert!(canonical_key(&a).is_ok());
}

#[test]
fn regex_leaves_pin_scalar_offsets_and_captures() {
    let regex_ok = |v: Value| match v {
        Value::Ok(re) => (*re).clone(),
        o => panic!("expected Ok(Regex), got {}", render(&o)),
    };
    let some_match = |v: Value| match v {
        Value::Some(m) => (*m).clone(),
        o => panic!("expected Some(Match), got {}", render(&o)),
    };

    let digits = regex_ok(builtin_regex_compile(Value::str("\\d+"), SP).unwrap());
    assert_eq!(
        render(&builtin_regex_is_match(digits.clone(), Value::str("abc123"), SP).unwrap()),
        "true"
    );
    let m = some_match(builtin_regex_find(digits.clone(), Value::str("é12"), SP).unwrap());
    assert_eq!(
        render(&member_value(&m, "start", SP).unwrap().unwrap()),
        "1"
    );
    assert_eq!(render(&member_value(&m, "end", SP).unwrap().unwrap()), "3");
    assert_eq!(
        render(&member_value(&m, "text", SP).unwrap().unwrap()),
        "12"
    );
    assert_eq!(
        render(&builtin_regex_find_all(digits.clone(), Value::str("a1 b22"), SP).unwrap()),
        "[Match { start: 1, end: 2, text: 1, groups: [], named: Map{} }, Match { start: 4, end: 6, text: 22, groups: [], named: Map{} }]"
    );
    assert_eq!(
        render(
            &builtin_regex_replace_all(digits.clone(), Value::str("a1b22"), Value::str("#"), SP,)
                .unwrap()
        ),
        "a#b#"
    );

    let comma_space = regex_ok(builtin_regex_compile(Value::str(",\\s*"), SP).unwrap());
    assert_eq!(
        render(&builtin_regex_split(comma_space, Value::str("a, b,c"), SP).unwrap()),
        "[a, b, c]"
    );

    let named =
        regex_ok(builtin_regex_compile(Value::str("(?P<word>[A-Za-z]+)-(\\d+)"), SP).unwrap());
    let m = some_match(builtin_regex_find(named, Value::str("abc-42"), SP).unwrap());
    assert_eq!(
        render(&member_value(&m, "groups", SP).unwrap().unwrap()),
        "[Some(abc), Some(42)]"
    );
    assert_eq!(
        render(&member_value(&m, "named", SP).unwrap().unwrap()),
        "Map{word: abc}"
    );
    assert_eq!(
        render(&builtin_json_stringify(m.clone())),
        "Ok({\"end\":6,\"groups\":[\"abc\",\"42\"],\"named\":{\"word\":\"abc\"},\"start\":0,\"text\":\"abc-42\"})"
    );
    assert!(values_equal(&m, &m).unwrap());
    assert_eq!(
        canonical_key(&m).err(),
        Some(CmpError::NotComparable("Match"))
    );

    assert!(matches!(
        builtin_regex_compile(Value::str("[abc"), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_regex_compile(Value::str("\\D+"), SP).unwrap(),
        Value::Err(_)
    ));
    assert!(matches!(
        builtin_regex_compile(Value::str("(?P<x>a)(?<x>b)"), SP).unwrap(),
        Value::Err(_)
    ));
    let alt = regex_ok(builtin_regex_compile(Value::str("a|b"), SP).unwrap());
    assert_eq!(
        render(&builtin_regex_is_match(alt, Value::str("b"), SP).unwrap()),
        "true"
    );
}

#[test]
fn data_format_leaves_pin_csv_toml_and_url() {
    let ok = |v: Value| match v {
        Value::Ok(inner) => (*inner).clone(),
        o => panic!("expected Ok(...), got {}", render(&o)),
    };

    let rows = ok(builtin_csv_parse(Value::str("name,age\nAda,36\n\"B, C\",7"), SP).unwrap());
    assert_eq!(
        render(&builtin_json_stringify(rows)),
        "Ok([[\"name\",\"age\"],[\"Ada\",\"36\"],[\"B, C\",\"7\"]])"
    );
    let trailing = ok(builtin_csv_parse(Value::str("a,b\n"), SP).unwrap());
    assert_eq!(
        render(&builtin_json_stringify(trailing)),
        "Ok([[\"a\",\"b\"]])"
    );
    let keyed = ok(builtin_csv_parse_with_header(Value::str("name,age\nAda,36"), SP).unwrap());
    assert_eq!(
        render(&builtin_json_stringify(keyed)),
        "Ok([{\"age\":\"36\",\"name\":\"Ada\"}])"
    );
    assert_eq!(
        render(
            &builtin_csv_stringify(
                Value::array(vec![Value::array(vec![
                    Value::str("a,b"),
                    Value::str("c\"d"),
                ])]),
                SP,
            )
            .unwrap()
        ),
        "\"a,b\",\"c\"\"d\""
    );

    let toml = ok(builtin_toml_parse(
            Value::str(
                "name = \"Ada\"\ndep = { version = \"1.2.0\" }\n[db]\nport = 5432\nflags = [true, false]\n",
            ),
            SP,
        )
        .unwrap());
    let json = builtin_toml_to_json(toml.clone(), SP).unwrap();
    assert_eq!(
        render(&builtin_json_stringify(json)),
        "Ok({\"db\":{\"flags\":[true,false],\"port\":5432},\"dep\":{\"version\":\"1.2.0\"},\"name\":\"Ada\"})"
    );
    let lock = ok(builtin_toml_parse(
        Value::str("[[package]]\nname = \"root\"\n[[package]]\nname = \"dep\"\n"),
        SP,
    )
    .unwrap());
    let lock_json = builtin_toml_to_json(lock, SP).unwrap();
    assert_eq!(
        render(&builtin_json_stringify(lock_json)),
        "Ok({\"package\":[{\"name\":\"root\"},{\"name\":\"dep\"}]})"
    );
    let toml_text = ok(builtin_toml_stringify(toml, SP).unwrap());
    assert!(render(&toml_text).contains("[db]"));
    let dotted =
        ok(builtin_toml_parse(Value::str("\"a.b\" = 1\nplain.child = \"x\"\n"), SP).unwrap());
    let dotted_json = builtin_toml_to_json(dotted.clone(), SP).unwrap();
    assert_eq!(
        render(&builtin_json_stringify(dotted_json.clone())),
        "Ok({\"a.b\":1,\"plain\":{\"child\":\"x\"}})"
    );
    let dotted_text = ok(builtin_toml_stringify(dotted, SP).unwrap());
    assert!(render(&dotted_text).contains("\"a.b\" = 1"));
    let reparsed_dotted = ok(builtin_toml_parse(dotted_text, SP).unwrap());
    assert_eq!(
        render(&builtin_json_stringify(
            builtin_toml_to_json(reparsed_dotted, SP).unwrap()
        )),
        render(&builtin_json_stringify(dotted_json))
    );

    let url = ok(builtin_url_parse(
        Value::str("HTTPS://Example.COM:443/a/b?q=topaz&tag=a&tag=b#frag"),
        SP,
    )
    .unwrap());
    assert_eq!(
        render(&builtin_url_scheme(url.clone(), SP).unwrap()),
        "https"
    );
    assert_eq!(
        render(&builtin_url_host(url.clone(), SP).unwrap()),
        "Some(example.com)"
    );
    assert_eq!(render(&builtin_url_path(url.clone(), SP).unwrap()), "/a/b");
    assert_eq!(
        render(&builtin_json_stringify(
            builtin_url_query(url.clone(), SP).unwrap()
        )),
        "Ok({\"q\":[\"topaz\"],\"tag\":[\"a\",\"b\"]})"
    );
    let same_url = ok(builtin_url_parse(
        Value::str("https://example.com:443/a/b?q=topaz&tag=a&tag=b#frag"),
        SP,
    )
    .unwrap());
    assert_eq!(
        values_compare(&same_url, &url).unwrap(),
        std::cmp::Ordering::Equal
    );
    let lower_url = ok(builtin_url_parse(Value::str("https://example.com:443/a/a"), SP).unwrap());
    assert_eq!(
        values_compare(&lower_url, &url).unwrap(),
        std::cmp::Ordering::Less
    );
    assert!(keys_equal(
        &canonical_key(&same_url).expect("same URL is keyable"),
        &canonical_key(&url).expect("URL is keyable")
    ));
    assert_eq!(
        render(&builtin_url_to_string(url, SP).unwrap()),
        "https://example.com:443/a/b?q=topaz&tag=a&tag=b#frag"
    );
}

#[test]
fn byte_buffer_alias_snapshot_overlap_and_atomic_failure() {
    let buffer = builtin_byte_buffer_allocate(Value::Int(6), Some(Value::Int(0)), SP).unwrap();
    builtin_byte_buffer_fill(
        buffer.clone(),
        Value::Int(0),
        Value::Int(6),
        Value::Int(1),
        SP,
    )
    .unwrap();
    builtin_byte_buffer_set(buffer.clone(), Value::Int(0), Value::Int(9), SP).unwrap();
    builtin_byte_buffer_set(buffer.clone(), Value::Int(1), Value::Int(8), SP).unwrap();

    let alias = buffer.clone();
    builtin_byte_buffer_copy(
        alias,
        buffer.clone(),
        Value::Int(0),
        Value::Int(2),
        Value::Int(4),
        SP,
    )
    .unwrap();
    let first = builtin_byte_buffer_to_bytes(buffer.clone(), SP).unwrap();
    assert_eq!(render(&first), "Bytes(090809080101)");

    let copied = builtin_byte_buffer_from_bytes(first.clone(), SP).unwrap();
    builtin_byte_buffer_set(copied.clone(), Value::Int(0), Value::Int(7), SP).unwrap();
    assert_eq!(render(&first), "Bytes(090809080101)");
    assert_eq!(
        render(&builtin_byte_buffer_to_bytes(copied, SP).unwrap()),
        "Bytes(070809080101)"
    );

    let before = render(&builtin_byte_buffer_to_bytes(buffer.clone(), SP).unwrap());
    assert!(
        builtin_byte_buffer_fill(
            buffer.clone(),
            Value::Int(0),
            Value::Int(6),
            Value::Int(256),
            SP,
        )
        .is_err()
    );
    assert_eq!(
        render(&builtin_byte_buffer_to_bytes(buffer.clone(), SP).unwrap()),
        before
    );
    assert!(
        builtin_byte_buffer_copy(
            buffer.clone(),
            buffer.clone(),
            Value::Int(0),
            Value::Int(6),
            Value::Int(1),
            SP,
        )
        .is_err()
    );
    assert_eq!(
        render(&builtin_byte_buffer_to_bytes(buffer.clone(), SP).unwrap()),
        before
    );
    builtin_byte_buffer_fill(buffer, Value::Int(6), Value::Int(0), Value::Int(255), SP).unwrap();
}

#[test]
fn byte_buffer_rejects_every_invalid_boundary_without_partial_writes() {
    for (length, value) in [(-1, 0), (1, -1), (1, 256)] {
        assert!(
            builtin_byte_buffer_allocate(Value::Int(length), Some(Value::Int(value)), SP).is_err(),
            "allocate({length}, {value}) must fault"
        );
    }

    let buffer = builtin_byte_buffer_allocate(Value::Int(4), Some(Value::Int(7)), SP).unwrap();
    let snapshot = || render(&builtin_byte_buffer_to_bytes(buffer.clone(), SP).unwrap());
    let before = snapshot();

    for index in [-1, 4] {
        assert!(
            builtin_byte_buffer_get(buffer.clone(), Value::Int(index), SP).is_err(),
            "get({index}) must fault"
        );
        assert!(
            builtin_byte_buffer_set(buffer.clone(), Value::Int(index), Value::Int(1), SP,).is_err(),
            "set({index}, 1) must fault"
        );
        assert_eq!(snapshot(), before);
    }
    for byte in [-1, 256] {
        assert!(
            builtin_byte_buffer_set(buffer.clone(), Value::Int(0), Value::Int(byte), SP,).is_err(),
            "set(0, {byte}) must fault"
        );
        assert_eq!(snapshot(), before);
    }

    for (start, length, byte) in [
        (-1, 0, 1),
        (0, -1, 1),
        (5, 0, 1),
        (3, 2, 1),
        (0, 4, -1),
        (0, 4, 256),
    ] {
        assert!(
            builtin_byte_buffer_fill(
                buffer.clone(),
                Value::Int(start),
                Value::Int(length),
                Value::Int(byte),
                SP,
            )
            .is_err(),
            "fill({start}, {length}, {byte}) must fault"
        );
        assert_eq!(snapshot(), before);
    }

    for (source_start, target_start, length) in [
        (-1, 0, 0),
        (0, -1, 0),
        (0, 0, -1),
        (5, 0, 0),
        (0, 5, 0),
        (3, 0, 2),
        (0, 3, 2),
    ] {
        assert!(
            builtin_byte_buffer_copy(
                buffer.clone(),
                buffer.clone(),
                Value::Int(source_start),
                Value::Int(target_start),
                Value::Int(length),
                SP,
            )
            .is_err(),
            "copy({source_start}, {target_start}, {length}) must fault"
        );
        assert_eq!(snapshot(), before);
    }

    // Both end positions admit a zero-length operation; the full range is
    // valid and neither case changes the fixed length.
    builtin_byte_buffer_fill(
        buffer.clone(),
        Value::Int(4),
        Value::Int(0),
        Value::Int(255),
        SP,
    )
    .unwrap();
    builtin_byte_buffer_copy(
        buffer.clone(),
        buffer.clone(),
        Value::Int(4),
        Value::Int(4),
        Value::Int(0),
        SP,
    )
    .unwrap();
    builtin_byte_buffer_fill(
        buffer.clone(),
        Value::Int(0),
        Value::Int(4),
        Value::Int(9),
        SP,
    )
    .unwrap();
    assert_eq!(snapshot(), "Bytes(09090909)");
    assert_eq!(
        render(&builtin_byte_buffer_length(buffer, SP).unwrap()),
        "4"
    );
}

#[test]
fn byte_buffer_raw_get_leaf_matches_the_tagged_wrapper() {
    let buffer = builtin_byte_buffer_allocate(Value::Int(3), Some(Value::Int(17)), SP).unwrap();
    assert_eq!(
        builtin_byte_buffer_get_i64(&buffer, Value::Int(1), SP).unwrap(),
        17
    );
    assert_eq!(
        render(&builtin_byte_buffer_get(buffer.clone(), Value::Int(1), SP).unwrap()),
        "17"
    );
    for index in [Value::Int(-1), Value::Int(3), Value::Bool(true)] {
        let direct = builtin_byte_buffer_get_i64(&buffer, index.clone(), SP)
            .expect_err("raw get must reject invalid index");
        let tagged = builtin_byte_buffer_get(buffer.clone(), index, SP)
            .expect_err("tagged get must reject invalid index");
        assert_eq!(direct.code, tagged.code);
        assert_eq!(direct.message, tagged.message);
        assert_eq!(direct.span, tagged.span);
    }
}

#[test]
fn exact_byte_handle_cores_preserve_boxed_results_and_atomic_failures() {
    let bytes = Value::Bytes(Rc::from([0_u8, 17, 255, 9].as_slice()));
    assert_eq!(builtin_bytes_length_i64(&bytes, SP).unwrap(), 4);
    assert_eq!(
        render(&builtin_bytes_get_i64(&bytes, 2, SP).unwrap()),
        render(&builtin_bytes_get(bytes.clone(), Value::Int(2), SP).unwrap())
    );
    assert_eq!(
        render(&builtin_bytes_get_i64(&bytes, -1, SP).unwrap()),
        "None"
    );
    assert_eq!(
        render(&builtin_bytes_slice_i64(&bytes, -3, 99, SP).unwrap()),
        render(&builtin_bytes_slice(bytes.clone(), Value::Int(-3), Value::Int(99), SP).unwrap())
    );

    let buffer = builtin_byte_buffer_from_bytes(bytes, SP).unwrap();
    builtin_byte_buffer_set_i64(&buffer, 0, 8, SP).unwrap();
    builtin_byte_buffer_fill_i64(&buffer, 1, 2, 7, SP).unwrap();
    assert_eq!(builtin_byte_buffer_length_i64(&buffer, SP).unwrap(), 4);
    assert_eq!(builtin_byte_buffer_get_raw_i64(&buffer, 2, SP).unwrap(), 7);
    assert_eq!(
        render(&builtin_byte_buffer_to_bytes_ref(&buffer, SP).unwrap()),
        "Bytes(08070709)"
    );

    builtin_byte_buffer_copy_i64(&buffer, &buffer, 0, 1, 3, SP).unwrap();
    assert_eq!(
        render(&builtin_byte_buffer_to_bytes_ref(&buffer, SP).unwrap()),
        "Bytes(08080707)"
    );

    let before = render(&builtin_byte_buffer_to_bytes_ref(&buffer, SP).unwrap());
    for result in [
        builtin_byte_buffer_set_i64(&buffer, 0, 256, SP),
        builtin_byte_buffer_fill_i64(&buffer, 1, 9, 1, SP),
        builtin_byte_buffer_copy_i64(&buffer, &buffer, 0, 3, 2, SP),
    ] {
        assert!(result.is_err());
        assert_eq!(
            render(&builtin_byte_buffer_to_bytes_ref(&buffer, SP).unwrap()),
            before
        );
    }

    let frozen = builtin_byte_buffer_to_bytes_ref(&buffer, SP).unwrap();
    builtin_byte_buffer_set_i64(&buffer, 0, 1, SP).unwrap();
    assert_eq!(render(&frozen), "Bytes(08080707)");
}

#[test]
fn byte_buffer_overlap_is_memmove_in_both_directions() {
    let original = Value::Bytes(Rc::from([1_u8, 2, 3, 4, 5, 6].as_slice()));
    let forward = builtin_byte_buffer_from_bytes(original.clone(), SP).unwrap();
    builtin_byte_buffer_copy(
        forward.clone(),
        forward.clone(),
        Value::Int(0),
        Value::Int(2),
        Value::Int(4),
        SP,
    )
    .unwrap();
    assert_eq!(
        render(&builtin_byte_buffer_to_bytes(forward, SP).unwrap()),
        "Bytes(010201020304)"
    );

    let backward = builtin_byte_buffer_from_bytes(original, SP).unwrap();
    builtin_byte_buffer_copy(
        backward.clone(),
        backward.clone(),
        Value::Int(2),
        Value::Int(0),
        Value::Int(4),
        SP,
    )
    .unwrap();
    assert_eq!(
        render(&builtin_byte_buffer_to_bytes(backward, SP).unwrap()),
        "Bytes(030405060506)"
    );
}
