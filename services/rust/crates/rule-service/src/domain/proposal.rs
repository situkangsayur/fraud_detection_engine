//! Rule proposals (from the LLM or an analyst) and their review lifecycle.
//!
//! A proposal is *data*, never an executable change: approving one creates a new rule, or a new version of an
//! existing rule, in **shadow** only. Promotion to `active` is a second, separate maker–checker approval on the
//! rule itself (architecture.md §4: "the LLM never activates anything").

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalType {
    NewRule,
    ModifyRule,
    RetireRule,
    TuneThreshold,
}

impl ProposalType {
    pub fn as_str(self) -> &'static str {
        match self {
            ProposalType::NewRule => "new_rule",
            ProposalType::ModifyRule => "modify_rule",
            ProposalType::RetireRule => "retire_rule",
            ProposalType::TuneThreshold => "tune_threshold",
        }
    }

    pub fn parse(s: &str) -> Option<ProposalType> {
        match s {
            "new_rule" => Some(ProposalType::NewRule),
            "modify_rule" => Some(ProposalType::ModifyRule),
            "retire_rule" => Some(ProposalType::RetireRule),
            "tune_threshold" => Some(ProposalType::TuneThreshold),
            _ => None,
        }
    }

    /// Whether the proposal carries a rule envelope in `definition`.
    pub fn needs_definition(self) -> bool {
        !matches!(self, ProposalType::RetireRule)
    }

    /// Whether the proposal targets an existing rule.
    pub fn needs_target(self) -> bool {
        !matches!(self, ProposalType::NewRule)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalSource {
    Llm,
    Analyst,
}

impl ProposalSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ProposalSource::Llm => "llm",
            ProposalSource::Analyst => "analyst",
        }
    }
}

/// Structural checks of a new proposal (semantic rule validation is done by the rule engine).
pub fn check_new(
    proposal_type: ProposalType,
    has_definition: bool,
    target_rule_id: Option<Uuid>,
) -> Result<(), &'static str> {
    if proposal_type.needs_definition() && !has_definition {
        return Err("definition is required for this proposal type");
    }
    if proposal_type.needs_target() && target_rule_id.is_none() {
        return Err("target_rule_id is required for this proposal type");
    }
    if !proposal_type.needs_target() && target_rule_id.is_some() {
        return Err("target_rule_id must be empty for new_rule");
    }
    Ok(())
}

/// Review decision on a pending proposal.
pub fn review(status: &str, created_by: Option<Uuid>, reviewer: Uuid) -> Result<(), &'static str> {
    if status != "pending" {
        return Err("only pending proposals can be reviewed");
    }
    if created_by == Some(reviewer) {
        return Err("maker–checker: the reviewer must differ from the proposal author");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_checks() {
        assert!(check_new(ProposalType::NewRule, true, None).is_ok());
        assert!(check_new(ProposalType::NewRule, false, None).is_err());
        assert!(check_new(ProposalType::NewRule, true, Some(Uuid::nil())).is_err());
        assert!(check_new(ProposalType::ModifyRule, true, None).is_err());
        assert!(check_new(ProposalType::RetireRule, false, Some(Uuid::nil())).is_ok());
        assert!(check_new(ProposalType::TuneThreshold, true, Some(Uuid::nil())).is_ok());
    }

    #[test]
    fn review_rules() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        assert!(review("pending", Some(a), b).is_ok());
        assert!(review("pending", Some(a), a).is_err());
        assert!(review("approved", Some(a), b).is_err());
        assert!(review("pending", None, b).is_ok()); // LLM-authored (no user) proposals
    }
}
