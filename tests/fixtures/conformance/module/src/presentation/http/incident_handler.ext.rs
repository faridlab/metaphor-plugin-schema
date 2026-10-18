//! The extension of `incident_handler.rs` (hand-written; ADR-0031).
//!
//! `POST /incidents/:id/page`: a hand verb whose moment is a hand-fired trigger (`after_page` in
//! `incident.hook.yaml`). It reads the incident, then runs the trigger's actions on one transaction
//! with `IncidentWriteHooks::fire_after_page`, the entry point the generator emits for a trigger no
//! write fires.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use backbone_core::http::{guarded, service_error_response, ApiResponse};
use backbone_core::write_hooks::WriteCtx;
use backbone_core::{CrudRepository, ServiceError};

use crate::application::service::{IncidentService, IncidentWriteHooks};
use crate::application::triggers::IncidentTriggersImpl;
use crate::presentation::dto::IncidentResponseDto;

async fn page(
    State(service): State<Arc<IncidentService>>,
    Path(id): Path<String>,
) -> (StatusCode, Json<ApiResponse<IncidentResponseDto>>) {
    let incident = match service.get_by_id(&id).await {
        Ok(Some(i)) => i,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::not_found("Incident", &id)),
            )
        }
        Err(e) => return service_error_response(&e),
    };
    let Some(pool) = service.repository().write_pool() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("the service has no database pool")),
        );
    };
    let outcome = backbone_orm::in_write_tx_result(pool, async {
        let ctx = WriteCtx::current(Some("conformance"));
        let hooks = IncidentWriteHooks::new(Arc::new(IncidentTriggersImpl::default()));
        hooks.fire_after_page(&ctx, &incident).await?;
        Ok::<_, ServiceError>(())
    })
    .await;
    match outcome {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::ok(IncidentResponseDto::from(incident))),
        ),
        Err(backbone_orm::WriteTxFailure::Inner(e)) => service_error_response(&e),
        Err(backbone_orm::WriteTxFailure::Db(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("the page could not be committed")),
        ),
    }
}

/// The custom incident routes, each guarded by the permission of its verb.
pub fn create_incident_extension_routes(service: Arc<IncidentService>) -> Router {
    Router::new()
        .route("/incidents/:id/page", guarded(post(page), "incident:page"))
        .with_state(service)
}
