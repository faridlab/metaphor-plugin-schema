//! The module's extension (hand-written; ADR-0031).
//!
//! It builds a second `Project` service carrying lifecycle hooks, an event publisher and a
//! hand-written check (`project_service.ext.rs`), and mounts it with the custom ticket routes
//! (`ticket_handler.ext.rs`) as [`ConformanceModule::extension_routes`].

use std::sync::Arc;

use axum::Router;
use backbone_core::http::BackboneCrudHandler;

use crate::application::service::{build_hooked_project_service, HookLog, IncidentService, ProjectService, TicketService};
use crate::domain::entity::Project;
use crate::presentation::dto::{CreateProjectDto, ProjectResponseDto, UpdateProjectDto};
use crate::ModuleParts;

/// State the module adds; read through the generated module's `Deref`.
pub struct ModuleExt {
    pub(crate) hook_log: HookLog,
    pub(crate) hooked_project_service: Arc<ProjectService>,
    pub(crate) ticket_service_ext: Arc<TicketService>,
    pub(crate) incident_service_ext: Arc<IncidentService>,
}

/// State the builder adds.
#[derive(Default)]
pub struct ModuleBuilderExt {}

impl ModuleBuilderExt {
    /// The build hook: called by the generated `build()` once every generated service exists.
    pub(crate) fn build(self, parts: &ModuleParts<'_>) -> anyhow::Result<ModuleExt> {
        let hook_log = HookLog::default();
        // The incident triggers' hand half writes through the service: bind it, weakly.
        crate::application::triggers::bind_incident_service(parts.incident_service);
        Ok(ModuleExt {
            hooked_project_service: Arc::new(build_hooked_project_service(
                parts.project_repository.clone(),
                hook_log.clone(),
            )),
            hook_log,
            ticket_service_ext: parts.ticket_service.clone(),
            incident_service_ext: parts.incident_service.clone(),
        })
    }
}

impl crate::ThisModule {
    /// What the project hooks and events recorded.
    pub fn hook_log(&self) -> HookLog {
        self.hook_log.clone()
    }

    /// The extension's routes: the hooked project service's generic routes at
    /// `/hooked-projects`, and the custom ticket routes.
    pub fn extension_routes(&self) -> Router {
        BackboneCrudHandler::<ProjectService, Project, CreateProjectDto, UpdateProjectDto, ProjectResponseDto>::routes_guarded(
            self.hooked_project_service.clone(),
            "/hooked-projects",
            crate::presentation::http::project_permissions(),
        )
        .merge(crate::presentation::http::create_ticket_extension_routes(self.ticket_service_ext.clone()))
        .merge(crate::presentation::http::create_incident_extension_routes(self.incident_service_ext.clone()))
    }
}
