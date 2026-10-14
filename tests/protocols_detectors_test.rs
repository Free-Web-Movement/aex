//! Integration tests for the built-in protocol detectors and their
//! `Match` / `Pass` / `NeedMore` branches.
//!
//! Covers `Http11Detector`, `Http2Detector` and (under the `proxy` feature) the
//! built-in `SocksDetector`.

use aex::detect::{DetectionState, ProtocolDetector, Verdict};
use aex::protocols::classify::{H2_CONNECTION_PREFACE, HTTP_METHODS};
use aex::protocols::http1::Http11Detector;
use aex::protocols::http2::Http2Detector;

// ---------------------------------------------------------------------------
// Http11Detector
// ---------------------------------------------------------------------------

#[test]
fn http11_detector_metadata() {
    let d = Http11Detector;
    assert_eq!(d.name(), "http11");
    assert_eq!(d.protocol(), "http");
    let longest = HTTP_METHODS.iter().map(|m| m.len()).max().unwrap();
    assert_eq!(d.max_need(), Some(longest));
}

#[test]
fn http11_detector_matches_every_known_method() {
    let mut state = DetectionState::new();
    for method in HTTP_METHODS {
        assert_eq!(
            Http11Detector.detect(method, &mut state),
            Verdict::Match,
            "method {:?} should match",
            String::from_utf8_lossy(method)
        );
    }
}

#[test]
fn http11_detector_need_more_on_partial_method() {
    let mut state = DetectionState::new();
    // "GE" is a proper prefix of "GET " -> needs the remaining 2 bytes.
    assert_eq!(
        Http11Detector.detect(b"GE", &mut state),
        Verdict::NeedMore(b"GET ".len() - 2)
    );
    // "DELE" is a proper prefix of "DELETE ".
    assert_eq!(
        Http11Detector.detect(b"DELE", &mut state),
        Verdict::NeedMore(b"DELETE ".len() - 4)
    );
    // Empty buffer still leaves every method possible: shortest pending wins.
    let shortest = HTTP_METHODS.iter().map(|m| m.len()).min().unwrap();
    assert_eq!(
        Http11Detector.detect(b"", &mut state),
        Verdict::NeedMore(shortest)
    );
}

#[test]
fn http11_detector_passes_non_method_bytes() {
    let mut state = DetectionState::new();
    assert_eq!(
        Http11Detector.detect(b"\x00\x01not-http", &mut state),
        Verdict::Pass
    );
    assert_eq!(Http11Detector.detect(b"X", &mut state), Verdict::Pass);
    // A method-looking prefix that diverges is also passed on.
    assert_eq!(Http11Detector.detect(b"GETX", &mut state), Verdict::Pass);
}

// ---------------------------------------------------------------------------
// Http2Detector
// ---------------------------------------------------------------------------

#[test]
fn http2_detector_metadata() {
    let d = Http2Detector;
    assert_eq!(d.name(), "http2");
    assert_eq!(d.protocol(), "http2");
    assert_eq!(d.max_need(), Some(H2_CONNECTION_PREFACE.len()));
}

#[test]
fn http2_detector_matches_full_preface() {
    let mut state = DetectionState::new();
    assert_eq!(
        Http2Detector.detect(H2_CONNECTION_PREFACE, &mut state),
        Verdict::Match
    );
}

#[test]
fn http2_detector_need_more_on_shared_prefix() {
    let mut state = DetectionState::new();
    let partial = b"PRI * ";
    assert_eq!(
        Http2Detector.detect(partial, &mut state),
        Verdict::NeedMore(H2_CONNECTION_PREFACE.len() - partial.len())
    );
}

#[test]
fn http2_detector_passes_unrelated_and_empty() {
    let mut state = DetectionState::new();
    assert_eq!(
        Http2Detector.detect(b"GET / HTTP/1.1", &mut state),
        Verdict::Pass
    );
    let mut state = DetectionState::new();
    assert_eq!(Http2Detector.detect(b"", &mut state), Verdict::Pass);
}

// ---------------------------------------------------------------------------
// SocksDetector (built-in, proxy feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "proxy")]
mod socks_detector {
    use super::*;
    use aex::proxy::SocksDetector;
    use aex::proxy::socks::SocksVersion;

    #[test]
    fn metadata() {
        let d = SocksDetector;
        assert_eq!(d.name(), "socks");
        assert_eq!(d.protocol(), "socks");
        assert_eq!(d.max_need(), Some(257));
    }

    #[test]
    fn empty_buffer_passes() {
        let mut state = DetectionState::new();
        assert_eq!(SocksDetector.detect(b"", &mut state), Verdict::Pass);
    }

    #[test]
    fn v5_greeting_branches() {
        // Only the version byte -> need one more byte (NMETHODS).
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x05], &mut state),
            Verdict::NeedMore(1)
        );

        // Zero methods is invalid -> pass.
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x05, 0x00], &mut state),
            Verdict::Pass
        );

        // Declared methods not fully buffered yet.
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x05, 0x02, 0x00], &mut state),
            Verdict::NeedMore(1)
        );

        // Complete method list -> match, version recorded.
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x05, 0x02, 0x00, 0x02], &mut state),
            Verdict::Match
        );
        assert_eq!(state.get_scratch::<SocksVersion>(), Some(SocksVersion::V5));
    }

    #[test]
    fn v4_request_branches() {
        // Version only -> need one more byte.
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x04], &mut state),
            Verdict::NeedMore(1)
        );

        // Unsupported command (not 1|2) -> pass.
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x04, 0x03], &mut state),
            Verdict::Pass
        );

        // Valid cmd but header not fully buffered (needs 8 bytes).
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x04, 0x01, 0, 0, 0, 0, 0], &mut state),
            Verdict::NeedMore(1)
        );

        // Full 8-byte connect request -> match, version recorded.
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x04, 0x01, 0, 0, 0, 0, 0, 0], &mut state),
            Verdict::Match
        );
        assert_eq!(state.get_scratch::<SocksVersion>(), Some(SocksVersion::V4));
    }

    #[test]
    fn unknown_first_byte_passes() {
        let mut state = DetectionState::new();
        assert_eq!(
            SocksDetector.detect(&[0x99, 0x00], &mut state),
            Verdict::Pass
        );
    }
}
