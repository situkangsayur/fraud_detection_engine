//! The JSON examples of `docs/technical/rule-dsl.md` are part of the contract: every one must parse into the
//! model and survive a serialise → parse round trip. Fixtures are copied into `tests/fixtures/` so the crate
//! tests stand alone (e.g. inside a Docker build); a second test scans the live document when it is present.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use rule_engine::model::{RuleDefinition, RuleEnvelope, RulesetSpec};
use rule_engine::validate::{validate_rule, StaticCatalog};
use serde_json::{json, Value};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn round_trip_definition(v: &Value) -> RuleDefinition {
    let def: RuleDefinition = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
    let again: RuleDefinition = serde_json::from_value(serde_json::to_value(&def).unwrap()).unwrap();
    assert_eq!(def, again);
    def
}

#[test]
fn fixture_definitions_parse_round_trip_and_validate() {
    let mut count = 0;
    let mut entries: Vec<_> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        if name.starts_with("ruleset_") {
            let spec: RulesetSpec = serde_json::from_value(v).unwrap();
            let again: RulesetSpec = serde_json::from_value(serde_json::to_value(&spec).unwrap()).unwrap();
            assert_eq!(spec, again);
        } else {
            let def = round_trip_definition(&v);
            let envelope = RuleEnvelope {
                code: "RL-FIX-001".into(),
                name: name.clone(),
                description: None,
                kind: def.kind(),
                typologies: vec![],
                event_types: vec![],
                risk_score: 10.0,
                trapped_score: 0.0,
                action: Default::default(),
                on_trapped: Default::default(),
                missing_as_no_match: false,
                definition: def,
            };
            let report = validate_rule(&envelope, &StaticCatalog::permissive());
            assert!(report.valid, "{name}: {:#?}", report.errors);
        }
        count += 1;
    }
    assert!(count >= 7, "expected all spec fixtures, found {count}");
}

#[test]
fn envelope_example_parses() {
    let v = json!({
        "code": "RL-CARD-003", "name": "Card shared by many customers",
        "description": "Same card fingerprint used by >= 3 distinct customers in 30 days",
        "kind": "velocity", "typologies": ["carding"], "event_types": ["transaction"],
        "risk_score": 45, "trapped_score": 0, "action": "score", "on_trapped": "ignore", "missing_as_no_match": false,
        "definition": serde_json::from_str::<Value>(
            &std::fs::read_to_string(fixtures_dir().join("velocity_card_shared.json")).unwrap()).unwrap()
    });
    let rule: RuleEnvelope = serde_json::from_value(v).unwrap();
    assert_eq!(rule.definition.kind(), rule.kind);
}

/// Scans every ```json block of the live spec; blocks that are valid JSON and look like a rule definition or a
/// ruleset must parse. (Blocks with placeholders such as `"...kind-specific body..."` are skipped.)
#[test]
fn live_spec_examples_parse() {
    let doc = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../docs/technical/rule-dsl.md");
    let Ok(text) = std::fs::read_to_string(&doc) else {
        eprintln!("rule-dsl.md not found at {}, skipping live scan", doc.display());
        return;
    };
    let mut checked = 0;
    for block in text.split("```json").skip(1) {
        let Some(body) = block.split("```").next() else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(body) else {
            continue;
        };
        if v.get("aggregation").is_some() {
            serde_json::from_value::<RulesetSpec>(v).unwrap();
            checked += 1;
        } else if v.get("kind").is_some() && v.get("definition").is_none() && v.get("rule_code").is_none() {
            round_trip_definition(&v);
            checked += 1;
        }
    }
    assert!(
        checked >= 7,
        "expected to check the spec examples, checked {checked}"
    );
}
