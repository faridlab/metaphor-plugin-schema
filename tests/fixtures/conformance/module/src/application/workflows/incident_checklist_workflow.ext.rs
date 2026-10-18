//! The hand-written half of the `IncidentChecklist` workflow (ADR-0031): the `risky_step` verb, which
//! behaves by the incident's title so a probe can choose how it goes wrong:
//!
//! - `boom...` stages a row and then fails for a reason that is not the caller's to fix: the row must
//!   not survive, and the engine retries;
//! - `refuse...` is refused as the caller's to fix: no retry;
//! - `linger...` stages a row, announces it ([`lingering`]) and never returns, so a probe can cut the
//!   run inside the step's transaction;
//! - anything else succeeds.

use super::*;

use backbone_core::write_hooks::OutboxRecord;

/// Signalled by `risky_step` once a `linger...` step is inside its transaction.
pub fn lingering() -> &'static tokio::sync::Notify {
    static N: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    N.get_or_init(tokio::sync::Notify::new)
}

/// The implementation the module's build attaches.
#[derive(Default)]
pub struct IncidentChecklistActionsImpl;

#[async_trait]
impl IncidentChecklistActions for IncidentChecklistActionsImpl {
    async fn risky_step(&self, ctx: &WriteCtx, params: &Value, _wf: &Value) -> ServiceResult<Value> {
        let title = params.get("title").and_then(Value::as_str).unwrap_or_default();
        let stage = || {
            ctx.stage(OutboxRecord::new(
                "WorkflowProbe",
                "Incident",
                title,
                ctx.company_or_nil(),
                serde_json::json!({ "title": title }),
                chrono::Utc::now(),
            ))
        };
        if title.starts_with("boom") {
            stage().await?;
            return Err(ServiceError::Internal("the risky step broke after staging a row".into()));
        }
        if title.starts_with("refuse") {
            return Err(ServiceError::Validation("the risky step was refused".into()));
        }
        if title.starts_with("linger") {
            stage().await?;
            lingering().notify_one();
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
        Ok(Value::Null)
    }
}
