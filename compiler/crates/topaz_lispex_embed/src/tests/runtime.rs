use super::*;
use crate::protocol::parse_response;
use crate::runtime::run_with_safety_fuel;
use crate::value_codec::hex_lower;
use crate::*;

#[test]
fn exact_complete_run_uses_two_fresh_instances_and_canonical_report() {
    let record = run(SOURCE, INPUT, Limits::MAXIMUM).expect("complete run");
    assert_eq!(record.category, SettledCategory::Complete);
    assert_eq!(record.operation, "evaluate");
    assert_eq!(record.fresh_instances, 2);
    assert_eq!(
        hex_lower(record.result.as_deref().expect("result")),
        RESULT_HEX
    );
    assert!(
        record
            .report_json
            .contains("\"portableProviderReceipt\":false")
    );
    assert!(record.report_json.contains("\"scope\":\"release\""));
    assert!(
        !record
            .report_json
            .contains(concat!("develop", "-integration"))
    );
    assert!(
        !record
            .report_json
            .contains(concat!("lispex-1.20", "-publication"))
    );
}

#[test]
fn deterministic_limit_exhaustion_has_no_result() {
    let mut limits = Limits::MAXIMUM;
    limits.evaluate.eval_work = 597;
    let record = run(SOURCE, INPUT, limits).expect("settled exhaustion");
    assert_eq!(record.category, SettledCategory::LimitExhaustion);
    assert!(record.result.is_none());
    assert!(
        record
            .report_json
            .contains("\"category\":\"limit-exhaustion\"")
    );
}

#[test]
fn malformed_value_refuses_before_runtime() {
    assert_eq!(
        run(SOURCE, &[0, 0], Limits::MAXIMUM),
        Err(RunError::InputRefusal("value-trailing"))
    );
}

#[test]
fn unsupported_profile_form_is_a_request_refusal() {
    assert_eq!(
        run(b"(set! value 1)\n", INPUT, Limits::MAXIMUM),
        Err(RunError::RequestRefusal("draft-profile".into()))
    );
}

#[test]
fn reduced_safety_fuel_is_engine_fault_without_report() {
    assert_eq!(
        run_with_safety_fuel(SOURCE, INPUT, Limits::MAXIMUM, 0),
        Err(RunError::EngineFault("safety-fuel-exhausted"))
    );
}

#[test]
fn repeated_runs_have_no_shared_evaluation_state() {
    let first = run(SOURCE, INPUT, Limits::MAXIMUM).expect("first");
    let second = run(SOURCE, INPUT, Limits::MAXIMUM).expect("second");
    assert_eq!(first, second);
    assert_eq!(first.fresh_instances, 2);
}

#[test]
fn prepared_rule_reuses_only_immutable_bytes_across_fresh_evaluations() {
    let runtime = ReusableRuntime::embedded().expect("runtime");
    let prepared = match runtime
        .prepare(SOURCE, Limits::MAXIMUM.prepare)
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => prepared,
        PrepareOutcome::LimitExhaustion(_) => panic!("unexpected preparation exhaustion"),
    };
    assert_eq!(prepared.component_sha256(), EVALUATOR_SHA256);
    assert_eq!(prepared.profile_id(), PROFILE_ID);
    assert!(!prepared.prepare_request_sha256().is_empty());
    assert!(!prepared.payload_sha256().is_empty());
    assert!(prepared.payload_len() > 0);
    let payload_sha256 = prepared.payload_sha256().to_string();
    let debug = format!("{prepared:?}");
    assert!(debug.contains(&payload_sha256));
    assert!(!debug.contains("payload:"));
    assert!(!debug.contains("binding_digests"));

    let first = runtime
        .evaluate(&prepared, INPUT, Limits::MAXIMUM.evaluate)
        .expect("first evaluation");
    let second = runtime
        .evaluate(&prepared, INPUT, Limits::MAXIMUM.evaluate)
        .expect("second evaluation");

    assert_eq!(first, second);
    assert_eq!(first.category, SettledCategory::Complete);
    assert_eq!(first.fresh_instances, 1);
    assert_eq!(prepared.payload_sha256(), payload_sha256);
    assert_eq!(
        hex_lower(first.result.as_deref().expect("result")),
        RESULT_HEX
    );
}

#[test]
fn prepared_rule_supports_concurrent_fresh_evaluations() {
    let runtime = ReusableRuntime::embedded().expect("runtime");
    let prepared = match runtime
        .prepare(SOURCE, Limits::MAXIMUM.prepare)
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => prepared,
        PrepareOutcome::LimitExhaustion(_) => panic!("unexpected preparation exhaustion"),
    };
    let workers = (0..4)
        .map(|_| {
            let prepared = prepared.clone();
            std::thread::spawn(move || runtime.evaluate(&prepared, INPUT, Limits::MAXIMUM.evaluate))
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker").expect("evaluation"))
        .collect::<Vec<_>>();
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(results.iter().all(|result| result.fresh_instances == 1));
}

#[test]
fn prepared_rule_rejects_a_component_binding_mismatch() {
    let runtime = ReusableRuntime::embedded().expect("runtime");
    let mut prepared = match runtime
        .prepare(SOURCE, Limits::MAXIMUM.prepare)
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => prepared,
        PrepareOutcome::LimitExhaustion(_) => panic!("unexpected preparation exhaustion"),
    };
    prepared.component_sha256 = "0";
    assert_eq!(
        runtime.evaluate(&prepared, INPUT, Limits::MAXIMUM.evaluate),
        Err(RunError::SelectionRefusal("prepared-component-digest"))
    );
}

#[test]
fn one_shot_facade_matches_direct_prepare_and_evaluate() {
    let runtime = ReusableRuntime::embedded().expect("runtime");
    let prepared = match runtime
        .prepare(SOURCE, Limits::MAXIMUM.prepare)
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => prepared,
        PrepareOutcome::LimitExhaustion(_) => panic!("unexpected preparation exhaustion"),
    };
    let direct = runtime
        .evaluate(&prepared, INPUT, Limits::MAXIMUM.evaluate)
        .expect("direct evaluation");
    let facade = run(SOURCE, INPUT, Limits::MAXIMUM).expect("facade");
    assert_eq!(facade.category, direct.category);
    assert_eq!(facade.code, direct.code);
    assert_eq!(facade.result, direct.result);
    assert_eq!(facade.fresh_instances, 2);
    assert!(
        facade
            .report_json
            .contains(prepared.prepare_request_sha256())
    );
    assert!(facade.report_json.contains(&direct.request_sha256));
}

#[test]
fn info_is_exact_and_has_no_selector_or_portable_receipt() {
    let info = info_json().expect("info");
    assert!(info.contains(EVALUATOR_SHA256));
    assert!(info.contains(CONTRACT_MANIFEST_SHA256));
    assert!(info.contains("\"selectors\":false"));
    assert!(info.contains("\"portableProviderReceipt\":false"));
    assert!(!info.contains(concat!("develop", "-integration")));
    assert!(!info.contains(concat!("lispex-1.20", "-publication")));
}

#[cfg(feature = "full-profile-contract")]
#[test]
fn full_profile_denominator_and_bounded_compatibility_identity_are_disjoint() {
    assert_eq!(FULL_LANGUAGE_PROFILE_ID, "lispex-profile-1.5");
    assert_eq!(FULL_PROFILE_DENOMINATOR.primitive_rows, 205);
    assert_eq!(FULL_PROFILE_DENOMINATOR.guest_calling_rows, 18);
    assert_eq!(FULL_PROFILE_DENOMINATOR.deferred_rows, 0);
    assert_eq!(
        FULL_PROFILE_DENOMINATOR.fixed_rows
            + FULL_PROFILE_DENOMINATOR.precharged_rows
            + FULL_PROFILE_DENOMINATOR.incremental_rows
            + FULL_PROFILE_DENOMINATOR.guest_calling_rows,
        FULL_PROFILE_DENOMINATOR.primitive_rows
    );
    assert_eq!(BOUNDED_APPLICATION_COMPATIBILITY_PROFILE_ID, PROFILE_ID);
    assert_ne!(
        FULL_PROFILE_ID,
        BOUNDED_APPLICATION_COMPATIBILITY_PROFILE_ID
    );

    let bounded = ReusableRuntime::embedded().expect("bounded runtime");
    let full = ReusableRuntime::full_profile().expect("full runtime");
    assert_eq!(bounded.profile_id(), PROFILE_ID);
    assert_eq!(bounded.evaluator_sha256(), EVALUATOR_SHA256);
    assert_eq!(full.profile_id(), FULL_PROFILE_ID);
    assert_eq!(full.evaluator_sha256(), FULL_EVALUATOR_SHA256);
}

#[test]
fn malformed_response_fails_closed() {
    assert_eq!(
        parse_response(b"LPXRSP01"),
        Err(RunError::ContractViolation("response framing is invalid"))
    );
}
