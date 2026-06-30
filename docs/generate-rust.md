# Rust Server-Side Code Generation

Deep-dive into the `metaphor schema generate` (and `generate:rust`) pipeline. This is the primary code generation target, producing server-side Rust code across 38 generation targets organized by architectural layer.

## Quick Start

```bash
# From inside a workspace project dir, MODULE auto-detects from CWD
metaphor schema generate

# Same thing with the alias
metaphor schema generate:rust

# Explicit MODULE — workspace project name
metaphor schema generate sapiens

# Or schema `module:` value (resolves to the same project)
metaphor schema generate bucket

# Specific targets only
metaphor schema generate --target rust,sql,repository,handler

# Preview without writing
metaphor schema generate --dry-run
```

---

## How MODULE Resolves

Inside a Metaphor workspace (a directory tree containing `metaphor.yaml`), the resolver tries, in order:

1. **Auto-detect from CWD** — when MODULE is omitted, walks up from CWD until it matches a `metaphor.yaml` project's `path:`. Errors with the available project list if no match.
2. **Workspace project name** — e.g. `bersihir-service`, `backbone-sapiens`. Resolves to that project's `schema/` directory.
3. **Schema `module:` value** — e.g. `bersihir`, `sapiens`, `bucket`. Read from each project's `schema/models/index.model.yaml` and matched.
4. **Legacy candidate paths** (kept for backwards compatibility outside workspaces) — `libs/modules/<MODULE>/schema`, `libs/modules/<MODULE>`, `modules/<MODULE>/schema`, `modules/<MODULE>`, then the literal arg as a direct path.

Outside a workspace, only step 4 applies.

> **Note** — Rust generate is single-module: it does not fan out to transitive `external_imports` / `depends_on` dependencies. Run the command per module if you need to regenerate dependent modules.

---

## Generation Targets

### Data Layer (5 targets)

| Target | Aliases | Description |
|--------|---------|-------------|
| `proto` | `protobuf` | Protocol Buffer v3 definitions (`.proto` files) |
| `rust` | - | Rust structs, enums, and entity implementations |
| `sql` | `migration`, `migrations` | PostgreSQL CREATE TABLE migrations |
| `repository` | `repo` | Repository implementations (database queries) |
| `repository-trait` | `repo-trait`, `repository_trait` | Repository trait definitions (interfaces) |

### Business Logic Layer (12 targets)

| Target | Aliases | Description |
|--------|---------|-------------|
| `service` | `services`, `svc` | Application service layer |
| `domain-service` | `domain_service`, `domain-svc` | Domain services with dependencies |
| `usecase` | `usecases`, `use-case`, `use_case`, `interactor`, `interactors` | Clean Architecture use cases |
| `auth` | `authentication`, `authorization` | Authentication and authorization logic |
| `events` | `domain-events`, `messaging` | Domain event handling |
| `state-machine` | `statemachine`, `sm` | State machine implementations |
| `validator` | `validation` | Validation logic |
| `specification` | `spec`, `specifications` | Business rule specifications |
| `cqrs` | `command`, `commands`, `query`, `queries` | CQRS command/query implementations |
| `computed` | `computed-fields`, `computed_fields`, `virtual` | Computed field logic |
| `bulk-operations` | `bulk_operations`, `bulk`, `batch` | Bulk/batch operation endpoints |
| `seeder` | `seeders`, `seed`, `seeds` | Database seeder scripts |

> Note: `permission` is planned but not yet implemented.

### API Layer (4 targets)

| Target | Aliases | Description |
|--------|---------|-------------|
| `handler` | `handlers`, `rest` | Axum REST handlers with CRUD endpoints |
| `grpc` | `tonic` | Tonic gRPC services with streaming support |
| `graphql` | `gql` | GraphQL schema and resolvers |
| `openapi` | `swagger` | OpenAPI 3.0 specifications |

### Infrastructure Layer (14 targets)

| Target | Aliases | Description |
|--------|---------|-------------|
| `trigger` | `triggers` | Event/trigger handlers |
| `workflow` | `workflows`, `flow`, `flows`, `saga`, `orchestration` | Workflow orchestration (Saga pattern) |
| `module` | `mod`, `lib` | Module-level code (`mod.rs`, re-exports) |
| `config` | `configuration`, `settings` | Configuration code |
| `value-object` | `value_object`, `vo` | Value object definitions |
| `projection` | `projections`, `read-model`, `read_model` | CQRS read model projections |
| `event-store` | `event_store`, `eventstore` | Event sourcing store |
| `export` | `exports`, `public-api` | Public API exports |
| `integration` | `acl`, `anti-corruption` | Integration adapters (Anti-Corruption Layer) |
| `event-subscription` | `event_subscription`, `subscription`, `subscriptions` | Event subscription handlers |
| `dto` | `dtos`, `data-transfer`, `transfer-objects` | Data Transfer Objects |
| `versioning` | `version`, `api-version`, `api-versioning` | API versioning |
| `integration-test` | `integration_test`, `test`, `tests` | Integration test scaffolding |
| `audit-triggers` | `audit_triggers`, `audit` | Database audit trigger functions |

### Framework Compliance (3 targets)

| Target | Aliases | Description |
|--------|---------|-------------|
| `app-state` | `app_state`, `appstate` | Application state struct |
| `routes-composer` | `routes_composer`, `routes` | Routes composition (Axum router) |
| `handlers-module` | `handlers_module` | Handlers module declarations |

### Meta Target

| Target | Description |
|--------|-------------|
| `all` | Generate all targets (default) |

---

## Generation Batch Ordering

Generators run in **3 sequential batches** to avoid race conditions where later generators depend on output from earlier ones:

1. **Batch 1 -- Data Layer**: `proto`, `rust`, `sql`, `repository`, `repository-trait`
2. **Batch 2 -- Business Logic**: `service`, `domain-service`, `usecase`, `auth`, `events`, `state-machine`, `validator`, `specification`, `cqrs`, `computed`, `bulk-operations`, `seeder`
3. **Batch 3 -- API & Infrastructure**: `handler`, `grpc`, `graphql`, `openapi`, `dto`, `trigger`, `workflow`, `module`, `config`, `value-object`, `projection`, `event-store`, `export`, `integration`, `event-subscription`, `versioning`, `integration-test`, `audit-triggers`, `app-state`, `routes-composer`, `handlers-module`

Within each batch, generators run independently.

---

## Generators Configuration

You can control which generators run at the schema file level using the `generators` section in `*.model.yaml`:

### Whitelist Mode

Only run the listed generators:

```yaml
generators:
  enabled: [rust, sql, repository]
```

### Blacklist Mode

Run all generators except the listed ones:

```yaml
generators:
  disabled: [cqrs, projection, event-store]
```

### Per-Target Opt-In

Enable or disable individual targets:

```yaml
generators:
  cqrs: true
  projection: true
  event-store: false
```

> **`grpc` / `graphql` flow into module wiring.** When `grpc` or
> `graphql` is disabled, the `module` target also drops the matching
> router-mount / service-registration / `MergedObject!` entries from
> the generated `lib.rs`, so the module compiles without dangling
> references to a transport that wasn't generated.

---

## Output Directory Structure

Generated code is placed in the module's source directory:

```
libs/modules/{module}/
  └── src/
      ├── entity/          # Rust structs and enums
      ├── repository/      # Repository implementations
      ├── service/         # Application services
      ├── domain/          # Domain services, value objects, specifications
      ├── handler/         # REST handlers
      ├── grpc/            # gRPC service implementations
      ├── dto/             # Data Transfer Objects
      ├── event/           # Events, event store, subscriptions
      ├── workflow/        # Workflow orchestration
      ├── migration/       # SQL migrations
      ├── proto/           # Protocol Buffer definitions
      ├── config/          # Configuration
      │   ├── mod.rs       #   hand-written re-exports (preserved across regen)
      │   └── generated.rs #   the `ModuleConfig` struct + loaders (regen target)
      ├── validator/       # Validation logic
      ├── auth/            # Authentication/authorization
      ├── cqrs/            # CQRS commands and queries
      └── mod.rs           # Module declarations
```

---

## Target Details

### `proto` -- Protocol Buffers

Generates Protocol Buffer v3 `.proto` files with:
- Message definitions for each model
- Enum definitions with `UNSPECIFIED = 0` sentinel
- Field numbering and type mapping
- `buf.validate` validation rules
- Google well-known type imports (`Timestamp`, etc.)
- Package naming based on module path

### `rust` -- Rust Structs

The largest generator. Produces:
- Struct definitions with `Serialize`, `Deserialize`, `Clone` derives
- Strongly-typed ID newtypes (e.g., `UserId(Uuid)`)
- `Entity` trait implementation with `id()`, `is_new()`, timestamp accessors
- Status enum checker methods (e.g., `is_active()`, `is_deleted()`)
- Soft-delete field detection and methods
- Hashed field detection (for password fields)
- Audit metadata JSONB field support
- State machine support in entity structs
- Field-level security via `EntityRepoMeta::private_fields()` / `owner_field()` overrides, emitted from `@private` and `@owner` field attributes (camelCase response keys; pruned by backbone-core's `apply_field_security`)
- `EntityRepoMeta::relations()` override emitted from `@one` relations that carry a `@foreign_key`, returning `&[(relation_name, target_table, local_fk)]` so handlers can expand sibling records via `?include=` (relation name and FK as camelCase response keys; target table from the target model's collection; only emitted when includable to-one relations exist)
- PascalCase conversion for all type names

### `sql` -- PostgreSQL Migrations

Generates SQL migration scripts with:
- `CREATE TABLE IF NOT EXISTS` statements
- All field constraints (NOT NULL, UNIQUE, DEFAULT)
- Foreign key constraints with ON DELETE/UPDATE actions. Cross-module
  references (e.g. `@foreign_key(sapiens.User.id)`) emit the bare, conventionally
  derived target table name (`REFERENCES users`), **not** a schema-qualified
  `REFERENCES sapiens.users`. The module name is not a Postgres schema — modules
  are composed into a single schema (`public`) — so the unqualified reference
  resolves via the search path. (A qualified name would point at a non-existent
  schema and the `ADD CONSTRAINT` would fail silently, leaving the FK uncreated.)
- GIN indexes for JSONB audit metadata
- Composite and single-field indexes
- Partial indexes via `@where(...)` (audit-metadata keys auto-rewritten to JSONB form)
- Automatic timestamp trigger functions
- CHECK constraints for JSONB structure validation

### `repository` -- Repository Implementations

Generates database access code with:
- CRUD operations (create, find_by_id, update, delete)
- List/paginate with filtering
- Soft-delete aware queries (trash, restore)
- Foreign key relationship loading
- Batch operations
- Transaction support

### `handler` -- REST Handlers

Generates Axum REST handlers with:
- Standard CRUD endpoints (GET, POST, PUT, DELETE)
- List with pagination and filtering
- Soft-delete endpoints (trash, restore) when enabled
- Bulk operation endpoints
- Request/response DTOs
- Error handling
- Authentication middleware integration via
  `backbone_auth::middleware::AuthContext` (an external crate, not a generated
  module) extracted from `Extension<AuthContext>` (the bearer token is read from
  the `Authorization` header inside the handler). All async dependencies
  carry `Send + Sync` bounds so handlers compose with Axum's `Router`.

### `grpc` -- gRPC Services

Generates Tonic gRPC services with:
- Unary RPCs for CRUD operations
- Server streaming for list operations
- Atomic batch RPCs — `BulkDelete`, `BulkRestore`, `RestoreAll`,
  `BulkPermanentDelete`, `BulkUpdate`, `BulkPatch` — with shared
  `Bulk{Model}Response` (affected entities) and `BulkMutate{Model}Response`
  (affected-row count) messages
- Request/response message mapping
- Error code mapping

### `graphql` -- GraphQL Schema & Resolvers

Generates `async-graphql` schema objects and resolvers with:
- One `Query` and one `Mutation` resolver per entity
- Field selection via `async-graphql` derive macros on the entity DTOs
- **`soft_delete` mutations** when the model implements `SoftDeletable`
  — the generator emits `soft_delete<Entity>` (sets `deleted_at`) and
  `restore<Entity>` instead of a hard `delete<Entity>`. Hard `delete`
  is only emitted for models without soft-delete.
- **Atomic batch mutations** — `{module}_bulk_delete_*`,
  `{module}_bulk_restore_*`, `{module}_restore_all_*`, and
  `{module}_bulk_permanent_delete_*` for id-based batch operations.
- **Deduped `Service` import** in the resolver module so re-exported
  services aren't pulled in twice when a module has multiple entities.
- **Schema suffix on merged roots** — the per-module merged roots are
  always named `<…>SchemaQuery` / `<…>SchemaMutation`, even when the
  module name already ends in `Query`/`Mutation`. This lets the host
  service combine modules with `MergedObject!((SchemaQuery, …))`
  without colliding on the unsuffixed `Query`/`Mutation` types that
  individual entity resolvers expose.

### `openapi` -- OpenAPI Specifications

Generates OpenAPI 3.0 specs with:
- Path definitions for all endpoints
- Schema definitions for all models
- Request/response body schemas
- Atomic batch paths — `PUT`/`PATCH` on the collection (`bulkUpdate`/
  `bulkPatch`) plus `/delete/bulk`, `/restore/bulk`, `/restore/all`,
  `/trash/bulk` — backed by a shared `BatchIds` request body and a
  `{Model}BulkResult` schema
- Use `--split` flag to generate one file per entity

> **Feature-gated imports** — `use utoipa::ToSchema` lines emitted into
> `entity/` and `dto/` modules are wrapped in `#[cfg(feature = "openapi")]`,
> so a module that opts out of the `openapi` cargo feature still compiles
> without pulling `utoipa` into its dependency graph.

> **Schema placement** — per-model schemas (`{Model}`, `Create{Model}Request`,
> `{Model}List`, etc.) are written under `components.schemas`, so every
> `#/components/schemas/{Model}` `$ref` resolves. They are emitted *before* the
> common `parameters`/`responses`/`requestBodies` sections; writing them after
> would nest them under `requestBodies` and leave every entity ref dangling in
> Swagger UI (fixed in 0.2.22).

### `dto` -- Data Transfer Objects

Generates request/response DTOs with:
- Create, Update, and Response variants
- Field filtering (omit auto-generated fields from Create DTOs)
- Nested DTO support for relations
- `use utoipa::ToSchema` imports are emitted under
  `#[cfg(feature = "openapi")]` so DTO modules compile without the
  `openapi` feature enabled.
- **Nullable response fields serialize as explicit `null` by default.**
  A nullable field whose value is `None` is rendered as `"field": null`
  (present, not omitted), so typed clients see a stable response shape.
  Opt a field out with the `@omit_if_none` attribute, which restores
  `#[serde(skip_serializing_if = "Option::is_none")]` for that field. The
  same rule governs the entity serializer in [`rust`](src/generators/rust.rs).
- **Request DTOs accept snake_case input alongside the canonical camelCase
  wire name.** `Create`/`Update`/`Patch{Entity}Dto` keep
  `#[serde(rename_all = "camelCase")]` (so `providerId` remains the documented
  format), but every multi-word field also gets
  `#[serde(alias = "<snake_name>")]`, letting clients send `provider_id` or
  `providerId` interchangeably. The alias is emitted only when the snake and
  camel forms differ (i.e. multi-word fields). Response DTOs are unchanged —
  output stays camelCase — so this is non-breaking.
  [`dto`](src/generators/dto.rs).

### `config` -- Module Configuration

Generates a per-module configuration struct.

- Written to **`src/config/generated.rs`** (so a hand-written
  `src/config/mod.rs` can re-export it without being clobbered on regen).
- The struct is named **`ModuleConfig`** (renamed from the previous
  `Config` to avoid collisions when consumers re-export multiple
  modules into one `use crate::config::Config` namespace).
- Includes loader helpers that read env vars + `application.yml`.

### `module` -- Module Wiring (`lib.rs` / `mod.rs`)

Generates the module's public re-exports and the `with_database` /
service-registration plumbing used by the host backend service.

- **Respects `generators.disabled`** — when `grpc` or `graphql` is
  disabled for the module, the corresponding wiring (router mount, gRPC
  service registration, GraphQL root merge) is omitted from the generated
  `lib.rs` rather than emitted-and-broken.
- **Raises the macro recursion limit.** The generated `lib.rs` carries a
  crate-level `#![recursion_limit = "1024"]` inner attribute. Generated
  domain policies contain deeply nested `serde_json::json!{}` literals
  that overflow Rust's default limit of 128 when the crate is compiled as
  a **library** (e.g. linked by integration tests under `tests/**`); the
  bin crate sets the same attribute in its hand-written `main.rs`.
- **Suppresses import-level noise crate-wide.** The generated `lib.rs`
  also carries `#![allow(unused_imports)]`. Each per-file generator emits a
  uniform import block and not every file uses every import, so this avoids
  widespread `unused_imports` warnings on freshly generated crates rather
  than pruning per generator. Real unused imports in hand-written
  `// <<< CUSTOM` blocks and `*_custom.rs` are still flagged by clippy.
- **Only declares modules whose files are emitted.** `pub mod` declarations
  in the generated `lib.rs` / `mod.rs` are gated on the same conditions that
  write the corresponding files, so the crate never references an absent
  module. In particular `permission` (domain) and `middleware` (application)
  are not declared while their generators remain unimplemented.
- Inserts an empty `// <<< CUSTOM` placeholder immediately after the
  `with_database(...)` call so consumers have a stable, merge-safe slot
  for extra setup (custom indexes, seed bootstraps, etc.).
- Merged GraphQL roots are exported as **`SchemaQuery`** /
  **`SchemaMutation`** (the `Schema` suffix is always appended, even when
  the module name already ends in `Query`/`Mutation`), so the host
  service can `MergedObject!((SchemaQuery, …))` without name clashes.

---

## Preserving Custom Code on Regeneration

Rust generators are **merge-aware**. Files containing `// <<< CUSTOM`
markers (or paired `// <<< CUSTOM METHODS START >>>` /
`// <<< CUSTOM METHODS END >>>` blocks) are merged with the freshly
generated output instead of being overwritten.

### Single-line marker

```rust
// File: libs/modules/sapiens/src/lib.rs
pub mod entity;
pub mod handler;

// <<< CUSTOM
pub mod my_extra_module;
// END CUSTOM
```

On regeneration, the merger:

1. Re-emits the canonical module contents.
2. Scans the existing file for `// <<< CUSTOM` blocks, capturing each
   block plus its **anchor line** (the nearest preceding non-empty,
   non-marker line).
3. Re-inserts each block into the regenerated content at the matching
   anchor.

### Inside-container vs after-container heuristic

The same anchor can mean *"keep me inside this struct/enum/use-list"* or
*"keep me below this module item"*. The merger looks at the **first
real content line of the CUSTOM block** to decide:

| Block content begins with                                                                  | Placement                          |
|--------------------------------------------------------------------------------------------|------------------------------------|
| A full statement (ends with `;`), or `pub mod`, `pub use`, `pub fn`, `impl`, `mod`, `use`, `fn`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `#[…]` | **After** any containing `};` (module / sibling scope) |
| A field-like fragment (e.g. `pub b: i32,` or an enum variant)                              | **Inside** the enclosing container |

This is why a `// <<< CUSTOM` block holding `pub mod admin;` survives a
regen of `lib.rs` while a `// <<< CUSTOM` block adding `Foo,` to a
`pub use crate::dto::{…};` list keeps its position inside the braces.

### Paired blocks

`// <<< CUSTOM METHODS START >>>` / `// <<< CUSTOM METHODS END >>>` are
preserved **verbatim** between the markers — no per-line filtering, no
content cap. Use them for impl-block extensions that can include nested
`{}` blocks (e.g. large `matches!` macros).

### What is merge-aware

| File pattern                              | Strategy                                                |
|-------------------------------------------|---------------------------------------------------------|
| `config/application*.yml`                 | YAML merge — user values (e.g. `database.url`) always win |
| `seed_order.yml`                          | Append new seeds to existing list                       |
| SQL seed files                            | Preserve custom data after the marker                   |
| Any `.rs` file with `// <<< CUSTOM`       | Block-aware Rust merge (anchor + placement heuristic)   |
| Migration files (`migrations/*.{up,down}.sql`) | Identity-based dedup (see below)                   |

### Migration dedup by identity

Migration filenames are timestamped on every regen
(`20260426220110_create_provider_staff_table.up.sql`), so a naive
`exists()` check would always miss and write a duplicate. The generator
treats any sibling under `migrations/` with the same
`_<identity>.{up,down}.sql` suffix — under **any** timestamp — as
"already migrated" and skips it (unless `--force` is passed). This keeps
re-running `metaphor schema generate` after a non-schema-shape change
idempotent.

---

## Practical Examples

In each example below, MODULE is omitted because it auto-detects from CWD when run inside a project directory. Pass MODULE explicitly (`metaphor schema generate sapiens --…`) when you want to target a different module.

### Generate Only Data Layer

```bash
metaphor schema generate --target proto,rust,sql,repository,repository-trait
```

### Generate for a Single Model

```bash
metaphor schema generate --models Customer --lenient
```

The `--lenient` flag is recommended with `--models` because filtered generation may have unresolvable cross-references.

### CI Pipeline: Changed Schemas Only

```bash
metaphor schema generate --changed --base main --validate
```

This:
1. Detects which `.model.yaml` / `.hook.yaml` / `.workflow.yaml` files changed since `main`
2. Generates code only for affected schemas
3. Runs `cargo check` to verify the generated code compiles

### Dry Run Preview

```bash
metaphor schema generate --dry-run
```

Shows all files that would be generated and their sizes without writing anything.

### Force Regenerate Everything

```bash
metaphor schema generate --force
```

Overwrites all existing generated files.

### Selective Hook and Workflow Generation

```bash
# Only generate for specific hooks
metaphor schema generate --hooks OrderHooks,CustomerHooks --lenient

# Only generate for specific workflows
metaphor schema generate --workflows OrderProcessing --lenient
```

### Target a Different Module From the Current Project

```bash
# Schema-module name
metaphor schema generate sapiens --target rust,sql

# Or workspace project name (resolves to the same place)
metaphor schema generate backbone-sapiens --target rust,sql
```
