//! Small shared vocabularies (enums and constants).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Fraud typologies (architecture.md §1). Also used as `labels.fraud_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Typology {
    Carding,
    AccountTakeover,
    BankAccountTakeover,
    SystemBreach,
    PromoAbuse,
    RefundAbuse,
    MoneyMule,
    Other,
}

impl Typology {
    pub const ALL: [Typology; 8] = [
        Self::Carding,
        Self::AccountTakeover,
        Self::BankAccountTakeover,
        Self::SystemBreach,
        Self::PromoAbuse,
        Self::RefundAbuse,
        Self::MoneyMule,
        Self::Other,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Carding => "carding",
            Self::AccountTakeover => "account_takeover",
            Self::BankAccountTakeover => "bank_account_takeover",
            Self::SystemBreach => "system_breach",
            Self::PromoAbuse => "promo_abuse",
            Self::RefundAbuse => "refund_abuse",
            Self::MoneyMule => "money_mule",
            Self::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

/// Well-known event types. `event_type` is free text on the wire (projects may define custom
/// types via mappings); these constants are the ones the feature set and templates understand.
#[derive(Debug)]
pub struct EventType;

impl EventType {
    pub const TRANSACTION: &'static str = "transaction";
    pub const LOGIN: &'static str = "login";
    pub const ACCOUNT_CHANGE: &'static str = "account_change";
    pub const PROMO_REDEMPTION: &'static str = "promo_redemption";
    pub const PAYOUT: &'static str = "payout";
    pub const REGISTRATION: &'static str = "registration";
    /// Return / refund request (used by `returns` stage projects).
    pub const REFUND: &'static str = "refund";

    pub const KNOWN: [&'static str; 7] = [
        Self::TRANSACTION,
        Self::LOGIN,
        Self::ACCOUNT_CHANGE,
        Self::PROMO_REDEMPTION,
        Self::PAYOUT,
        Self::REGISTRATION,
        Self::REFUND,
    ];

    /// Valid event type syntax: lower snake case, 2–40 chars.
    pub fn is_valid(s: &str) -> bool {
        (2..=40).contains(&s.len())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
    }
}

/// Final decision of the scoring pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Review,
    Decline,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Review => "review",
            Self::Decline => "decline",
        }
    }
}

/// Tri-state rule outcome (rule-dsl §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleOutcome {
    Match,
    NoMatch,
    Trapped,
}

/// Effect of a matched rule (rule-dsl §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    #[default]
    Score,
    ForceReview,
    ForceDecline,
    ForceApprove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    Simple,
    Velocity,
    Composite,
    Reference,
    Graph,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typology_roundtrip() {
        for t in Typology::ALL {
            assert_eq!(Typology::parse(t.as_str()), Some(t));
            assert_eq!(serde_json::to_value(t).ok(), Some(serde_json::json!(t.as_str())));
        }
    }

    #[test]
    fn event_type_syntax() {
        assert!(EventType::is_valid("promo_redemption"));
        assert!(!EventType::is_valid("Promo"));
        assert!(!EventType::is_valid("1abc"));
    }
}
