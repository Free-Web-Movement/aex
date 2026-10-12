//! Pluggable protocol detection for the unified server.
//!
//! The registry ships empty: every detector — HTTP/1.1, HTTP/2, WebSocket,
//! trojan, NAT forwarding, ... — is added manually by the server composer.
//! Detectors are middleware-like components held in an ordered
//! [`DetectorRegistry`] and may be registered, removed, reordered, or
//! replaced at runtime. Each connection takes a snapshot of the registry
//! before detection starts, so mutations never tear an in-flight detection.
//!
//! On every connection the snapshot is evaluated in order: each detector
//! inspects the bytes buffered so far and either claims the connection
//! ([`Verdict::Match`]), declines it ([`Verdict::Pass`]), or asks for more
//! bytes ([`Verdict::NeedMore`]). The first claim wins and short-circuits
//! the rest, so two conflicting detectors can never both handle the same
//! connection; explicit [`ProtocolDetector::conflicts_with`] declarations
//! additionally reject incompatible combinations at registration time.
//!
//! A match carries a [`DetectorMode`]. Standard claims dispatch to the
//! handler mapped to the claimed protocol. Forward-mode detectors (NAT-style
//! stateful forwarders) terminate detection entirely: the connection is
//! handed straight to their handler for direct forwarding with no further
//! detection steps.
//!
//! Per-connection progress is kept in a [`DetectionState`] ("link state"):
//! how many bytes were consumed, which detector claimed the connection and
//! in which mode, a verdict trace of every evaluated detector, and
//! per-detector scratch space. After dispatch the state is stored as a
//! context attribute so downstream handlers and middlewares can query what
//! was detected.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Hard cap on bytes buffered during detection. Detectors must decide before
/// this; past it the connection falls through to the default TCP handler.
pub const MAX_PEEK: usize = 16 * 1024;

/// How the pipeline treats a match from a detector.
///
/// The registry ships empty — every detector (HTTP/1.1, HTTP/2, WebSocket,
/// trojan, ...) is added manually by the server composer, in the order that
/// suits the deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorMode {
    /// Standard protocol detection: on match, dispatch to the handler mapped
    /// to this detector's protocol label.
    Standard,
    /// Stateful forwarding (e.g. NAT transparent proxy): on match, stop
    /// detection entirely and hand the connection straight to the forwarder.
    /// No further detection steps run for this connection.
    Forward,
}

/// What a detector decides after examining the currently buffered bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Claim the connection for this detector's protocol.
    Match,
    /// This protocol does not match; evaluate the next detector.
    Pass,
    /// Undecidable with the current bytes; need at least `n` more bytes
    /// (relative to what has been buffered so far).
    NeedMore(usize),
}

impl Verdict {
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::Match => "match",
            Verdict::Pass => "pass",
            Verdict::NeedMore(_) => "need-more",
        }
    }
}

/// A recorded verdict from one detector, in evaluation order.
#[derive(Debug, Clone)]
pub struct DetectionEvent {
    pub detector: String,
    pub verdict_name: &'static str,
    pub needed_more: Option<usize>,
}

/// The winning claim recorded by the first matching detector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub detector: String,
    pub protocol: String,
    pub mode: DetectorMode,
}

/// Per-connection detection state ("link state").
///
/// Tracks everything that happened during the detection phase of one
/// connection: buffered byte count, per-detector scratch space, the ordered
/// verdict trace, and the final claim. It is stored into the context's local
/// type map after dispatch, so the whole downstream chain shares it.
#[derive(Default)]
pub struct DetectionState {
    /// Total bytes buffered when detection last ran.
    pub buffered: usize,
    claimed: Option<Claim>,
    finished: bool,
    history: Vec<DetectionEvent>,
    scratch: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl DetectionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The winning claim, if any detector has matched.
    pub fn claim(&self) -> Option<&Claim> {
        self.claimed.as_ref()
    }

    /// Whether the detection phase is over (claimed or fully declined).
    pub fn is_finished(&self) -> bool {
        self.finished || self.claimed.is_some()
    }

    /// Whether at least one pending detector still needs more bytes.
    pub fn needs_more(&self) -> bool {
        !self.is_finished() && self.history.iter().any(|e| e.needed_more.is_some())
    }

    /// Ordered trace of every verdict produced so far.
    pub fn history(&self) -> &[DetectionEvent] {
        &self.history
    }

    pub(crate) fn record(&mut self, event: DetectionEvent) {
        self.history.push(event);
    }

    pub(crate) fn set_claim(&mut self, detector: &str, protocol: &str, mode: DetectorMode) {
        self.claimed = Some(Claim {
            detector: detector.to_string(),
            protocol: protocol.to_string(),
            mode,
        });
    }

    pub(crate) fn finish(&mut self) {
        self.finished = true;
    }

    /// Store detector-private scratch data keyed by type.
    pub fn set_scratch<T: Send + Sync + 'static>(&mut self, val: T) {
        self.scratch.insert(TypeId::of::<T>(), Box::new(val));
    }

    /// Fetch a clone of previously stored scratch data.
    pub fn get_scratch<T: Clone + 'static>(&self) -> Option<T> {
        self.scratch
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>().cloned())
    }

    /// Mutably access previously stored scratch data.
    pub fn get_scratch_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.scratch
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut::<T>())
    }
}

/// A pluggable protocol detector.
///
/// Implementations inspect only the bytes buffered so far and never block:
/// if more data is required to be sure, return [`Verdict::NeedMore`] with the
/// number of additional bytes needed.
pub trait ProtocolDetector: Send + Sync {
    /// Unique identifier of this detector instance (used for ordering,
    /// replacement, removal, and conflict declarations).
    fn name(&self) -> &str;

    /// Protocol label written into the [`Claim`] on a match.
    fn protocol(&self) -> &str;

    /// Names of detectors this one cannot coexist with. Registration fails
    /// if any name here is already present (or later registers), preventing
    /// ambiguous pipelines by construction.
    fn conflicts_with(&self) -> Vec<String> {
        Vec::new()
    }

    /// Upper bound on bytes this detector needs to reach a decision, or
    /// `None` if unbounded. The pipeline uses it to stop reading early once
    /// every pending detector has been satisfied.
    fn max_need(&self) -> Option<usize> {
        None
    }

    /// How a match from this detector is treated by the pipeline. Defaults
    /// to [`DetectorMode::Standard`]; stateful forwarders (NAT-style) return
    /// [`DetectorMode::Forward`] so that a match terminates detection and the
    /// connection is forwarded directly.
    fn mode(&self) -> DetectorMode {
        DetectorMode::Standard
    }

    /// Evaluate the buffered bytes. Called at most once per read chunk, in
    /// registry order, until this detector passes, matches, or detection is
    /// otherwise finished.
    fn detect(&self, buf: &[u8], state: &mut DetectionState) -> Verdict;
}

/// Where to insert a detector within the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Position {
    Front,
    Back,
    Before(String),
    After(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error("detector `{0}` is already registered")]
    Duplicate(String),
    #[error("detector `{new}` conflicts with `{existing}`")]
    Conflict { new: String, existing: String },
    #[error("position anchor `{0}` not found")]
    AnchorNotFound(String),
}

type Entries = RwLock<Vec<Arc<dyn ProtocolDetector>>>;

/// Ordered, runtime-mutable collection of protocol detectors.
pub struct DetectorRegistry {
    entries: Entries,
}

impl Default for DetectorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DetectorRegistry {
    /// Empty registry (no built-in detectors).
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
        }
    }

    fn check_insertable(
        entries: &[Arc<dyn ProtocolDetector>],
        d: &dyn ProtocolDetector,
    ) -> Result<(), RegisterError> {
        if entries.iter().any(|e| e.name() == d.name()) {
            return Err(RegisterError::Duplicate(d.name().to_string()));
        }
        for declared in d.conflicts_with() {
            if let Some(existing) = entries.iter().find(|e| e.name() == declared) {
                return Err(RegisterError::Conflict {
                    new: d.name().to_string(),
                    existing: existing.name().to_string(),
                });
            }
        }
        // Reverse direction: an already-registered detector may declare this
        // newcomer as incompatible.
        for existing in entries {
            if existing.conflicts_with().iter().any(|c| c == d.name()) {
                return Err(RegisterError::Conflict {
                    new: d.name().to_string(),
                    existing: existing.name().to_string(),
                });
            }
        }
        Ok(())
    }

    fn insert_at(
        entries: &mut Vec<Arc<dyn ProtocolDetector>>,
        pos: &Position,
        d: Arc<dyn ProtocolDetector>,
    ) -> Result<(), RegisterError> {
        let idx = match pos {
            Position::Front => 0,
            Position::Back => entries.len(),
            Position::Before(name) => entries
                .iter()
                .position(|e| e.name() == name)
                .ok_or_else(|| RegisterError::AnchorNotFound(name.clone()))?,
            Position::After(name) => entries
                .iter()
                .position(|e| e.name() == name)
                .map(|i| i + 1)
                .ok_or_else(|| RegisterError::AnchorNotFound(name.clone()))?,
        };
        entries.insert(idx, d);
        Ok(())
    }

    /// Append a detector at the back after conflict checks.
    pub fn register(&self, d: Arc<dyn ProtocolDetector>) -> Result<(), RegisterError> {
        self.register_at(Position::Back, d)
    }

    /// Insert a detector at an explicit position after conflict checks.
    pub fn register_at(
        &self,
        pos: Position,
        d: Arc<dyn ProtocolDetector>,
    ) -> Result<(), RegisterError> {
        let mut entries = self.entries.write().expect("detector registry poisoned");
        Self::check_insertable(&entries, d.as_ref())?;
        Self::insert_at(&mut entries, &pos, d)?;
        Ok(())
    }

    /// Replace an existing detector in place (same position). Fails with
    /// [`RegisterError::AnchorNotFound`] if no entry carries that name; the
    /// replacement itself must pass conflict checks against the others.
    pub fn replace(&self, d: Arc<dyn ProtocolDetector>) -> Result<(), RegisterError> {
        let mut entries = self.entries.write().expect("detector registry poisoned");
        let idx = entries
            .iter()
            .position(|e| e.name() == d.name())
            .ok_or_else(|| RegisterError::AnchorNotFound(d.name().to_string()))?;
        let others: Vec<Arc<dyn ProtocolDetector>> = entries
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, e)| e.clone())
            .collect();
        Self::check_insertable(&others, d.as_ref())?;
        entries[idx] = d;
        Ok(())
    }

    /// Remove a detector by name. Returns whether it was present.
    pub fn unregister(&self, name: &str) -> bool {
        let mut entries = self.entries.write().expect("detector registry poisoned");
        let len_before = entries.len();
        entries.retain(|e| e.name() != name);
        entries.len() != len_before
    }

    /// Ordered list of detector names.
    pub fn list(&self) -> Vec<String> {
        self.entries
            .read()
            .expect("detector registry poisoned")
            .iter()
            .map(|e| e.name().to_string())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries
            .read()
            .expect("detector registry poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Consistent view of the current pipeline, used for exactly one
    /// connection's detection phase.
    pub fn snapshot(&self) -> Vec<Arc<dyn ProtocolDetector>> {
        self.entries
            .read()
            .expect("detector registry poisoned")
            .clone()
    }
}

/// Run the detection pipeline over `buf`, mutating `state`.
///
/// Detectors are evaluated strictly in order; the first [`Verdict::Match`]
/// sets the claim and short-circuits the rest. Returns once the state is
/// finished (matched or fully declined); callers should keep feeding bytes
/// while `!state.is_finished()` and more data may arrive.
pub fn run_pipeline(
    detectors: &[Arc<dyn ProtocolDetector>],
    buf: &[u8],
    state: &mut DetectionState,
) {
    if state.is_finished() {
        return;
    }
    state.buffered = buf.len();

    let mut needed: Option<usize> = None;
    for d in detectors {
        let v = d.detect(buf, state);
        let need = match &v {
            Verdict::NeedMore(n) => Some(*n),
            _ => None,
        };
        state.record(DetectionEvent {
            detector: d.name().to_string(),
            verdict_name: v.name(),
            needed_more: need,
        });
        match v {
            Verdict::Match => {
                let mode = d.mode();
                state.set_claim(d.name(), d.protocol(), mode);
                return;
            }
            Verdict::NeedMore(n) => {
                needed = Some(match needed {
                    Some(prev) => prev.max(n),
                    None => n,
                });
            }
            Verdict::Pass => {}
        }
    }

    if needed.is_none() {
        // Every pending detector passed: nothing will ever claim this
        // connection regardless of future bytes.
        state.finish();
    } else if state.buffered.saturating_add(needed.unwrap_or(0)) > MAX_PEEK {
        // The outstanding need can never be satisfied within the peek cap:
        // give up and fall through to the default handler.
        state.finish();
    }
}
