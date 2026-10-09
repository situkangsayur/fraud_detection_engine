//! Governance state machine for rules and rulesets (architecture.md §4, maker–checker).
//!
//! Rules and rulesets share the same lifecycle **and** the same versioning model: every edit creates an
//! immutable version, and what serves is derived from the approval ledger ([`serving_from_ledger`]).
//!
//! ```text
//!            submit                approve(active|shadow)
//!   draft ───────────▶ pending_approval ─────────────────▶ active | shadow
//!     ▲                     │ reject                          │  │ submit (promote shadow → active,
//!     └─────────────────────┘                                 │  │        or re-approve an edit)
//!     ▲  edit (PUT = new version)                             │  ▼
//!     └───────────────────────────────────────────────────────┘ pending_approval
//!   any non-retired ── retire ──▶ retired (terminal)
//! ```
//!
//! ## Why pure functions and an enum, not a `RuleState` class hierarchy
//!
//! In Java this is often modelled with the State pattern (`DraftState`, `PendingState`, … each overriding
//! `submit()`, `approve()`). In Rust, a closed `enum` + one `match` does the same job with less code, and the
//! compiler checks that every (status, action) pair is handled. The function takes plain values and returns a
//! `Result`: no database, no clock, no HTTP. That makes the rules of the game ("an approver may not approve
//! their own submission") trivially unit-testable, and the API layer only has to persist the outcome.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Workflow status of a rule or ruleset (`rules.rules.status`, `rules.rulesets.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Draft,
    PendingApproval,
    Active,
    Shadow,
    Retired,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::PendingApproval => "pending_approval",
            Status::Active => "active",
            Status::Shadow => "shadow",
            Status::Retired => "retired",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "draft" => Some(Status::Draft),
            "pending_approval" => Some(Status::PendingApproval),
            "active" => Some(Status::Active),
            "shadow" => Some(Status::Shadow),
            "retired" => Some(Status::Retired),
            _ => None,
        }
    }
}

/// Status an approver may promote to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Active,
    Shadow,
}

impl TargetStatus {
    pub fn as_status(self) -> Status {
        match self {
            TargetStatus::Active => Status::Active,
            TargetStatus::Shadow => Status::Shadow,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.as_status().as_str()
    }

    pub fn parse(s: &str) -> Option<TargetStatus> {
        match s {
            "active" => Some(TargetStatus::Active),
            "shadow" => Some(TargetStatus::Shadow),
            _ => None,
        }
    }
}

/// A workflow action requested by a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Create a new version (PUT). The subject goes back to draft; the previously approved version keeps serving.
    Edit,
    Submit,
    Approve(TargetStatus),
    Reject,
    Retire,
}

/// Why a transition is not allowed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LifecycleError {
    #[error("cannot {action} a {status} item")]
    InvalidTransition {
        action: &'static str,
        status: &'static str,
    },
    #[error("maker–checker: the approver must be a different user than the submitter")]
    SelfApproval,
    #[error("a user actor is required for this action")]
    ActorRequired,
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::Edit => "edit",
        Action::Submit => "submit",
        Action::Approve(_) => "approve",
        Action::Reject => "reject",
        Action::Retire => "retire",
    }
}

/// Computes the next status. `submitted_by` is the maker recorded at submission, `actor` the current user.
pub fn transition(
    current: Status,
    action: Action,
    submitted_by: Option<Uuid>,
    actor: Option<Uuid>,
) -> Result<Status, LifecycleError> {
    let invalid = || LifecycleError::InvalidTransition {
        action: action_name(action),
        status: current.as_str(),
    };
    match (current, action) {
        (Status::Retired, _) => Err(invalid()),
        (_, Action::Edit) => Ok(Status::Draft),
        (Status::Draft | Status::Shadow | Status::Active, Action::Submit) => Ok(Status::PendingApproval),
        (Status::PendingApproval, Action::Approve(target)) => {
            let actor = actor.ok_or(LifecycleError::ActorRequired)?;
            if submitted_by == Some(actor) {
                return Err(LifecycleError::SelfApproval);
            }
            Ok(target.as_status())
        }
        (Status::PendingApproval, Action::Reject) => {
            actor.ok_or(LifecycleError::ActorRequired)?;
            Ok(Status::Draft)
        }
        (_, Action::Retire) => {
            actor.ok_or(LifecycleError::ActorRequired)?;
            Ok(Status::Retired)
        }
        _ => Err(invalid()),
    }
}

/// One approved decision from the approval ledger (`core.approvals` with `decision = 'approved'`), in
/// chronological order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovedVersion {
    pub version: i32,
    pub target: TargetStatus,
}

/// What is currently live for one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, utoipa::ToSchema)]
pub struct Serving {
    /// Version contributing to scores.
    pub live_version: Option<i32>,
    /// Version evaluated in shadow (traced, never scored). Usually a newer candidate of a live rule.
    pub shadow_version: Option<i32>,
}

impl Serving {
    pub fn is_served(&self) -> bool {
        self.live_version.is_some() || self.shadow_version.is_some()
    }
}

/// Derives what serves from the approval ledger.
///
/// ## Why the ledger, not `rules.status`
///
/// `status` answers "where is the latest version in the workflow?". Serving answers a different question: "which
/// approved version runs now?". If editing a live rule (PUT → draft) took it offline until re-approval, a simple
/// typo fix would open a fraud window. Instead, the last approved version keeps serving while its successor waits
/// for approval, and a version approved into shadow runs **next to** the live one (champion/challenger).
///
/// Replay rules (chronological):
/// * approve → `active` v: `live = v`; a shadow version ≤ v is obsolete and dropped;
/// * approve → `shadow` v: `shadow = v`; if v was the live version, it is demoted (`live = None`).
pub fn serving_from_ledger(approvals: &[ApprovedVersion], retired: bool) -> Serving {
    if retired {
        return Serving::default();
    }
    let mut s = Serving::default();
    for a in approvals {
        match a.target {
            TargetStatus::Active => {
                s.live_version = Some(a.version);
                if s.shadow_version.is_some_and(|sv| sv <= a.version) {
                    s.shadow_version = None;
                }
            }
            TargetStatus::Shadow => {
                if s.live_version == Some(a.version) {
                    s.live_version = None;
                }
                s.shadow_version = Some(a.version);
            }
        }
    }
    s
}

/// Workflow status after an approval of `approved_version` into `target`, given what served before.
///
/// Approving a newer version into **shadow** while an older version is live keeps the item "active"
/// (champion/challenger): the status reflects what matters most operationally, i.e. something scores.
pub fn status_after_approval(target: TargetStatus, approved_version: i32, before: Serving) -> Status {
    match target {
        TargetStatus::Shadow if before.live_version.is_some_and(|v| v != approved_version) => Status::Active,
        _ => target.as_status(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn u(n: u128) -> Option<Uuid> {
        Some(Uuid::from_u128(n))
    }

    #[test]
    fn happy_path_draft_to_active() {
        let s = transition(Status::Draft, Action::Submit, None, u(1)).unwrap();
        assert_eq!(s, Status::PendingApproval);
        let s = transition(s, Action::Approve(TargetStatus::Active), u(1), u(2)).unwrap();
        assert_eq!(s, Status::Active);
    }

    #[test]
    fn self_approval_is_rejected() {
        let err = transition(
            Status::PendingApproval,
            Action::Approve(TargetStatus::Active),
            u(7),
            u(7),
        )
        .unwrap_err();
        assert_eq!(err, LifecycleError::SelfApproval);
    }

    #[test]
    fn approve_requires_a_user() {
        let err = transition(
            Status::PendingApproval,
            Action::Approve(TargetStatus::Shadow),
            u(1),
            None,
        )
        .unwrap_err();
        assert_eq!(err, LifecycleError::ActorRequired);
    }

    #[test]
    fn cannot_approve_a_draft_or_touch_retired() {
        assert!(transition(Status::Draft, Action::Approve(TargetStatus::Active), None, u(2)).is_err());
        for a in [Action::Edit, Action::Submit, Action::Reject, Action::Retire] {
            assert!(transition(Status::Retired, a, None, u(2)).is_err());
        }
        assert!(transition(Status::PendingApproval, Action::Submit, None, u(2)).is_err());
    }

    #[test]
    fn reject_returns_to_draft_and_shadow_can_be_promoted() {
        assert_eq!(
            transition(Status::PendingApproval, Action::Reject, u(1), u(2)).unwrap(),
            Status::Draft
        );
        assert_eq!(
            transition(Status::Shadow, Action::Submit, None, u(1)).unwrap(),
            Status::PendingApproval
        );
        assert_eq!(
            transition(Status::Active, Action::Edit, None, u(1)).unwrap(),
            Status::Draft
        );
        assert_eq!(
            transition(Status::Active, Action::Retire, None, u(1)).unwrap(),
            Status::Retired
        );
    }

    fn a(version: i32, target: TargetStatus) -> ApprovedVersion {
        ApprovedVersion { version, target }
    }

    #[test]
    fn serving_keeps_live_version_while_edit_is_pending() {
        // v1 approved active, v2 created (no approval yet) → v1 still live.
        let s = serving_from_ledger(&[a(1, TargetStatus::Active)], false);
        assert_eq!(
            s,
            Serving {
                live_version: Some(1),
                shadow_version: None
            }
        );
    }

    #[test]
    fn serving_champion_challenger_then_promotion() {
        let ledger = [a(1, TargetStatus::Active), a(2, TargetStatus::Shadow)];
        assert_eq!(
            serving_from_ledger(&ledger, false),
            Serving {
                live_version: Some(1),
                shadow_version: Some(2)
            }
        );
        let ledger = [
            a(1, TargetStatus::Active),
            a(2, TargetStatus::Shadow),
            a(2, TargetStatus::Active),
        ];
        assert_eq!(
            serving_from_ledger(&ledger, false),
            Serving {
                live_version: Some(2),
                shadow_version: None
            }
        );
    }

    #[test]
    fn serving_demotion_and_retirement() {
        let ledger = [a(1, TargetStatus::Active), a(1, TargetStatus::Shadow)];
        assert_eq!(
            serving_from_ledger(&ledger, false),
            Serving {
                live_version: None,
                shadow_version: Some(1)
            }
        );
        assert_eq!(serving_from_ledger(&ledger, true), Serving::default());
        assert!(!serving_from_ledger(&[], false).is_served());
    }

    #[test]
    fn status_after_approval_keeps_champion_active() {
        let live1 = Serving {
            live_version: Some(1),
            shadow_version: None,
        };
        assert_eq!(
            status_after_approval(TargetStatus::Shadow, 2, live1),
            Status::Active
        );
        assert_eq!(
            status_after_approval(TargetStatus::Shadow, 1, live1),
            Status::Shadow
        );
        assert_eq!(
            status_after_approval(TargetStatus::Shadow, 1, Serving::default()),
            Status::Shadow
        );
        assert_eq!(
            status_after_approval(TargetStatus::Active, 2, live1),
            Status::Active
        );
    }
}
