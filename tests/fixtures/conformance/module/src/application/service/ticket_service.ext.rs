//! The extension of `ticket_service.rs` (hand-written; ADR-0031).
//!
//! A custom service method: archiving a ticket with a reason. The schema's rule
//! `archive_reason_verb` names this function as where it is enforced (`enforced_by`), so the
//! register labels it VERB.

use std::collections::HashMap;

use backbone_core::violation::Violation;
use backbone_core::{ServiceError, ServiceResult};

use crate::domain::entity::Ticket;
use crate::domain::state_machine::{TicketState, TicketStateMachine, TicketTransition};

/// Operations the module adds to the generated `TicketService`.
#[async_trait::async_trait]
pub trait TicketServiceExt {
    /// Archive a resolved ticket, recording why. Refuses a blank reason
    /// (`archive_reason_required`) and a ticket the machine cannot archive
    /// (`transition_not_allowed`), then moves the state through the generic service, so the
    /// schema's guard and the database's transition trigger still hold.
    async fn archive_with_reason(&self, id: &str, reason: &str) -> ServiceResult<Option<Ticket>>;
}

#[async_trait::async_trait]
impl TicketServiceExt for super::TicketService {
    async fn archive_with_reason(&self, id: &str, reason: &str) -> ServiceResult<Option<Ticket>> {
        archive_with_reason(self, id, reason).await
    }
}

/// The verb itself; the rule register checks this function exists.
pub async fn archive_with_reason(
    service: &super::TicketService,
    id: &str,
    reason: &str,
) -> ServiceResult<Option<Ticket>> {
    if reason.trim().is_empty() {
        return Err(ServiceError::Violations(vec![Violation::new(
            "archive_reason",
            "archive_reason_required",
            "archiving needs a reason",
        )]));
    }
    let Some(ticket) = service.get_by_id(id).await? else {
        return Ok(None);
    };
    let state: TicketState = ticket
        .status
        .to_string()
        .parse()
        .map_err(|_| ServiceError::Internal("stored status is not a machine state".into()))?;
    if !TicketStateMachine::from_state(state).can_transition(TicketTransition::Archive) {
        return Err(ServiceError::Violations(vec![Violation::new(
            "status",
            "transition_not_allowed",
            "only a resolved ticket can be archived",
        )]));
    }
    let mut fields = HashMap::new();
    fields.insert("status".to_string(), serde_json::json!("archived"));
    fields.insert("archive_reason".to_string(), serde_json::json!(reason));
    backbone_core::write_guard::as_transition("archive", service.partial_update(id, fields)).await
}
