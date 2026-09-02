use super::*;
use crate::*;

#[test]
fn application_quota_document_is_explicit_and_closed() {
    let json = format!(
        concat!(
            "{{\"schema\":\"{}\",",
            "\"concurrent_evaluations\":2,\"queued_evaluations\":2,",
            "\"total_evaluations\":16,\"aggregate_input_bytes\":65536,",
            "\"aggregate_result_bytes\":16000000,",
            "\"aggregate_output_bytes\":16000000,",
            "\"aggregate_transcript_bytes\":16000000,",
            "\"aggregate_safety_fuel\":16000000000,",
            "\"prepared_bytes\":1000000,\"wall_millis\":5000}}"
        ),
        APPLICATION_QUOTAS_SCHEMA
    );
    assert_eq!(
        ApplicationQuotas::parse_json(&json).expect("quota document"),
        application_quotas()
    );
    assert!(
        ApplicationQuotas::parse_json(
            &json.replace("\"wall_millis\":5000", "\"wall_millis\":5000,\"unknown\":1")
        )
        .is_err()
    );
    assert!(
        ApplicationQuotas::parse_json(&json.replace(
            "\"aggregate_safety_fuel\":16000000000",
            "\"aggregate_safety_fuel\":999999999"
        ))
        .is_err()
    );
    assert!(
        ApplicationQuotas::parse_json(&json.replace(
            "\"concurrent_evaluations\":2",
            "\"concurrent_evaluations\":0"
        ))
        .is_err()
    );
}

#[test]
fn limits_parser_is_closed_and_bounded() {
    assert_eq!(
        Limits::parse_json(&limits_json()).expect("maximum limits"),
        Limits::MAXIMUM
    );
    for bad in [
        limits_json().replace("\"schema\":", "\"unknown\":0,\"schema\":"),
        limits_json().replace("\"prepare_work\":1000000", "\"prepare_work\":1000001"),
        limits_json().replace("\"prepare_work\":1000000", "\"prepare_work\":-1"),
        limits_json().replace("\"prepare_work\":1000000", "\"prepare_work\":1.5"),
        limits_json().replace("\"prepare_work\":1000000", "\"prepare_work\":1e3"),
        limits_json().replace(
            "\"prepare_work\":1000000",
            "\"prepare_work\":1000000,\"prepare_work\":1000000",
        ),
        limits_json().replace("\"prepare_work\":1000000,", ""),
    ] {
        assert!(Limits::parse_json(&bad).is_err(), "{bad}");
    }
}
