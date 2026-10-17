//! The extension of `project_service.rs` (hand-written; ADR-0031).
//!
//! Lifecycle hooks, an event publisher and a hand-written check for `Project`, composed onto a
//! second service over the generated repository. The generated `build_project_service` attaches
//! only the schema's guard and no hooks, so a module that wants hooks builds its service here.
//! Every hook and event appends one line to a shared [`HookLog`], which the conformance probes
//! read to prove each fires exactly once per write, with the right before and after.

use std::sync::{Arc, Mutex};

use backbone_core::violation::Violation;
use backbone_core::write_guard::{GuardOutcome, WriteCtx, WriteGuard, WriteKind};
use backbone_core::{ServiceLifecycle, ServiceResult};
use backbone_messaging::crud_event::{CrudEvent, CrudEventPublisher};
use backbone_messaging::EventError;

use crate::domain::entity::Project;
use crate::infrastructure::persistence::ProjectRepository;

/// What the hooks and the event publisher saw, in order.
#[derive(Clone, Default)]
pub struct HookLog(Arc<Mutex<Vec<String>>>);

impl HookLog {
    /// Every line recorded so far.
    pub fn entries(&self) -> Vec<String> {
        self.0.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Forget what was recorded.
    pub fn clear(&self) {
        if let Ok(mut l) = self.0.lock() {
            l.clear();
        }
    }

    fn push(&self, line: String) {
        if let Ok(mut l) = self.0.lock() {
            l.push(line);
        }
    }
}

/// The lifecycle hooks: each records itself with the row it saw.
pub struct ProjectHooks {
    log: HookLog,
}

#[async_trait::async_trait]
impl ServiceLifecycle<Project> for ProjectHooks {
    async fn before_create(&self, entity: &mut Project) -> ServiceResult<()> {
        self.log.push(format!("before_create:{}", entity.code));
        Ok(())
    }
    async fn after_create(&self, entity: &Project) -> ServiceResult<()> {
        self.log.push(format!("after_create:{}:{}", entity.code, entity.id));
        Ok(())
    }
    async fn before_update(&self, entity: &mut Project) -> ServiceResult<()> {
        self.log.push(format!("before_update:{}:{}", entity.code, entity.name));
        Ok(())
    }
    async fn after_update(&self, entity: &Project) -> ServiceResult<()> {
        self.log.push(format!("after_update:{}:{}", entity.code, entity.name));
        Ok(())
    }
    async fn before_delete(&self, entity: &Project) -> ServiceResult<()> {
        self.log.push(format!("before_delete:{}", entity.code));
        Ok(())
    }
    async fn after_delete(&self, id: &str) -> ServiceResult<()> {
        self.log.push(format!("after_delete:{id}"));
        Ok(())
    }
    async fn before_restore(&self, entity: &Project) -> ServiceResult<()> {
        self.log.push(format!("before_restore:{}", entity.code));
        Ok(())
    }
    async fn before_hard_delete(&self, entity: &Project) -> ServiceResult<()> {
        self.log.push(format!("before_hard_delete:{}", entity.code));
        Ok(())
    }
}

/// The event publisher: records each event with what it carries.
pub struct ProjectEvents {
    log: HookLog,
}

#[async_trait::async_trait]
impl CrudEventPublisher<Project> for ProjectEvents {
    async fn publish(&self, event: CrudEvent<Project>) -> Result<(), EventError> {
        let line = match &event {
            CrudEvent::Created { entity, .. } => format!("event:created:{}", entity.code),
            CrudEvent::Updated { before, after, .. } => format!("event:updated:{}->{}", before.name, after.name),
            CrudEvent::Patched { before, after, .. } => format!("event:patched:{}->{}", before.name, after.name),
            CrudEvent::SoftDeleted { entity, .. } => format!("event:soft_deleted:{}", entity.code),
            CrudEvent::Restored { entity, .. } => format!("event:restored:{}", entity.code),
            CrudEvent::HardDeleted { entity_id, .. } => format!("event:hard_deleted:{entity_id}"),
            CrudEvent::BulkCreated { entities, .. } => format!("event:bulk_created:{}", entities.len()),
        };
        self.log.push(line);
        Ok(())
    }
}

/// A hand-written check the schema cannot express: a project name may not contain the word
/// "forbidden". It refuses with code `name_forbidden` on the `name` path.
pub struct ProjectCustomRules;

#[async_trait::async_trait]
impl WriteGuard<Project> for ProjectCustomRules {
    async fn check(&self, ctx: &WriteCtx<'_, Project>) -> GuardOutcome {
        let mut outcome = GuardOutcome::allow();
        if matches!(ctx.kind, WriteKind::Create | WriteKind::Update) {
            if let Some(after) = ctx.after {
                if after.name.to_lowercase().contains("forbidden") {
                    outcome.refuse.push(
                        Violation::new("name", "name_forbidden", "a project name may not say forbidden")
                            .with_param("word", "forbidden"),
                    );
                }
            }
        }
        outcome
    }
}

/// The schema's guard and the hand-written check, together.
pub struct ProjectGuards;

#[async_trait::async_trait]
impl WriteGuard<Project> for ProjectGuards {
    async fn check(&self, ctx: &WriteCtx<'_, Project>) -> GuardOutcome {
        super::ProjectRowRules.check(ctx).await.merge(ProjectCustomRules.check(ctx).await)
    }
}

/// A `ProjectService` with the hooks, the event publisher and both guards attached.
pub fn build_hooked_project_service(repository: Arc<ProjectRepository>, log: HookLog) -> super::ProjectService {
    super::ProjectService::new(
        repository,
        Arc::new(ProjectHooks { log: log.clone() }),
        Arc::new(ProjectEvents { log }),
    )
    .with_guard(Arc::new(ProjectGuards))
}
