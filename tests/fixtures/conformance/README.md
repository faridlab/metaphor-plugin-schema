# Conformance fixture

Driven by `tests/conformance.rs`. It proves, at runtime, that the features the schema DSL
declares do what the handbook says, through a module this crate's generator writes. The
latest run's matrix is in [`RESULTS.md`](RESULTS.md); the test rewrites it on every run.

## Running it

```bash
CONFORMANCE_DATABASE_URL=postgresql://root:password@localhost:5434/postgres \
CARGO_TARGET_DIR=$HOME/startapp/.cargo-target \
    cargo test --test conformance -- --ignored --nocapture
```

`CONFORMANCE_DATABASE_URL` is an admin DSN whose role may `CREATE DATABASE` and `CREATE ROLE`.
The run creates `conformance_<random>` and a login role `conformance_app_<random>` (NOSUPERUSER,
NOBYPASSRLS), and drops both at the end, also when a probe panics. It touches nothing else.

The identity module's tables (`public.permissions`, `public.roles`, `public.role_permissions`,
`public.user_roles`, `public.user_permissions`, `sapiens.organization_users`,
`sapiens.direct_permission_grants`) are created in the scratch database before the module's migrations
run, as a composing service has them from the identity module, so the module's seed migration writes
into them and the framework's `IdentityGrantSource` reads them.

The probes of area D also drive the outbox: the module's tables are created with its generated
`declared::migrate_outbox` (as a composing service does at startup), staged rows are read back as the
owner, and the relay and inbox functions of `backbone-outbox` run against them.

The framework crates come from `../modules/backbone-framework` through `[patch.crates-io]` when
that checkout exists, or from `METAPHOR_FRAMEWORK_PATH`; set it to `crates.io` to test the
released crates. `CONFORMANCE_KEEP=1` keeps the generated module and prints where it is.
`CONFORMANCE_JOBS` sets cargo's `-j` for the generated module's build (default 2).

The test fails when any probe fails, so it can gate CI once the failures it reports are fixed.

## What happens

1. The module skeleton (`../modules/backbone-module`) is copied; its example entity is removed
   and `module/` is laid over it. The copy is committed and regenerated with this checkout's
   generator.
2. `module/tests/conformance_runtime.rs` runs inside the generated module. It builds the scratch
   database from the module's own migrations (as the owner), adds a minimal org spine
   (`organization.org_units` and its two helpers, as the organization module provides them),
   grants the app role the way `rls_app_role.sql` does, and connects as that role.
3. The module's routes (`all_crud_routes`, `transition_routes`, and the extension's
   `extension_routes`) are mounted behind `tenant_route` → `org_auth` (a verifier bound to the
   tenant) → a bridge publishing the `AuthContext` the generated routes read. Every probe drives
   this router in-process with `tower::ServiceExt::oneshot` and records PASS, FAIL or UNSUPPORTED
   with the request, the expectation, the answer and a diagnosis. The bridge is the default
   stack, the host's own `AuthContext` with no grant resolver installed (`admin-*` holds `*:*`,
   `reader-*` holds `ticket:read`, anyone else nothing), so every generic-route probe of areas A to H
   runs fail-closed. Area I mounts three more stacks over the same routes: one behind the framework's
   `resolve_grants` reading scratch identity tables, one with nothing producing an `AuthContext`, and
   one behind a gate that attaches `PermissionWaiver`.
4. Back in `tests/conformance.rs`: the rules register and the doctor are checked against the
   runtime answers, and a few variants of the fixture are generated (not built) to probe layouts
   the main fixture cannot carry without breaking the build.

## The fixture

| File | Exercises |
|---|---|
| `module/schema/models/index.model.yaml` | module schema, `company_fence: strict`, `org_fence: strict`, the extension layout (`generators.extensions: true`), and the `authorization:` block: two custom ticket verbs (`escalate`, `archive_with_reason`) and two system roles, `ADMIN` (`*:*`) and `viewer` (`ticket:read` and the dotted, plural `projects.read`) |
| `module/schema/models/project.model.yaml` | `Project`, company-fenced (`company_id`): string, `@email`/`@url` attributes on strings, `email?` and `url?` types, decimal, money, int with `@range` and a literal default, int64 with `@min`/`@max`, bool default, date, datetime, json, enum default, `@unique`, `@immutable`, `@private`, `@exclude_from_dto`, `@length(min,max)`, `@pattern`, `@non_negative`, `@positive`, audit metadata and soft delete |
| `module/schema/models/ticket.model.yaml` | `Ticket`, org-fenced (`org_unit_id`): a foreign key to `Project`, a state field, a `projection` lifecycle field, nullable decimals and text, `@default(uuid)` on a non-id field, a composite `indexes: unique` |
| `module/schema/models/label.model.yaml` | `Label`, public reference data declared `public: true`: its reads need no permission and no login where the host mounts `public_routes()` outside its auth layer, its writes still need `label:<action>`; it has no company or org column, so it carries no fence |
| `module/schema/models/incident.model.yaml` | `Incident`, company-fenced: a `severity` int, a `status` machine, fields (`sla`, `handled_by`) that a trigger fills, carrying the hook triggers |
| `module/schema/models/approval.model.yaml` | `Approval`, company-fenced, a `hand_set` state bound to a machine |
| `module/schema/hooks/project.hook.yaml` | a declared uniqueness rule (`count(...) == 0` → coded partial unique index) and a row rule |
| `module/schema/hooks/ticket.hook.yaml` | the `Ticket.status` machine (a final state, a guarded edge, a `from: "*"` edge, `verbs: generated`) and one rule per register label: GUARD, SHADOW, VERB (`enforced_by`), UNENFORCED; declared `events:`: five staged by the generated write hooks (`on: create`, `transition(resolve)` with an `if:`, `enter(archived)`, `update(title)`, `delete`) and one staged by hand (`TicketEscalated`) |
| `module/schema/hooks/incident.hook.yaml` | `Incident.status` machine (verbs generated, `on_enter` / `on_exit` lists), declared events, and the `triggers:`: `before_create` and a `timing:` + `condition:` entry that `set:`, `after_create` (`log`, `emit:`, `notify`, `send_email`, and two hand actions), `after_update_status`, `after_update_status_resolved`, `after_update_title` with an `if:` over `$old`, `after_reopen` (a machine verb with a hand action), `before_delete` / `after_delete`, `after_page` (named for no write: hand-fired), and `sweep_stale` (a `schedule:` entry) |
| `module/schema/hooks/approval_state.hook.yaml` | the `Approval.state` machine (`verbs: generated`) over the hand_set field |
| `module/schema/workflows/incident_escalation.workflow.yaml` | `executor: engine`: an event starts it, a condition pages (a bespoke verb, with a declared function in its parameters, and a `send_email`), a wait for the `IncidentAcknowledged` event the Incident write hooks stage (keyed by incident, 15 minutes), a state-machine transition on the answer, an update and a refused transition on the timeout, an `emit_event`, a terminal `result`, a 7 day deadline |
| `module/schema/workflows/incident_checklist.workflow.yaml` | `executor: engine`: opens a follow-up incident through the generated service, runs a verb that can fail (by title: `boom` stages a row then breaks, `refuse` is refused, `linger` never returns), and compensates by retiring the follow-up through the service |
| `module/schema/workflows/ticket_triage.workflow.yaml` | a saga with `implemented_by` naming the hand function that runs it (`ticket_handler.ext.rs::escalate`): documented, never run by the engine |
| `module/schema/workflows/ticket_escalation.workflow.yaml` | a workflow with no `executor` and no `implemented_by`: documentation only, generates nothing, so nothing runs it |
| `module/schema/rules.baseline.yaml` | the rule ratchet, accepting the one deliberately unenforced rule |
| `module/src/application/service/project_service.ext.rs` | lifecycle hooks (`ServiceLifecycle`), an event publisher, a hand-written `WriteGuard` check composed with the generated one, and hand-written `WriteHooks` that stage an event and can fail after staging it |
| `module/src/application/workflows/incident_escalation_workflow.ext.rs`, `incident_checklist_workflow.ext.rs` | the hand halves of the two engine workflows: `page_responder` (stages a row on the step's transaction) and `on_call_name` (the declared function), and `risky_step`, whose behaviour the incident's title chooses |
| `module/src/application/triggers/incident_triggers.ext.rs` | `IncidentTriggersImpl`, the hand half of the triggers: a refusal after the hooks staged their rows, a follow-up written through the generated service (nesting), two rows staged on the write's transaction, the scheduled method, and the service handle a build step binds |
| `module/src/presentation/http/incident_handler.ext.rs` | `POST /incidents/:id/page`, a hand verb that fires the hand-fired trigger with `fire_after_page` in its own transaction |
| `module/src/application/service/ticket_service.ext.rs` | a custom service method (`archive_with_reason`), the function the VERB rule names |
| `module/src/presentation/http/ticket_handler.ext.rs` | custom routes, each wrapped in `guarded(..)`: the custom method over HTTP (`ticket:archive_with_reason`), and an `escalate` flow (`ticket:escalate`) that reads the org scope, moves the ticket through the generic service and stages the hand-declared `TicketEscalated` in the same transaction |
| `module/src/lib.ext.rs` | builds the hooked project service and mounts `extension_routes()`, the hooked service's generic routes through `routes_guarded` |
| `module/src/**/mod.ext.rs` | the re-exports the extension files need |
| `module/metaphor.codegen.yaml` | declares the runtime probe file `user_owned` |
| `module/tests/conformance_runtime.rs` | the runtime probes (areas A–I) |

## Permissions (area I)

The probes assert, for every generic verb (list, get, count, aggregate, history, trash, create, bulk
create, upsert, put, patch, bulk put, bulk patch, bulk preview, delete, bulk delete, restore, bulk
restore, restore all, purge, bulk purge, empty trash), every generated transition and every custom
verb, that a caller who holds only another permission is refused `403` with the violation
`permission_required` and `params.permission` naming exactly what the route requires, before the body is
read and before a record is looked up. Each of those is paired with the fail-closed default: with no
resolver installed, a caller whose `AuthContext` holds nothing is refused the same way, and a request
with no `AuthContext` is `401`. They also drive wildcards (`ticket:*`, `*:read`, `*:*`), authority by
role name, a tenant role created as rows after the service started (granting at the next request, and
revoked the same way), the legacy spellings and their deprecation signal, an identity store that cannot
be read (`503`, no table name in the body), a gate that waives the check for the requests it admits,
and the seed migration: every permission and the system roles present, a re-run that changes nothing,
and a tenant-edited role grant surviving a re-run. Generator variants prove that an unguarded hand
route, a direct call of the unguarded generic routes, and a permission the vocabulary lacks fail
`schema doctor`, that the register and the doctor label the mode, and that `requires:` renames a
permission or is refused when it names an undeclared one.

## Public routes (area I)

`Label` declares `public: true`. The probes mount every route of the module with no authentication layer
at all (only the tenant router) and the module's `public_routes()` alone, and assert that the public
reads answer `200` with no login (list, one record, count) and for a logged-in caller who holds nothing,
that the same model's writes are `401` with no authentication and `403` naming `label:<action>` for a
caller who holds nothing, that its non-public siblings in the same mount are `401`, that
`public_routes()` mounts no other entity, that the tenant router in front of it still refuses an unknown
tenant, and that the seed holds `label:create` and not `label:read`. Variants prove the register label
(`PERMISSION PUBLIC (read)`), that a public write is a doctor warning, that `public:` on a model with a
company fence fails generation unless it says `public_scoped: true`, and that a malformed or
contradictory declaration is refused.

## Hook triggers (area D)

`Incident` carries the triggers. The probes assert that a `before_create` `set:` is persisted and that a
trigger keyed by a label with `timing:` and a `condition:` runs after it and only when it holds; that
`after_create` stages what its compiled actions say (an `emit:` payload built from its arguments, a
`NotificationRequested` per `notify` and `send_email`) with the request's correlation id and the `log()`
line carrying it, leaving out a field the write cannot know (`$context`); that `after_update_status`
fires when a transition changes the state (not for a write that leaves it alone), with `$old`, that the
value-suffixed `after_update_status_resolved` fires only into `resolved`, and that `on_enter` and
`on_exit` fire on a transition into and out of a state; that a verb trigger (`after_reopen`) runs its
hand method; that a hand method returning an error refuses the write with its code and takes down
everything the hooks staged before it; that triggers writing through the generated service nest and
commit together up to depth 8, and the run past it is refused (`trigger_depth_exceeded`) and rolls the
chain back; that a hand-fired trigger runs through `fire_after_page`; that a soft delete fires
`before_delete` and `after_delete`; and that a `schedule:` entry registers a job whose method the
generated runner calls. Variants prove that generation refuses an `emit:` of an undeclared event, a guard
the rule language cannot compile, a key naming no field, and hand methods in a module without the
extension layout; that `legacy_triggers: true` keeps the old behaviour; that the doctor names a missing
extension file or method and warns on a client called inside the transaction; and that the build refuses
an implementation lacking a required method.

## Workflows run by the engine (area F)

`IncidentEscalation` and `IncidentChecklist` run on `backbone-core`'s workflow engine; the probes deliver
events to the module's `background_tasks()` as the outbox relay would (the real `IncidentAcknowledged` row
the write hooks staged is taken from the outbox and handed over) and call the engine's sweep as the scheduled
job `workflow_timer_sweep` would. They assert that an event starts one instance and a replay of it starts
none; that the instance runs to its wait and stops, with each step logged and the start event's correlation
id kept; that a condition takes its branch, a bespoke verb runs through the extension inside the step's
transaction, and a declared function evaluates in an expression; that a wait ignores an event whose key does
not match, resumes on the matching one (the acknowledgement of the incident through its real transition
route), and does nothing when the same event is delivered again; that a wait expires through the sweep only
when its due time has passed, and an instance past its deadline ends `expired`; that entity actions go
through the generated services (the machine's `after_update_status_resolved` trigger staged its event),
a transition step uses the state machine and its refusal (resolve from open) takes the failure branch and
leaves the record alone; that a step failing after it wrote leaves none of its writes, a transient failure
stores its backoff and is tried three times, then the instance fails and compensates through the service,
and compensating again changes nothing; that a refusal the caller can fix is not retried; that a run cut
inside a step's transaction leaves the instance on that step and the sweep resumes it without repeating the
completed one; and that instances are fenced by company: an event of company B never resumes company A's
instance, company B's scope cannot read it, the app role with no scope sees nothing, and the sweep still
serves company A. Variants prove that a function call in a condition (unless listed under `functions:`) and a
wait condition that is not an equality key fail generation, that `executor` absent generates nothing, that
the doctor names a missing extension file or method and a documented saga naming a function that does not
exist, that the register labels `WORKFLOW RUNS`, `WORKFLOW DOCUMENTS` and `WORKFLOW DOCUMENTED`, and that
the build refuses an implementation lacking a required method.

## What it cannot cover

- Row history of an `audited: true` model: the capture trigger and the history provider come
  from backbone-auditlog, which a standalone module does not compose.
- The ADR-0029 acting-unit decorator: it is installed by a composing service's `tenancy.yaml`
  (`metaphor schema tenancy`), not declared in a module.
- Delivery to a real broker or in-process bus: the relay is driven with test sinks (success, a
  failing publish, an exhausted one); `relay::drain_once`, `reemit` and `inbox::once` are the
  framework's own and run against the scratch database.
- The relay and the scheduler: the probes hand events to `background_tasks().deliver` and call
  `timer_sweep` themselves. The composing service wires the first into its relay and runs the second as the
  job `workflow_timer_sweep`, every minute; neither is a module concern.
- A consumer of the `NotificationRequested` a workflow stages (`send_email`, `notify`): the probes read the
  row from the outbox, as for the triggers' requests.
- A consumer of `NotificationRequested`: no mail or notification module consumes the row yet, so the
  probes read it from the outbox table.

## Adding a probe

Add it to the area function in `module/tests/conformance_runtime.rs` with `rec.check(...)`,
giving the request, what should happen, and where to look when it does not. A probe that needs a
schema the main fixture cannot carry goes in `variant_probes` in `tests/conformance.rs`.
