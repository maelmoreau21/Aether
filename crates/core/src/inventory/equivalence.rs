//! When may one copy of a title stand in for another?
//!
//! Several Nodes can hold a copy of the same film or episode, and a work can be consolidated
//! from several libraries. Being the same *work* is not enough to replace a file by another one
//! in the middle of a playback session: a director's cut, a different encode with another
//! runtime or a mis-grouped episode would silently change what the viewer watches, and the
//! saved position would point at a different scene.
//!
//! A candidate is therefore only offered as an equivalent source when measurable facts agree:
//!
//! * [`SourceEquivalence::Identical`]: same byte size and same sampled content fingerprint
//!   (`inventory_asset_fingerprints`). The fingerprint is a hint, not an identity, but a match
//!   on both size and a sampled digest is strong evidence of the same file.
//! * [`SourceEquivalence::SameRuntime`]: both probes report a duration and the two agree within
//!   a tight tolerance, so a playback position keeps its meaning. This covers a re-encode or a
//!   different container of the same cut; it does not claim identical bytes.
//!
//! Everything else is not equivalent and is never substituted automatically. Equivalence says
//! nothing about permission: the caller still filters candidates through the viewer's
//! libraries, parental policy and the Node-side live authorization.

/// Absolute floor of the runtime tolerance, in seconds (container rounding, trailing padding).
const RUNTIME_TOLERANCE_FLOOR_SECS: f64 = 1.0;
/// Relative runtime tolerance. 0.2 % is 7 s on a one-hour episode, below any edit of a cut.
const RUNTIME_TOLERANCE_RATIO: f64 = 0.002;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SourceEquivalence {
    /// Same runtime only: ranked after an identical file when scores tie.
    SameRuntime,
    /// Same size and same sampled fingerprint.
    Identical,
}

impl SourceEquivalence {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Identical => "identical",
            Self::SameRuntime => "same-runtime",
        }
    }
}

/// What is known about one copy, independently of where it is hosted.
#[derive(Clone, Debug, Default)]
pub(super) struct SourceFacts {
    pub size: i64,
    pub fingerprint: Option<String>,
    pub duration_secs: Option<f64>,
}

impl SourceFacts {
    pub(super) fn from_probe_duration(duration: Option<&str>) -> Option<f64> {
        duration
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
    }

    /// Container duration straight from a stored probe document.
    ///
    /// Deserialises only `format.duration`: serde skips every other field without building it,
    /// which matters because the playback descriptor evaluates every copy of a title (99 of them
    /// in the 100-Node benchmark) and a full `Probe` decode doubled its latency.
    pub(super) fn duration_from_probe_json(probe_json: &str) -> Option<f64> {
        #[derive(serde::Deserialize)]
        struct Document {
            format: Option<Format>,
        }
        #[derive(serde::Deserialize)]
        struct Format {
            duration: Option<serde_json::Value>,
        }
        let document: Document = serde_json::from_str(probe_json).ok()?;
        match document.format?.duration? {
            serde_json::Value::String(text) => Self::from_probe_duration(Some(&text)),
            serde_json::Value::Number(number) => number
                .as_f64()
                .filter(|value| value.is_finite() && *value > 0.0),
            _ => None,
        }
    }
}

/// Classify `candidate` against the requested copy, or `None` when it must not replace it.
pub(super) fn classify(
    requested: &SourceFacts,
    candidate: &SourceFacts,
) -> Option<SourceEquivalence> {
    if let (Some(a), Some(b)) = (&requested.fingerprint, &candidate.fingerprint)
        && a == b
        && requested.size == candidate.size
        && requested.size > 0
    {
        return Some(SourceEquivalence::Identical);
    }
    let (a, b) = (requested.duration_secs?, candidate.duration_secs?);
    let tolerance = RUNTIME_TOLERANCE_FLOOR_SECS.max(a.max(b) * RUNTIME_TOLERANCE_RATIO);
    ((a - b).abs() <= tolerance).then_some(SourceEquivalence::SameRuntime)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(size: i64, fingerprint: Option<&str>, duration: Option<f64>) -> SourceFacts {
        SourceFacts {
            size,
            fingerprint: fingerprint.map(str::to_owned),
            duration_secs: duration,
        }
    }

    #[test]
    fn same_size_and_fingerprint_is_identical_even_without_a_probe() {
        let a = facts(1_000, Some("abc"), None);
        let b = facts(1_000, Some("abc"), None);
        assert_eq!(classify(&a, &b), Some(SourceEquivalence::Identical));
    }

    #[test]
    fn fingerprint_alone_is_not_trusted_when_sizes_differ() {
        let a = facts(1_000, Some("abc"), Some(5_400.0));
        let b = facts(2_000, Some("abc"), Some(5_400.0));
        // Falls back to the runtime rule rather than claiming identical bytes.
        assert_eq!(classify(&a, &b), Some(SourceEquivalence::SameRuntime));
        let c = facts(2_000, Some("abc"), Some(6_300.0));
        assert_eq!(classify(&a, &c), None);
    }

    #[test]
    fn a_longer_cut_is_never_equivalent() {
        let theatrical = facts(8_000_000_000, None, Some(7_200.0));
        let extended = facts(9_000_000_000, None, Some(8_100.0));
        assert_eq!(classify(&theatrical, &extended), None);
        assert_eq!(classify(&extended, &theatrical), None);
    }

    #[test]
    fn a_reencode_with_the_same_runtime_is_equivalent_but_not_identical() {
        let remux = facts(30_000_000_000, Some("x"), Some(7_200.04));
        let encode = facts(4_000_000_000, Some("y"), Some(7_200.00));
        assert_eq!(
            classify(&remux, &encode),
            Some(SourceEquivalence::SameRuntime)
        );
    }

    #[test]
    fn runtime_tolerance_scales_but_stays_tight() {
        let a = facts(1, None, Some(3_600.0));
        // 0.2 % of an hour is 7.2 s.
        assert_eq!(
            classify(&a, &facts(1, None, Some(3_607.0))),
            Some(SourceEquivalence::SameRuntime)
        );
        assert_eq!(classify(&a, &facts(1, None, Some(3_608.0))), None);
        // Short clips keep the one-second floor.
        let short = facts(1, None, Some(60.0));
        assert_eq!(
            classify(&short, &facts(1, None, Some(60.9))),
            Some(SourceEquivalence::SameRuntime)
        );
        assert_eq!(classify(&short, &facts(1, None, Some(62.0))), None);
    }

    #[test]
    fn unknown_or_invalid_durations_are_never_equivalent() {
        let known = facts(1, None, Some(100.0));
        assert_eq!(classify(&known, &facts(1, None, None)), None);
        assert_eq!(classify(&facts(1, None, None), &known), None);
        assert_eq!(SourceFacts::from_probe_duration(Some("nan")), None);
        assert_eq!(SourceFacts::from_probe_duration(Some("-4")), None);
        assert_eq!(SourceFacts::from_probe_duration(Some("0")), None);
        assert_eq!(SourceFacts::from_probe_duration(Some("12.5")), Some(12.5));
    }

    #[test]
    fn duration_is_read_from_a_stored_probe_whatever_else_it_holds() {
        let probe = r#"{"format":{"duration":"7200.040000","tags":{"title":"x"}},
            "streams":[{"index":0,"codec_type":"video","unknown":{"deep":[1,2,3]}}],"extra":true}"#;
        assert_eq!(SourceFacts::duration_from_probe_json(probe), Some(7200.04));
        // A numeric duration (other probers) is accepted too.
        assert_eq!(
            SourceFacts::duration_from_probe_json(r#"{"format":{"duration":12.5}}"#),
            Some(12.5)
        );
        for broken in [
            "{}",
            "not json",
            r#"{"format":null}"#,
            r#"{"format":{}}"#,
            r#"{"format":{"duration":"N/A"}}"#,
            r#"{"format":{"duration":-3}}"#,
            r#"{"format":{"duration":true}}"#,
        ] {
            assert_eq!(
                SourceFacts::duration_from_probe_json(broken),
                None,
                "{broken}"
            );
        }
    }

    #[test]
    fn identical_outranks_same_runtime() {
        assert!(SourceEquivalence::Identical > SourceEquivalence::SameRuntime);
    }
}
