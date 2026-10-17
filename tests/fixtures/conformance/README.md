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
   tenant) → a bridge publishing the `AuthContext` the generated transition handlers read. Every
   probe drives this router in-process with `tower::ServiceExt::oneshot` and records PASS, FAIL
   or UNSUPPORTED with the request, the expectation, the answer and a diagnosis.
4. Back in `tests/conformance.rs`: the rules register and the doctor are checked against the
   runtime answers, and a few variants of the fixture are generated (not built) to probe layouts
   the main fixture cannot carry without breaking the build.

## The fixture

| File | Exercises |
|---|---|
| `module/schema/models/index.model.yaml` | module schema, `company_fence: strict`, `org_fence: strict`, the extension layout (`generators.extensions: true`) |
| `module/schema/models/project.model.yaml` | `Project`, company-fenced (`company_id`): string, `@email`/`@url` attributes on strings, `email?` and `url?` types, decimal, money, int with `@range` and a literal default, int64 with `@min`/`@max`, bool default, date, datetime, json, enum default, `@unique`, `@immutable`, `@private`, `@exclude_from_dto`, `@length(min,max)`, `@pattern`, `@non_negative`, `@positive`, audit metadata and soft delete |
| `module/schema/models/ticket.model.yaml` | `Ticket`, org-fenced (`org_unit_id`): a foreign key to `Project`, a state field, a `projection` lifecycle field, nullable decimals and text, `@default(uuid)` on a non-id field, a composite `indexes: unique` |
| `module/schema/models/approval.model.yaml` | `Approval`, company-fenced, a `hand_set` state bound to a machine |
| `module/schema/hooks/project.hook.yaml` | a declared uniqueness rule (`count(...) == 0` → coded partial unique index) and a row rule |
| `module/schema/hooks/ticket.hook.yaml` | the `Ticket.status` machine (a final state, a guarded edge, a `from: "*"` edge, `verbs: generated`) and one rule per register label: GUARD, SHADOW, VERB (`enforced_by`), UNENFORCED; declared `events:` |
| `module/schema/hooks/approval_state.hook.yaml` | the `Approval.state` machine (`verbs: generated`) over the hand_set field |
| `module/schema/workflows/ticket_escalation.workflow.yaml` | a workflow declaration, to prove it is declaration-only |
| `module/schema/rules.baseline.yaml` | the rule ratchet, accepting the one deliberately unenforced rule |
| `module/src/application/service/project_service.ext.rs` | lifecycle hooks (`ServiceLifecycle`), an event publisher, a hand-written `WriteGuard` check composed with the generated one |
| `module/src/application/service/ticket_service.ext.rs` | a custom service method (`archive_with_reason`), the function the VERB rule names |
| `module/src/presentation/http/ticket_handler.ext.rs` | custom routes: the custom method over HTTP, and an `escalate` flow that reads the org scope and moves the ticket through the generic service |
| `module/src/lib.ext.rs` | builds the hooked project service and mounts `extension_routes()` |
| `module/src/**/mod.ext.rs` | the re-exports the extension files need |
| `module/metaphor.codegen.yaml` | declares the runtime probe file `user_owned` |
| `module/tests/conformance_runtime.rs` | the runtime probes (areas A–H) |

## What it cannot cover

- Row history of an `audited: true` model: the capture trigger and the history provider come
  from backbone-auditlog, which a standalone module does not compose.
- The ADR-0029 acting-unit decorator: it is installed by a composing service's `tenancy.yaml`
  (`metaphor schema tenancy`), not declared in a module.
- The `text` field type: the validator refuses it as unknown, although the skeleton's
  `docs/schema/TYPES.md` lists it; `string` already maps to `TEXT`.
- Outbox staging: the generator emits none, and the generic service publishes outside the
  write's transaction.

## Adding a probe

Add it to the area function in `module/tests/conformance_runtime.rs` with `rec.check(...)`,
giving the request, what should happen, and where to look when it does not. A probe that needs a
schema the main fixture cannot carry goes in `variant_probes` in `tests/conformance.rs`.
