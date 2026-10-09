//! Stage templates (multi-tenancy.md §2): typical reference lists, rules and rulesets per project stage.
//!
//! Templates are JSON files compiled into the binary with `include_str!`. They are data, not code: adding a
//! template means adding a file and one line to [`TEMPLATES`]. A unit test validates every template rule with
//! the real rule-engine validator, so a broken template fails the build pipeline instead of a customer bootstrap.

use serde::Deserialize;
use serde_json::Value;

/// `(stage, json)` of every embedded template.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("pre_payment", include_str!("../../templates/pre_payment.json")),
    ("post_payment", include_str!("../../templates/post_payment.json")),
    ("returns", include_str!("../../templates/returns.json")),
    ("promo", include_str!("../../templates/promo.json")),
    (
        "account_security",
        include_str!("../../templates/account_security.json"),
    ),
    ("payout", include_str!("../../templates/payout.json")),
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub stage: String,
    pub description: String,
    #[serde(default)]
    pub reference_lists: Vec<TemplateList>,
    /// Rule envelopes (rule-dsl §2), kept as JSON and validated by the engine at bootstrap.
    #[serde(default)]
    pub rules: Vec<Value>,
    #[serde(default)]
    pub rulesets: Vec<TemplateRuleset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateList {
    pub name: String,
    pub description: String,
    pub list_type: String,
    pub key_kind: String,
    #[serde(default)]
    pub columns: Value,
    #[serde(default)]
    pub entries: Vec<TemplateEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateEntry {
    pub key: String,
    #[serde(default)]
    pub attributes: Value,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateRuleset {
    pub code: String,
    pub name: String,
    pub description: String,
    pub event_types: Vec<String>,
    pub typologies: Vec<String>,
    pub aggregation: String,
    pub max_score: f64,
    pub members: Vec<TemplateMember>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateMember {
    /// Rule code within the same template.
    pub rule: String,
    pub weight: f64,
}

/// Names of the available templates.
pub fn names() -> Vec<&'static str> {
    TEMPLATES.iter().map(|(n, _)| *n).collect()
}

/// Loads a template by stage name.
pub fn load(name: &str) -> Result<Template, String> {
    let (_, raw) = TEMPLATES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("unknown template '{name}' (available: {})", names().join(", ")))?;
    serde_json::from_str(raw).map_err(|e| format!("template '{name}' is malformed: {e}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::adapters::catalog::ProjectCatalog;
    use rule_engine::validate_rule_json;
    use std::collections::HashSet;

    #[test]
    fn every_template_parses_and_every_rule_validates() {
        let catalog = ProjectCatalog::builtins_only();
        for (name, _) in TEMPLATES {
            let t = load(name).unwrap();
            assert_eq!(&t.stage, name);
            let mut codes = HashSet::new();
            for rule in &t.rules {
                let (report, parsed) = validate_rule_json(rule, &catalog);
                assert!(report.valid, "{name}/{}: {:#?}", rule["code"], report.errors);
                let parsed = parsed.unwrap();
                assert!(
                    codes.insert(parsed.code.clone()),
                    "duplicate code {}",
                    parsed.code
                );
                for list in &report.referenced_lists {
                    assert!(
                        t.reference_lists.iter().any(|l| &l.name == list),
                        "{name}: rule {} references list {list} not defined in the template",
                        parsed.code
                    );
                }
            }
            for rs in &t.rulesets {
                assert!(!rs.members.is_empty(), "{name}/{} has no members", rs.code);
                for m in &rs.members {
                    assert!(
                        codes.contains(&m.rule),
                        "{name}/{}: unknown member {}",
                        rs.code,
                        m.rule
                    );
                }
            }
        }
    }

    #[test]
    fn templates_cover_all_rule_kinds_and_typologies() {
        let mut kinds = HashSet::new();
        let mut typologies = HashSet::new();
        let mut has_formula = false;
        let mut stats = HashSet::new();
        for (name, raw) in TEMPLATES {
            let t = load(name).unwrap();
            for r in &t.rules {
                kinds.insert(r["kind"].as_str().unwrap().to_string());
                for ty in r["typologies"].as_array().unwrap() {
                    typologies.insert(ty.as_str().unwrap().to_string());
                }
            }
            has_formula |= raw.contains("\"formula\"");
            for s in ["zscore", "gaussian_tail", "linear_trend", "poisson_tail"] {
                if raw.contains(&format!("\"fn\": \"{s}\"")) {
                    stats.insert(s);
                }
            }
        }
        for k in ["simple", "velocity", "composite", "reference", "graph"] {
            assert!(kinds.contains(k), "no template rule of kind {k}");
        }
        for ty in [
            "carding",
            "account_takeover",
            "bank_account_takeover",
            "system_breach",
            "promo_abuse",
            "refund_abuse",
            "money_mule",
        ] {
            assert!(typologies.contains(ty), "no template rule for typology {ty}");
        }
        assert!(has_formula);
        assert_eq!(stats.len(), 4, "{stats:?}");
    }

    #[test]
    fn unknown_template_is_an_error() {
        assert!(load("nope").unwrap_err().contains("available"));
    }
}
