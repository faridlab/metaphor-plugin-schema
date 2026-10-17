// The extension of this directory's generated `mod.rs` (hand-written; ADR-0031). It is pulled
// in with `include!`, so these lines resolve against this directory.
pub use project_service::{build_hooked_project_service, HookLog};
pub use ticket_service::TicketServiceExt;
