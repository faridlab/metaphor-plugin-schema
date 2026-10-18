//! The hand-written half of the triggers of `Incident` (ADR-0031): what the schema's `triggers:`
//! cannot say. The generated write hooks call these methods in file order, inside the write's
//! transaction; an error rolls the write back with everything it staged.
//!
//! - `screen_title` refuses an incident whose title says `veto`, after the hooks staged their events
//!   (so the proof that a refusal takes them down has something to take down);
//! - `spawn_follow_up` writes the next incident of a `chain:<n>/<max>` title through the generated service,
//!   which joins the transaction: a chain that is too long meets the nesting cap;
//! - `note_reopen` and `page_on_call` stage a row of their own on the write's transaction;
//! - `sweep_stale` is the scheduled entry's method.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use backbone_core::violation::Violation;
use backbone_core::write_hooks::OutboxRecord;

use crate::application::service::IncidentService;
use crate::presentation::dto::CreateIncidentDto;

use super::*;

/// The service the follow-up trigger writes through, bound by the module's build step
/// (`lib.ext.rs`). Held weakly: the service owns the hooks that own this.
static INCIDENTS: OnceLock<Weak<IncidentService>> = OnceLock::new();

/// Bind the incident service the `spawn_follow_up` trigger writes through.
pub fn bind_incident_service(service: &Arc<IncidentService>) {
    let _ = INCIDENTS.set(Arc::downgrade(service));
}

static SWEEPS: AtomicUsize = AtomicUsize::new(0);

/// How many times the scheduled `sweep_stale` entry ran.
pub fn sweeps_run() -> usize {
    SWEEPS.load(Ordering::SeqCst)
}

/// The implementation the generated service builder attaches.
#[derive(Default)]
pub struct IncidentTriggersImpl;

fn stage_row(ctx: &WriteCtx, kind: &str, e: &Incident, payload: serde_json::Value) -> OutboxRecord {
    let company = ctx.company_of(&serde_json::json!({ "company_id": e.company_id }));
    OutboxRecord::new(
        kind,
        "Incident",
        e.id.to_string(),
        company,
        payload,
        chrono::Utc::now(),
    )
}

#[async_trait]
impl IncidentTriggers for IncidentTriggersImpl {
    async fn after_create__screen_title(&self, _ctx: &WriteCtx, e: &Incident) -> ServiceResult<()> {
        if e.title.contains("veto") {
            return Err(ServiceError::Violations(vec![Violation::new(
                "title",
                "trigger_refused",
                "the title screen refused this incident",
            )]));
        }
        Ok(())
    }

    async fn after_create__spawn_follow_up(
        &self,
        _ctx: &WriteCtx,
        e: &Incident,
    ) -> ServiceResult<()> {
        // `chain:<n>/<max>`: write `chain:<n+1>/<max>` until `n` reaches `max`.
        let Some((n, max)) = e
            .title
            .strip_prefix("chain:")
            .and_then(|rest| rest.split_once('/'))
            .and_then(|(n, max)| Some((n.parse::<u32>().ok()?, max.parse::<u32>().ok()?)))
        else {
            return Ok(());
        };
        if n >= max {
            return Ok(());
        }
        let service = INCIDENTS
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| ServiceError::Internal("the incident service is not bound".into()))?;
        service
            .create(CreateIncidentDto {
                company_id: e.company_id,
                title: format!("chain:{}/{max}", n + 1),
                severity: 1,
                sla: None,
                handled_by: None,
            })
            .await?;
        Ok(())
    }

    async fn after_reopen__note_reopen(
        &self,
        ctx: &WriteCtx,
        old: &Incident,
        e: &Incident,
    ) -> ServiceResult<()> {
        ctx.stage(stage_row(
            ctx,
            "ReopenNoted",
            e,
            serde_json::json!({ "was": old.status.to_string(), "is": e.status.to_string() }),
        ))
        .await
    }

    async fn after_page__page_on_call(&self, ctx: &WriteCtx, e: &Incident) -> ServiceResult<()> {
        ctx.stage(stage_row(
            ctx,
            "OnCallPaged",
            e,
            serde_json::json!({ "title": e.title }),
        ))
        .await
    }

    async fn scheduled__sweep_stale(&self, _ctx: &WriteCtx) -> ServiceResult<()> {
        SWEEPS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
