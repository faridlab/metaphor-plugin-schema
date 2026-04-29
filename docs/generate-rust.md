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
- PascalCase conversion for all type names

### `sql` -- PostgreSQL Migrations

Generates SQL migration scripts with:
- `CREATE TABLE IF NOT EXISTS` statements
- All field constraints (NOT NULL, UNIQUE, DEFAULT)
- Foreign key constraints with ON DELETE/UPDATE actions
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
- Authentication middleware integration

### `grpc` -- gRPC Services

Generates Tonic gRPC services with:
- Unary RPCs for CRUD operations
- Server streaming for list operations
- Request/response message mapping
- Error code mapping

### `openapi` -- OpenAPI Specifications

Generates OpenAPI 3.0 specs with:
- Path definitions for all endpoints
- Schema definitions for all models
- Request/response body schemas
- Use `--split` flag to generate one file per entity

### `dto` -- Data Transfer Objects

Generates request/response DTOs with:
- Create, Update, and Response variants
- Field filtering (omit auto-generated fields from Create DTOs)
- Nested DTO support for relations

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
