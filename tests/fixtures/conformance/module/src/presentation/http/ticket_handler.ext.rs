//! The extension of `ticket_handler.rs` (hand-written; ADR-0031).
//!
//! Two custom routes over the generated `TicketService`, mounted by the module's extension
//! (`lib.ext.rs`) beside the generated routes:
//!
//! - `POST /tickets/:id/archive-with-reason` — the custom service method
//!   (`TicketServiceExt::archive_with_reason`) behind HTTP.
//! - `POST /tickets/:id/escalate` — a custom business flow: it reads the caller's org scope off
//!   the request (set by `org_auth`, never by the body), starts the ticket through the
//!   generated machine's `start` verb, and stamps the note with the acting unit. The write runs
//!   through the generic service, so the schema's guard and the org fence both apply.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use backbone_core::http::{service_error_response, ApiResponse};
use backbone_core::write_hooks::{DeclaredEvent, WriteCtx};
use backbone_core::{CrudRepository, ServiceError};

use crate::application::service::{TicketService, TicketServiceExt};
use crate::domain::event::declared;
use crate::presentation::dto::TicketResponseDto;

/// The body of the archive verb.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveBody {
    /// Why the ticket is archived.
    pub reason: String,
}

/// The body of the escalate flow.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscalateBody {
    /// What the escalation says.
    pub note: String,
}

type Answer = (StatusCode, Json<ApiResponse<TicketResponseDto>>);

async fn archive_with_reason(
    State(service): State<Arc<TicketService>>,
    Path(id): Path<String>,
    Json(body): Json<ArchiveBody>,
) -> Answer {
    match service.archive_with_reason(&id, &body.reason).await {
        Ok(Some(t)) => (StatusCode::OK, Json(ApiResponse::ok(TicketResponseDto::from(t)))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(ApiResponse::not_found("Ticket", &id))),
        Err(e) => service_error_response(&e),
    }
}

async fn escalate(
    State(service): State<Arc<TicketService>>,
    Path(id): Path<String>,
    Json(body): Json<EscalateBody>,
) -> Answer {
    let Some(scope) = backbone_orm::org_scope::current_org_scope() else {
        return (StatusCode::UNAUTHORIZED, Json(ApiResponse::error("escalation needs an org-scoped session")));
    };
    let Some(pool) = service.repository().write_pool() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error("the service has no database pool")));
    };
    let mut fields = HashMap::new();
    fields.insert("status".to_string(), serde_json::json!("in_progress"));
    fields.insert(
        "note".to_string(),
        serde_json::json!(format!("escalated at {}: {}", scope.acting_unit_id(), body.note)),
    );
    // The flow is one transaction: the verb's write, the generated hooks' events, and the event
    // this hand code stages. The service's own transaction joins it as a savepoint.
    let outcome = backbone_orm::in_write_tx_result(pool, async {
        let moved = backbone_core::write_guard::as_transition("start", service.partial_update(&id, fields)).await?;
        if let Some(ticket) = &moved {
            let ctx = WriteCtx::current(Some("conformance"));
            let event = declared::TicketEscalated { id: ticket.id, project_id: ticket.project_id };
            ctx.stage(event.record(&ticket.id.to_string(), &ctx)).await?;
        }
        Ok::<_, ServiceError>(moved)
    })
    .await;
    match outcome {
        Ok(Some(t)) => (StatusCode::OK, Json(ApiResponse::ok(TicketResponseDto::from(t)))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(ApiResponse::not_found("Ticket", &id))),
        Err(backbone_orm::WriteTxFailure::Inner(e)) => service_error_response(&e),
        Err(backbone_orm::WriteTxFailure::Db(_)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error("the escalation could not be committed")))
        }
    }
}

/// The custom ticket routes.
pub fn create_ticket_extension_routes(service: Arc<TicketService>) -> Router {
    Router::new()
        .route("/tickets/:id/archive-with-reason", post(archive_with_reason))
        .route("/tickets/:id/escalate", post(escalate))
        .with_state(service)
}
