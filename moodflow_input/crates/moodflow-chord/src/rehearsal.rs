//! Replay a bounded, complete attempt without touching the live input session.
use crate::settle::Settlement;
use crate::{ChordEvent, Keymap, KeymapError, OutputKind, Settler};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_STROKES: usize = 128;
const MAX_ATTEMPT_MS: u64 = 120_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    pub key: String,
    pub down: bool,
    pub at_ms: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RehearsalResult {
    pub keys: Vec<String>,
    pub output: Option<String>,
    pub kind: Option<OutputKind>,
    pub at_ms: u64,
    pub idle: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RehearsalError {
    Profile(KeymapError),
    TooManyStrokes,
    InvalidTime,
    UnknownKey,
    Unbalanced,
}

fn record(output: &mut Vec<RehearsalResult>, settled: Option<Settlement>, at_ms: u64, idle: bool) {
    if let Some((keys, event)) = settled {
        let (text, kind) = match event {
            ChordEvent::Emit { output, kind } => (Some(output), Some(kind)),
            ChordEvent::NoMatch { .. } => (None, None),
        };
        output.push(RehearsalResult {
            keys: keys.into_iter().collect(),
            output: text,
            kind,
            at_ms,
            idle,
        });
    }
}

/// The clock is relative to the first captured press. After the final release,
/// advance by one interval so any pending half gets feedback in isolation.
pub fn rehearse(map: &Keymap, strokes: &[Stroke]) -> Result<Vec<RehearsalResult>, RehearsalError> {
    if strokes.len() > MAX_STROKES {
        return Err(RehearsalError::TooManyStrokes);
    }
    map.validate_for_apply().map_err(RehearsalError::Profile)?;
    if strokes.is_empty() {
        return Ok(Vec::new());
    }
    let mut settler = Settler::new(map);
    let mut held = HashSet::new();
    let mut previous = 0;
    let mut results = Vec::new();
    for stroke in strokes {
        if stroke.at_ms < previous || stroke.at_ms > MAX_ATTEMPT_MS {
            return Err(RehearsalError::InvalidTime);
        }
        previous = stroke.at_ms;
        let key = stroke.key.as_str();
        if !settler.is_chord_key(key) {
            return Err(RehearsalError::UnknownKey);
        }
        let outcome = if stroke.down && held.insert(key) {
            settler.down_with_keys(key, stroke.at_ms)
        } else if !stroke.down && held.remove(key) {
            settler.up_with_keys(key, stroke.at_ms)
        } else {
            return Err(RehearsalError::Unbalanced);
        };
        record(&mut results, outcome, stroke.at_ms, false);
    }
    if !held.is_empty() {
        return Err(RehearsalError::Unbalanced);
    }
    let deadline = previous + map.interval_ms;
    record(&mut results, settler.expire(deadline), deadline, true);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strokes(events: &[(&str, bool, u64)]) -> Vec<Stroke> {
        events
            .iter()
            .map(|&(key, down, at_ms)| Stroke {
                key: key.into(),
                down,
                at_ms,
            })
            .collect()
    }

    #[test]
    fn overlapping_and_staggered_strokes_get_the_same_exact_chord() {
        let map = crate::keymap::tests::sample();
        for (release, other_down) in [(90, 30), (30, 90), (30, 329)] {
            let mut events = vec![
                ("s", true, 0),
                ("j", true, other_down),
                ("s", false, release),
                ("j", false, 375),
            ];
            events.sort_by_key(|e| e.2);
            let results = rehearse(&map, &strokes(&events)).unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].keys, ["j", "s"]);
            assert_eq!(results[0].output.as_deref(), Some("zhang"));
            assert_eq!(results[0].kind, Some(OutputKind::Syllable));
            assert!(!results[0].idle);
        }
    }

    #[test]
    fn exact_timeout_splits_the_halves_and_reports_each_attempt() {
        let map = crate::keymap::tests::sample();
        let events = [
            ("s", true, 0),
            ("s", false, 30),
            ("j", true, 330),
            ("j", false, 370),
        ];
        let results = rehearse(&map, &strokes(&events)).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].keys, ["s"]);
        assert_eq!(results[0].output, None);
        assert_eq!(results[0].at_ms, 330);
        assert_eq!(results[1].keys, ["j"]);
        assert_eq!(results[1].output, None);
        assert!(results[1].idle);
        assert_eq!(results[1].at_ms, 670);
    }

    #[test]
    fn released_single_key_can_resolve_to_a_fragment_after_wait() {
        let map = crate::keymap::tests::sample();
        let events = strokes(&[("a", true, 0), ("a", false, 50)]);
        let results = rehearse(&map, &events).unwrap();
        assert_eq!(results[0].output.as_deref(), Some("zh"));
        assert_eq!(results[0].kind, Some(OutputKind::Fragment));
        assert!(!results[0].idle, "a is not a prefix in the sample");
        let mut broader_map = map;
        broader_map.mappings[0].keys.push("a".into());
        let results = rehearse(&broader_map, &events).unwrap();
        assert_eq!(results[0].output.as_deref(), Some("zh"));
        assert!(results[0].idle);
        assert_eq!(results[0].at_ms, 350);
    }

    #[test]
    fn malformed_or_unfinished_attempts_cannot_yield_partial_feedback() {
        let map = crate::keymap::tests::sample();
        for (events, expected) in [
            (vec![("s", true, 0)], RehearsalError::Unbalanced),
            (
                vec![("s", true, 0), ("s", true, 1)],
                RehearsalError::Unbalanced,
            ),
            (vec![("j", false, 0)], RehearsalError::Unbalanced),
            (vec![("q", true, 0)], RehearsalError::UnknownKey),
            (
                vec![("s", true, 5), ("s", false, 4)],
                RehearsalError::InvalidTime,
            ),
            (
                vec![("s", true, 0), ("s", false, 120_001)],
                RehearsalError::InvalidTime,
            ),
            (
                vec![("s", true, 0); MAX_STROKES + 1],
                RehearsalError::TooManyStrokes,
            ),
        ] {
            assert_eq!(rehearse(&map, &strokes(&events)), Err(expected));
        }
    }
}
