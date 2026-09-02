use super::*;
use crate::value_codec::hex_lower;
use crate::*;

#[test]
fn application_quota_refusal_is_charge_before_and_cache_is_exact() {
    let prepared = prepared(SOURCE);
    let mut quotas = application_quotas();
    quotas.concurrent_evaluations = 1;
    quotas.queued_evaluations = 0;
    quotas.total_evaluations = 2;
    quotas.aggregate_input_bytes = 2;
    quotas.aggregate_result_bytes = 2 * Limits::MAXIMUM.evaluate.result_bytes;
    quotas.aggregate_output_bytes = 2 * Limits::MAXIMUM.evaluate.output_bytes;
    quotas.aggregate_transcript_bytes = 2 * Limits::MAXIMUM.evaluate.transcript_bytes;
    quotas.aggregate_safety_fuel = 2 * SAFETY_FUEL;
    quotas.prepared_bytes = prepared.payload_len() as u64;
    let application = ApplicationRuntime::new(quotas).expect("application");

    let first = CancellationToken::new();
    first.control.begin().expect("begin first");
    let active = application
        .admit(
            &prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &first,
            Instant::now() + Duration::from_secs(1),
            SAFETY_FUEL,
        )
        .expect("first admission");
    let snapshot = application.snapshot();
    assert_eq!(snapshot.active_evaluations, 1);
    assert_eq!(snapshot.prepared_entries, 1);
    assert_eq!(snapshot.prepared_bytes, prepared.payload_len() as u64);

    let second = CancellationToken::new();
    second.control.begin().expect("begin second");
    assert!(matches!(
        application.admit(
            &prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &second,
            Instant::now() + Duration::from_secs(1),
            SAFETY_FUEL,
        ),
        Err(ApplicationError::Operational(OperationalFault::QueueFull))
    ));
    let snapshot = application.snapshot();
    assert_eq!(snapshot.accepted_evaluations, 1);
    assert_eq!(snapshot.prepared_entries, 1);
    drop(active);
    assert_eq!(application.snapshot().active_evaluations, 0);

    let third = CancellationToken::new();
    third.control.begin().expect("begin third");
    let active = application
        .admit(
            &prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &third,
            Instant::now() + Duration::from_secs(1),
            SAFETY_FUEL,
        )
        .expect("queue refusal did not consume aggregate quota");
    drop(active);
    assert_eq!(application.snapshot().accepted_evaluations, 2);

    let fourth = CancellationToken::new();
    fourth.control.begin().expect("begin fourth");
    assert!(matches!(
        application.admit(
            &prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &fourth,
            Instant::now() + Duration::from_secs(1),
            SAFETY_FUEL,
        ),
        Err(ApplicationError::Operational(
            OperationalFault::TotalEvaluationsExceeded
        ))
    ));
    assert_eq!(application.snapshot().accepted_evaluations, 2);
}

#[test]
fn application_queued_cancellation_removes_waiter_and_token_is_single_use() {
    let prepared = prepared(SOURCE);
    let mut quotas = application_quotas();
    quotas.concurrent_evaluations = 1;
    let application = ApplicationRuntime::new(quotas).expect("application");
    let first = CancellationToken::new();
    first.control.begin().expect("begin first");
    let active = application
        .admit(
            &prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &first,
            Instant::now() + Duration::from_secs(1),
            SAFETY_FUEL,
        )
        .expect("first admission");

    let queued_application = application.clone();
    let queued_prepared = prepared.clone();
    let queued_token = CancellationToken::new();
    let worker_token = queued_token.clone();
    let worker = thread::spawn(move || {
        worker_token.control.begin().expect("begin queued");
        queued_application.admit(
            &queued_prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &worker_token,
            Instant::now() + Duration::from_secs(1),
            SAFETY_FUEL,
        )
    });
    while application.snapshot().queued_evaluations == 0 {
        thread::yield_now();
    }
    assert!(queued_token.cancel());
    assert!(matches!(
        worker.join().expect("queued worker"),
        Err(ApplicationError::Operational(OperationalFault::Cancelled))
    ));
    assert_eq!(application.snapshot().queued_evaluations, 0);
    assert_eq!(
        queued_token.control.begin(),
        Err(OperationalFault::Cancelled)
    );
    drop(active);
    assert_eq!(application.snapshot().active_evaluations, 0);
}

#[test]
fn application_active_cancellation_is_selective_and_cleanup_allows_next_call() {
    let loop_rule = prepared(LOOP_SOURCE);
    let success_rule = prepared(SOURCE);
    let application = ApplicationRuntime::new(application_quotas()).expect("application");
    let loop_application = application.clone();
    let loop_token = CancellationToken::new();
    let worker_token = loop_token.clone();
    let worker = thread::spawn(move || {
        loop_application.evaluate(&loop_rule, INPUT, Limits::MAXIMUM.evaluate, &worker_token)
    });
    while application.snapshot().active_evaluations == 0 {
        thread::yield_now();
    }
    assert!(loop_token.cancel(), "loop settled before cancellation");

    let success = application
        .evaluate(
            &success_rule,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &CancellationToken::new(),
        )
        .expect("unrelated successful evaluation");
    assert_eq!(
        hex_lower(success.result.as_deref().expect("result")),
        RESULT_HEX
    );
    assert!(matches!(
        worker.join().expect("loop worker"),
        Err(ApplicationError::Operational(OperationalFault::Cancelled))
    ));
    let snapshot = application.snapshot();
    assert_eq!(snapshot.active_evaluations, 0);
    assert_eq!(snapshot.queued_evaluations, 0);

    let next = application
        .evaluate(
            &success_rule,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &CancellationToken::new(),
        )
        .expect("next evaluation");
    assert_eq!(next, success);
}

#[test]
fn application_wall_deadline_has_no_settled_result_and_releases_capacity() {
    let loop_rule = prepared(LOOP_SOURCE);
    let mut quotas = application_quotas();
    quotas.wall_millis = 1;
    let application = ApplicationRuntime::new(quotas).expect("application");
    assert!(matches!(
        application.evaluate(
            &loop_rule,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &CancellationToken::new(),
        ),
        Err(ApplicationError::Operational(
            OperationalFault::DeadlineExceeded
        ))
    ));
    let snapshot = application.snapshot();
    assert_eq!(snapshot.active_evaluations, 0);
    assert_eq!(snapshot.queued_evaluations, 0);
}

#[test]
fn application_engine_fault_releases_capacity_for_next_call() {
    let prepared = prepared(SOURCE);
    let application = ApplicationRuntime::new(application_quotas()).expect("application");
    assert_eq!(
        application.evaluate_with_safety_fuel(
            &prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &CancellationToken::new(),
            0,
        ),
        Err(ApplicationError::Runtime(RunError::EngineFault(
            "safety-fuel-exhausted"
        )))
    );
    assert_eq!(application.snapshot().active_evaluations, 0);
    let next = application
        .evaluate(
            &prepared,
            INPUT,
            Limits::MAXIMUM.evaluate,
            &CancellationToken::new(),
        )
        .expect("next evaluation");
    assert_eq!(
        hex_lower(next.result.as_deref().expect("result")),
        RESULT_HEX
    );
}

#[cfg(feature = "full-profile-contract")]
#[test]
fn full_profile_runtime_reuses_the_exact_prepared_rule_under_application_quotas() {
    const VECTOR_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/lispex-full-provider-intake/v1/inputs/",
        "products/full-embed-evaluator/v1.15.8/vectors/"
    );
    let prepared_bytes =
        std::fs::read(format!("{VECTOR_ROOT}prepared.lpxfull")).expect("full prepared vector");
    let prepared_artifact = decode_full_artifact(&prepared_bytes).expect("full prepared artifact");
    let request_sha256 = format!("sha256:{}", hex_lower(&prepared_artifact.request_sha256));
    let runtime = ReusableRuntime::full_profile().expect("full runtime");
    let prepared = runtime
        .load_full_prepared_consumer_artifact(&prepared_bytes, &request_sha256)
        .expect("full prepared rule");
    assert_eq!(prepared.component_sha256(), FULL_EVALUATOR_SHA256);
    assert_eq!(prepared.profile_id(), FULL_PROFILE_ID);

    let complete_bytes =
        std::fs::read(format!("{VECTOR_ROOT}complete.lpxfull")).expect("full complete vector");
    let complete = decode_full_artifact(&complete_bytes).expect("full complete artifact");
    let (input, limits) = full_evaluate_request_inputs(&complete.replay_request);
    let application =
        ApplicationRuntime::full_profile(application_quotas()).expect("full application");
    let result = application
        .evaluate(&prepared, &input, limits, &CancellationToken::new())
        .expect("full evaluation");
    assert_eq!(result.category, SettledCategory::Complete);
    assert_eq!(result.response_bytes, complete.response);
    assert_eq!(application.snapshot().prepared_entries, 1);
    assert_eq!(application.snapshot().active_evaluations, 0);

    let locally_prepared = prepare_full_consumer_artifact(SOURCE, Limits::MAXIMUM.prepare)
        .expect("full local preparation artifact");
    let local_request = format!(
        "sha256:{}",
        preparation_request_sha256(SOURCE, Limits::MAXIMUM.prepare)
            .expect("full preparation request")
    );
    let local_rule = runtime
        .load_full_prepared_consumer_artifact(&locally_prepared, &local_request)
        .expect("full local prepared rule");
    assert_eq!(local_rule.profile_id(), FULL_PROFILE_ID);
    assert_eq!(
        runtime
            .evaluate(&local_rule, INPUT, Limits::MAXIMUM.evaluate)
            .expect("full local evaluation")
            .category,
        SettledCategory::Complete
    );
}

#[cfg(feature = "full-profile-contract")]
#[test]
fn full_profile_application_closes_queue_cancellation_concurrency_and_cleanup() {
    const VECTOR_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/lispex-full-provider-intake/v1/inputs/",
        "products/full-embed-evaluator/v1.15.8/vectors/"
    );
    let prepared_bytes =
        std::fs::read(format!("{VECTOR_ROOT}prepared.lpxfull")).expect("full prepared vector");
    let prepared_artifact = decode_full_artifact(&prepared_bytes).expect("full prepared artifact");
    let request_sha256 = format!("sha256:{}", hex_lower(&prepared_artifact.request_sha256));
    let prepared = ReusableRuntime::full_profile()
        .expect("full runtime")
        .load_full_prepared_consumer_artifact(&prepared_bytes, &request_sha256)
        .expect("full prepared rule");
    let complete_bytes =
        std::fs::read(format!("{VECTOR_ROOT}complete.lpxfull")).expect("full complete vector");
    let complete = decode_full_artifact(&complete_bytes).expect("full complete artifact");
    let (input, limits) = full_evaluate_request_inputs(&complete.replay_request);

    let mut queue_quotas = application_quotas();
    queue_quotas.concurrent_evaluations = 1;
    queue_quotas.queued_evaluations = 1;
    let queued_application =
        ApplicationRuntime::full_profile(queue_quotas).expect("queued full application");
    let active_token = CancellationToken::new();
    active_token
        .control
        .begin()
        .expect("begin active full call");
    let active = queued_application
        .admit(
            &prepared,
            &input,
            limits,
            &active_token,
            Instant::now() + Duration::from_secs(5),
            SAFETY_FUEL,
        )
        .expect("active full admission");
    let waiter_application = queued_application.clone();
    let waiter_prepared = prepared.clone();
    let waiter_input = input.clone();
    let waiter_token = CancellationToken::new();
    let worker_token = waiter_token.clone();
    let waiter = thread::spawn(move || {
        worker_token
            .control
            .begin()
            .expect("begin queued full call");
        waiter_application.admit(
            &waiter_prepared,
            &waiter_input,
            limits,
            &worker_token,
            Instant::now() + Duration::from_secs(5),
            SAFETY_FUEL,
        )
    });
    while queued_application.snapshot().queued_evaluations == 0 {
        thread::yield_now();
    }
    assert!(waiter_token.cancel());
    assert!(matches!(
        waiter.join().expect("queued full worker"),
        Err(ApplicationError::Operational(OperationalFault::Cancelled))
    ));
    drop(active);
    let snapshot = queued_application.snapshot();
    assert_eq!(snapshot.active_evaluations, 0);
    assert_eq!(snapshot.queued_evaluations, 0);

    let concurrent_application =
        ApplicationRuntime::full_profile(application_quotas()).expect("concurrent full app");
    let workers = (0..2)
        .map(|_| {
            let application = concurrent_application.clone();
            let prepared = prepared.clone();
            let input = input.clone();
            thread::spawn(move || {
                application.evaluate(&prepared, &input, limits, &CancellationToken::new())
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| {
            worker
                .join()
                .expect("full concurrent worker")
                .expect("full result")
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|result| result.category == SettledCategory::Complete)
    );
    assert_eq!(results[0].response_bytes, results[1].response_bytes);
    let snapshot = concurrent_application.snapshot();
    assert_eq!(snapshot.active_evaluations, 0);
    assert_eq!(snapshot.queued_evaluations, 0);
    assert_eq!(snapshot.accepted_evaluations, 2);
}
