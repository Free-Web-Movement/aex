// Tests for the unified detection pipeline (`aex::detect`): verdict
// naming, per-connection DetectionState, the DetectorRegistry (register/replace/
// unregister/ordering/conflicts) and run_pipeline combinatorics. The pipeline's
// regression tests live here via the public API — no code under src/.

use std::sync::Arc;

use aex::detect::{
    DetectionState, DetectorMode, DetectorRegistry, MAX_PEEK, Position, ProtocolDetector,
    RegisterError, Verdict, run_pipeline,
};

struct Passer;
impl ProtocolDetector for Passer {
    fn name(&self) -> &str {
        "passer"
    }
    fn protocol(&self) -> &str {
        "pass-proto"
    }
    fn detect(&self, _buf: &[u8], _state: &mut DetectionState) -> Verdict {
        Verdict::Pass
    }
}

struct ClaimC {
    prefix: &'static [u8],
    tag: &'static str,
}
impl ProtocolDetector for ClaimC {
    fn name(&self) -> &str {
        self.tag
    }
    fn protocol(&self) -> &str {
        "claim-proto"
    }
    fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
        if buf.starts_with(self.prefix) {
            Verdict::Match
        } else {
            Verdict::NeedMore(1)
        }
    }
}

struct NeedPending {
    n: usize,
    tag: &'static str,
}
impl ProtocolDetector for NeedPending {
    fn name(&self) -> &str {
        self.tag
    }
    fn protocol(&self) -> &str {
        "need-proto"
    }
    fn detect(&self, _buf: &[u8], _state: &mut DetectionState) -> Verdict {
        Verdict::NeedMore(self.n)
    }
}

struct Stub {
    name: &'static str,
    conflict: &'static [&'static str],
}
impl ProtocolDetector for Stub {
    fn name(&self) -> &str {
        self.name
    }
    fn protocol(&self) -> &str {
        "stub-proto"
    }
    fn conflicts_with(&self) -> Vec<String> {
        self.conflict.iter().map(|s| s.to_string()).collect()
    }
    fn detect(&self, _buf: &[u8], _state: &mut DetectionState) -> Verdict {
        Verdict::Pass
    }
}

fn stub(name: &'static str) -> Arc<dyn ProtocolDetector> {
    Arc::new(Stub {
        name,
        conflict: &[],
    })
}

#[test]
fn verdict_name_classifies_kinds() {
    assert_eq!(Verdict::Match.name(), "match");
    assert_eq!(Verdict::Pass.name(), "pass");
    assert_eq!(Verdict::NeedMore(3).name(), "need-more");
}

#[test]
fn detection_state_default_is_empty() {
    let st: DetectionState = DetectionState::default();
    assert_eq!(st.buffered, 0);
    assert!(st.claim().is_none());
    assert!(!st.is_finished());
    assert!(!st.needs_more());
    assert!(st.history().is_empty());
    assert_eq!(DetectionState::new().buffered, 0);
}

#[test]
fn detection_state_scratch_roundtrip() {
    let mut st = DetectionState::new();
    assert_eq!(st.get_scratch::<u32>(), None);
    st.set_scratch(9u32);
    assert_eq!(st.get_scratch::<u32>(), Some(9));
    if let Some(v) = st.get_scratch_mut::<u32>() {
        *v += 1;
    }
    assert_eq!(st.get_scratch::<u32>(), Some(10));
    assert!(st.get_scratch::<String>().is_none());
}

#[test]
fn protocol_detector_defaults() {
    let d = stub("defaults");
    assert!(d.conflicts_with().is_empty());
    assert_eq!(d.max_need(), None);
    assert_eq!(d.mode(), DetectorMode::Standard);
}

#[test]
fn run_pipeline_first_match_claims_and_short_circuits() {
    let dets: Vec<Arc<dyn ProtocolDetector>> = vec![
        Arc::new(Passer),
        Arc::new(ClaimC {
            prefix: b"CLM ",
            tag: "claimc",
        }),
    ];
    let mut st = DetectionState::new();
    run_pipeline(&dets, b"CLM x", &mut st);

    assert_eq!(st.buffered, 5);
    assert!(st.is_finished());
    let claim = st.claim().unwrap();
    assert_eq!(claim.detector, "claimc");
    assert_eq!(claim.protocol, "claim-proto");
    assert_eq!(claim.mode, DetectorMode::Standard);
    // Passer ran first, ClaimC second and short-circuited.
    assert_eq!(st.history().len(), 2);
    assert_eq!(st.history()[0].verdict_name, "pass");
    assert!(st.history()[0].needed_more.is_none());
    assert_eq!(st.history()[1].verdict_name, "match");
}

#[test]
fn run_pipeline_need_more_records_and_merges() {
    let dets: Vec<Arc<dyn ProtocolDetector>> = vec![
        Arc::new(NeedPending { n: 2, tag: "need2" }),
        Arc::new(NeedPending { n: 7, tag: "need7" }),
    ];
    let mut st = DetectionState::new();
    run_pipeline(&dets, b"ab", &mut st);

    assert_eq!(st.buffered, 2);
    assert!(!st.is_finished());
    assert!(st.needs_more());
    assert!(st.claim().is_none());
    assert_eq!(st.history().len(), 2);
    assert_eq!(st.history()[0].needed_more, Some(2));
    assert_eq!(st.history()[1].needed_more, Some(7));
}

#[test]
fn run_pipeline_all_pass_finishes_without_claim() {
    let dets: Vec<Arc<dyn ProtocolDetector>> = vec![Arc::new(Passer)];
    let mut st = DetectionState::new();
    run_pipeline(&dets, b"xx", &mut st);

    assert!(st.is_finished());
    assert!(st.claim().is_none());
    assert_eq!(st.history().len(), 1);
}

#[test]
fn run_pipeline_on_finished_state_is_a_noop() {
    let dets: Vec<Arc<dyn ProtocolDetector>> = vec![Arc::new(ClaimC {
        prefix: b"CLM ",
        tag: "claimc",
    })];
    let mut st = DetectionState::new();
    run_pipeline(&dets, b"CLM one", &mut st);
    assert!(st.is_finished());
    assert_eq!(st.buffered, 7);

    run_pipeline(&dets, b"CLM two", &mut st);
    assert_eq!(st.buffered, 7, "finished state must not be re-evaluated");
    assert_eq!(st.history().len(), 1);
}

#[test]
fn run_pipeline_over_peek_cap_finishes_without_claim() {
    let dets: Vec<Arc<dyn ProtocolDetector>> = vec![Arc::new(NeedPending {
        n: MAX_PEEK + 10,
        tag: "greedy",
    })];
    let mut st = DetectionState::new();
    run_pipeline(&dets, b"", &mut st);

    assert!(st.is_finished());
    assert!(st.claim().is_none());
    assert!(!st.needs_more());
}

#[test]
fn detector_registry_default_and_register_list() {
    let r = DetectorRegistry::default();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);

    r.register(stub("d1")).unwrap();
    r.register(stub("d2")).unwrap();
    assert_eq!(r.list(), vec!["d1".to_string(), "d2".to_string()]);
    assert_eq!(r.len(), 2);
    assert!(!r.is_empty());
}

#[test]
fn detector_registry_duplicate_rejected() {
    let r = DetectorRegistry::new();
    r.register(stub("d1")).unwrap();
    let err = r.register(stub("d1")).unwrap_err();
    match err {
        RegisterError::Duplicate(name) => assert_eq!(name, "d1"),
        other => panic!("expected duplicate, got {other:?}"),
    }
}

#[test]
fn detector_registry_conflict_rejected_forward_and_reverse() {
    let fwd = DetectorRegistry::new();
    fwd.register(stub("b")).unwrap();
    let a_conflicts_b = Arc::new(Stub {
        name: "a",
        conflict: &["b"],
    });
    match fwd.register(a_conflicts_b).unwrap_err() {
        RegisterError::Conflict { new, existing } => {
            assert_eq!(new, "a");
            assert_eq!(existing, "b");
        }
        other => panic!("expected conflict, got {other:?}"),
    }

    let rev = DetectorRegistry::new();
    rev.register(Arc::new(Stub {
        name: "a",
        conflict: &["b"],
    }))
    .unwrap();
    match rev.register(stub("b")).unwrap_err() {
        RegisterError::Conflict { new, existing } => {
            assert_eq!(new, "b");
            assert_eq!(existing, "a");
        }
        other => panic!("expected conflict, got {other:?}"),
    }
}

#[test]
fn detector_registry_position_insertion_and_missing_anchor() {
    let r = DetectorRegistry::new();
    r.register(stub("d1")).unwrap();
    r.register(stub("d2")).unwrap();

    r.register_at(Position::Front, stub("f")).unwrap();
    assert_eq!(r.list(), vec!["f", "d1", "d2"]);

    r.register_at(Position::After("d1".to_string()), stub("da"))
        .unwrap();
    assert_eq!(r.list(), vec!["f", "d1", "da", "d2"]);

    r.register_at(Position::Before("d2".to_string()), stub("db"))
        .unwrap();
    assert_eq!(r.list(), vec!["f", "d1", "da", "db", "d2"]);

    match r
        .register_at(Position::Before("missing".to_string()), stub("x"))
        .unwrap_err()
    {
        RegisterError::AnchorNotFound(name) => assert_eq!(name, "missing"),
        other => panic!("expected anchor not found, got {other:?}"),
    }
    match r
        .register_at(Position::After("missing".to_string()), stub("y"))
        .unwrap_err()
    {
        RegisterError::AnchorNotFound(name) => assert_eq!(name, "missing"),
        other => panic!("expected anchor not found, got {other:?}"),
    }
    assert_eq!(r.list(), vec!["f", "d1", "da", "db", "d2"]);
}

#[test]
fn detector_registry_replace_in_place() {
    let r = DetectorRegistry::new();
    r.register(stub("d1")).unwrap();
    r.register(stub("d2")).unwrap();

    r.replace(stub("d1")).unwrap();
    assert_eq!(r.list(), vec!["d1", "d2"]);

    match r.replace(stub("nonexistent")).unwrap_err() {
        RegisterError::AnchorNotFound(name) => assert_eq!(name, "nonexistent"),
        other => panic!("expected anchor not found, got {other:?}"),
    }

    let conflicting = Arc::new(Stub {
        name: "d2",
        conflict: &["d1"],
    });
    match r.replace(conflicting).unwrap_err() {
        RegisterError::Conflict { new, existing } => {
            assert_eq!(new, "d2");
            assert_eq!(existing, "d1");
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    assert_eq!(r.list(), vec!["d1", "d2"]);
}

#[test]
fn detector_registry_unregister_reports_presence() {
    let r = DetectorRegistry::new();
    r.register(stub("d1")).unwrap();
    r.register(stub("d2")).unwrap();

    assert!(r.unregister("d1"));
    assert_eq!(r.list(), vec!["d2"]);
    assert!(!r.unregister("d1"));
}
