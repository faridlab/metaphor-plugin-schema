//! The hand-written half of the `IncidentEscalation` workflow (ADR-0031): what its steps cannot say.
//! Each method is required by the generated `IncidentEscalationActions` trait.
//!
//! - `page_responder` stages a `ResponderPaged` row on the step's own transaction, so the proof that
//!   a bespoke verb runs inside the step (and commits with it) reads that row;
//! - `fn_on_call_name` is the declared function `on_call_name(..)` an expression of the workflow calls.

use super::*;

use backbone_core::write_hooks::OutboxRecord;

/// The implementation the module's build attaches.
#[derive(Default)]
pub struct IncidentEscalationActionsImpl;

#[async_trait]
impl IncidentEscalationActions for IncidentEscalationActionsImpl {
    async fn page_responder(&self, ctx: &WriteCtx, params: &Value, _wf: &Value) -> ServiceResult<Value> {
        let incident = params.get("incident_id").and_then(Value::as_str).unwrap_or_default();
        ctx.stage(OutboxRecord::new(
            "ResponderPaged",
            "Incident",
            incident,
            ctx.company_or_nil(),
            serde_json::json!({ "who": params.get("who").cloned().unwrap_or(Value::Null) }),
            chrono::Utc::now(),
        ))
        .await?;
        Ok(serde_json::json!({ "paged": true }))
    }

    async fn fn_on_call_name(&self, _ctx: &WriteCtx, args: &[Value]) -> ServiceResult<Value> {
        let severity = args.first().and_then(Value::as_i64).unwrap_or(0);
        Ok(Value::String(format!("responder-sev{severity}")))
    }
}
