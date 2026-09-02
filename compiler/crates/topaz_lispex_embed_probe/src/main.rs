use topaz_lispex_embed_probe::{
    ABI_ID, COMPONENT_ID, EVALUATOR_SHA256, GOLDEN_INPUT, GOLDEN_SOURCE, MODEL_ID, PROFILE_ID,
    RECEIPT_ID, RUNTIME_ID, RUNTIME_POLICY_ID, SAFETY_FUEL, Selection, VALUE_CODEC_ID, hex_lower,
    run_golden_probe, sha256_hex,
};

fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("private probe accepts no artifact, profile, runtime, or fallback arguments");
        std::process::exit(64);
    }
    let evidence = match run_golden_probe(&Selection::exact()) {
        Ok(evidence) => evidence,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!(
        concat!(
            "{{",
            "\"schema\":\"topaz.lispex-embedding-private-probe/v0\",",
            "\"status\":\"passed\",",
            "\"component\":{{",
            "\"id\":\"{}\",",
            "\"artifactSha256\":\"{}\",",
            "\"imports\":0",
            "}},",
            "\"contract\":{{",
            "\"profile\":\"{}\",",
            "\"model\":\"{}\",",
            "\"abi\":\"{}\",",
            "\"valueCodec\":\"{}\",",
            "\"receiptSchema\":\"{}\",",
            "\"stability\":\"draft-v0\"",
            "}},",
            "\"runtime\":{{",
            "\"id\":\"{}\",",
            "\"policy\":\"{}\",",
            "\"safetyFuel\":\"{}\",",
            "\"freshInstances\":{},",
            "\"fallback\":false",
            "}},",
            "\"probe\":{{",
            "\"sourceSha256\":\"{}\",",
            "\"inputSha256\":\"{}\",",
            "\"prepareRequestSha256\":\"{}\",",
            "\"evaluateRequestSha256\":\"{}\",",
            "\"prepareCategory\":\"{}\",",
            "\"evaluateCategory\":\"{}\",",
            "\"resultSha256\":\"{}\",",
            "\"resultHex\":\"{}\"",
            "}},",
            "\"claimBoundary\":{{",
            "\"private\":true,",
            "\"portablePublicReceipt\":false,",
            "\"componentAdmission\":false,",
            "\"publicCompatibility\":false,",
            "\"publicDistribution\":false,",
            "\"topazRun\":false",
            "}}",
            "}}"
        ),
        COMPONENT_ID,
        EVALUATOR_SHA256,
        PROFILE_ID,
        MODEL_ID,
        ABI_ID,
        VALUE_CODEC_ID,
        RECEIPT_ID,
        RUNTIME_ID,
        RUNTIME_POLICY_ID,
        SAFETY_FUEL,
        evidence.fresh_instances,
        sha256_hex(GOLDEN_SOURCE),
        sha256_hex(GOLDEN_INPUT),
        evidence.prepare_request_sha256,
        evidence.evaluate_request_sha256,
        evidence.prepare.category.as_str(),
        evidence.evaluate.category.as_str(),
        evidence.result_sha256,
        hex_lower(&evidence.evaluate.payload),
    );
}
