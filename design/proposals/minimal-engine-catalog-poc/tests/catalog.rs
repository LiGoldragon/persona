use persona_minimal_engine_catalog_poc::{
    DesiredCatalog, DesiredEngine, EngineId, Generation, GenerationReporting, InMemoryCatalog,
    ObservationOutcome, ObservedAt, ObservedCatalog, ObservedEngine, PaceAmount, PaceInterval,
    QuotaCatalog, QuotaPace, QuotaReporting, QuotaSnapshot, RemainingAmount, ResetInterpretation,
    ResetInterval, SourceTimestamp,
};

struct FakeCatalogState {
    catalog: InMemoryCatalog,
    alpha: EngineId,
    beta: EngineId,
}

#[test]
fn newer_mismatch_is_reported_while_stale_observation_cannot_replace_it() {
    let mut state = FakeCatalogState {
        catalog: InMemoryCatalog::default(),
        alpha: EngineId("fixture-alpha".into()),
        beta: EngineId("fixture-beta".into()),
    };
    state.catalog.set_desired(DesiredEngine {
        id: state.alpha.clone(),
        generation: Generation(2),
    });
    assert_eq!(
        state.catalog.record_observation(ObservedEngine {
            id: state.alpha.clone(),
            generation: Generation(1),
            observed_at: ObservedAt(20),
        }),
        ObservationOutcome::GenerationMismatch
    );
    assert_eq!(
        state.catalog.record_observation(ObservedEngine {
            id: state.alpha.clone(),
            generation: Generation(2),
            observed_at: ObservedAt(10),
        }),
        ObservationOutcome::StaleObservation
    );
    let report = state
        .catalog
        .generation_report(&state.alpha)
        .expect("desired fixture exists");
    assert_eq!(report.desired, Generation(2));
    assert_eq!(
        report
            .observed
            .expect("newer mismatch was retained")
            .generation,
        Generation(1)
    );
}

#[test]
fn desired_and_observed_state_remain_independent_per_fixture_engine() {
    let mut state = FakeCatalogState {
        catalog: InMemoryCatalog::default(),
        alpha: EngineId("fixture-alpha".into()),
        beta: EngineId("fixture-beta".into()),
    };
    state.catalog.set_desired(DesiredEngine {
        id: state.alpha.clone(),
        generation: Generation(4),
    });
    state.catalog.set_desired(DesiredEngine {
        id: state.beta.clone(),
        generation: Generation(9),
    });
    assert_eq!(
        state.catalog.record_observation(ObservedEngine {
            id: state.beta.clone(),
            generation: Generation(9),
            observed_at: ObservedAt(30),
        }),
        ObservationOutcome::Current
    );
    assert_eq!(
        state
            .catalog
            .generation_report(&state.alpha)
            .expect("alpha desired")
            .observed,
        None
    );
    assert_eq!(
        state
            .catalog
            .generation_report(&state.beta)
            .expect("beta desired")
            .observed
            .expect("beta observed")
            .generation,
        Generation(9)
    );
}

#[test]
fn quota_report_preserves_supplied_reset_and_pace_without_daily_target() {
    let mut state = FakeCatalogState {
        catalog: InMemoryCatalog::default(),
        alpha: EngineId("fixture-alpha".into()),
        beta: EngineId("fixture-beta".into()),
    };
    let supplied = QuotaSnapshot {
        source_timestamp: SourceTimestamp(1_725_000_000),
        reset_interval: ResetInterval(18_000),
        remaining: RemainingAmount(73),
        pace: QuotaPace {
            amount: PaceAmount(7),
            interval: PaceInterval(3_600),
        },
        reset_interpretation: ResetInterpretation::RollingWindow,
    };
    state
        .catalog
        .record_quota(state.beta.clone(), supplied.clone());
    let report = state
        .catalog
        .quota_report(&state.beta)
        .expect("beta quota exists");
    assert_eq!(report.engine, state.beta);
    assert_eq!(report.snapshot, supplied);
    assert_eq!(state.catalog.quota_report(&state.alpha), None);
}
