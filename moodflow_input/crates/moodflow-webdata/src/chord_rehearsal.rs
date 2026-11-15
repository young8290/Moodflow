//! The Hub can validate and replay an unsaved chord draft. No host state is read or modified.
use moodflow_chord::{Keymap, MAX_STROKES, RehearsalError, Stroke, rehearse};
use moodflow_keys::keymap::{key_name_to_vk_with_letters, symbol_keys};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const PROFILE_LIMIT: usize = 512 * 1024;

pub(super) fn dispatch(params: &Value) -> anyhow::Result<Value> {
    let text = super::str_param(params, "text")?;
    let strokes = params
        .get("strokes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("缺少练习按键"))?;
    if text.len() > PROFILE_LIMIT {
        return Ok(json!({ "error": "profile_limit" }));
    }
    if strokes.len() > MAX_STROKES {
        return Ok(json!({ "error": "trace_limit" }));
    }
    let Ok(map) = Keymap::from_json(text) else {
        return Ok(json!({ "error": "invalid_profile" }));
    };
    if map.mappings.is_empty() {
        return Ok(json!({ "error": "empty_profile" }));
    }
    let symbols: BTreeMap<_, _> = symbol_keys()
        .filter_map(|(name, ch)| key_name_to_vk_with_letters(name).map(|vk| (vk, ch)))
        .collect();
    let mut physical = BTreeMap::new();
    for name in map.left_keys.iter().chain(&map.right_keys) {
        let normalized = name.trim().to_ascii_lowercase();
        let ch = if normalized.len() == 1 && normalized.as_bytes()[0].is_ascii_lowercase() {
            normalized.chars().next()
        } else {
            key_name_to_vk_with_letters(name).and_then(|vk| symbols.get(&vk).copied())
        };
        let Some(ch) = ch else {
            return Ok(json!({ "error": "unsupported_key", "key": name }));
        };
        if physical.insert(ch, name).is_some() {
            return Ok(json!({ "error": "duplicate_key", "key": name }));
        }
    }
    let strokes: Vec<Stroke> = serde_json::from_value(Value::Array(strokes.clone()))?;
    let outcomes = match rehearse(&map, &strokes) {
        Ok(results) => results,
        Err(RehearsalError::TooManyStrokes) => return Ok(json!({ "error": "trace_limit" })),
        Err(_) => return Ok(json!({ "error": "invalid_trace" })),
    };
    Ok(json!({ "physical": physical, "intervalMs": map.interval_ms,
        "maxStrokes": MAX_STROKES, "outcomes": outcomes }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(left: &[&str], right: &[&str], strokes: Value) -> Value {
        let text = json!({
            "version": 1, "name": "样例", "left_keys": left, "right_keys": right,
            "mappings": [{"keys": [left[0]], "output": "a", "kind": "fragment"}]
        })
        .to_string();
        json!({ "text": text, "strokes": strokes })
    }

    #[test]
    fn supported_aliases_match_the_runtime_and_preserve_the_draft_key_name() {
        let params = input(
            &["grave", "A"],
            &["equals"],
            json!([
                {"key":"grave", "down":true, "atMs":0},
                {"key":"grave", "down":false, "atMs":10},
            ]),
        );
        let result = dispatch(&params).unwrap();
        assert_eq!(
            result["physical"],
            json!({"a":"A", "`":"grave", "=":"equals"})
        );
        assert_eq!(result["intervalMs"], 300);
        assert_eq!(
            result["outcomes"],
            json!([{
                "keys":["grave"], "output":"a", "kind":"fragment", "atMs":10, "idle":false
            }])
        );
    }

    #[test]
    fn impossible_physical_layouts_and_partial_trials_report_distinct_errors() {
        let empty = json!([]);
        assert_eq!(
            dispatch(&input(&["grave"], &["backtick"], empty.clone())).unwrap()["error"],
            "duplicate_key"
        );
        assert_eq!(
            dispatch(&input(&["a"], &["numpad1"], empty)).unwrap()["error"],
            "unsupported_key"
        );
        assert_eq!(
            dispatch(&input(
                &["a"],
                &["k"],
                json!([{"key":"a", "down":true, "atMs":0}])
            ))
            .unwrap()["error"],
            "invalid_trace"
        );
    }
}
