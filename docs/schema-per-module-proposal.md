# Proposal: per-module Postgres schema scoping

Status: draft / RFC
Owner: (tbd)
Motivation: isolate each module's tables into its own Postgres schema
(`sapiens.*`, `bucket.*`, …) instead of one flat `public` namespace, to remove
cross-module table-name collisions and make modules portable across projects.

## Problem

All generated tables land in `public`. Two modules that both declare a
collection named `notification_preferences` with different shapes therefore
collide on one physical table. In `bersihir-service` this was patched by hand
(a `sapiens` schema + a `search_path` pool + reconciliation migrations) — work
that has to be redone in every consumer that composes the same modules.

We want this to be a first-class, generated property of a model, so the ORM
constant and the migration DDL stay in lockstep automatically.

## Why it is cheap here

The physical table name has exactly one source of truth:

```rust
// src/ast/model.rs:46
pub fn collection_name(&self) -> String {
    self.collection.clone().unwrap_or_else(|| to_snake_case_plural(&self.name))
}
```

Every consumer of the table name flows through it:

- `src/generators/repository.rs:166` — `pub const TABLE_NAME: &str = "{table_name}";`
- `src/generators/sql.rs:48,56` — `CREATE TABLE IF NOT EXISTS {table_name}`
- `src/generators/sql.rs:229–231,270` — audit trigger / function / index names

And `backbone_orm` interpolates the table name **raw** into SQL
(`query_builder.rs:157` → `format!("SELECT {} FROM {}", …, self.table)`), so a
schema-qualified value like `sapiens.notification_preferences` produces valid
`FROM sapiens.notification_preferences` for the **generated CRUD layer**.

So for generated code the feature reduces to: teach the model about a schema, and
qualify the table name at the points that reference the *table* — while keeping
the *bare* name for derived identifiers (index/trigger/function names cannot
contain a dot and are already schema-scoped to their table by Postgres).

### Caveat — codegen does NOT cover hand-written raw SQL

Modules also carry large amounts of **hand-written raw SQL** with table names as
string literals, which `collection_name()` does not feed and codegen therefore
cannot re-qualify:

- backbone-sapiens: ~170 raw query sites across ~71 files (auth_service, session/
  token/user repositories, event_store, user_query, …)
- backbone-bucket: ~30 sites across ~15 files

Hand-qualifying these (and re-doing it every release) is not viable. The
practical resolution is to set the connection pool's `search_path` to
`<module_schema>, public` — supplied **consumer-side** via `.with_database(pool)`,
so no crate change is needed. With `search_path` set, module-private tables
resolve in the module schema and shared kernel tables fall back to `public`.

**Therefore `search_path` is required, not optional.** Codegen schema-qualification
*complements* it (it puts tables in the right schema and qualifies generated FK
targets); it does not replace it. A module with any hand-written unqualified SQL
cannot rely on qualification alone.

### Per-model, not per-module: the kernel-table entanglement

A module may *own* tables that are cross-cuttingly referenced by other modules —
e.g. sapiens owns `users` / `roles` / `user_roles`, which consumer tables FK into.
Moving those into `sapiens.*` reintroduces cross-schema FKs. So schema assignment
must be **per model**: keep shared identity tables in `public`, scope only the
module's *private* tables. The `@schema(...)` per-model override below is what
makes this possible; expect a kernel-vs-private triage of each module's models.

## DSL surface

Two levels, file-default + per-model override:

```yaml
# schema/models/notification_preference.model.yaml
schema: sapiens                 # file-level default for every model in this file

model NotificationPreference {
  # @schema("sapiens")          # optional per-model override of the file default
  collection "sapiens_notification_preferences"
  ...
}
```

A **module-level** default is also supported in `index.model.yaml`:

```yaml
# schema/models/index.model.yaml
module: bucket
schema: bucket          # every model in the module defaults to this schema
```

Resolution order for a model's schema (most specific wins): per-model `schema:`
→ file-level `schema:` → module-level `index.model.yaml` `schema:` → none
(= `public`, current behaviour). So a whole module is scoped with one line, and
individual models opt out by setting their own `schema:` (e.g. `schema: ""` for
the kernel tables that must stay in `public`). Models with no schema anywhere are
unchanged, so cross-cutting kernel tables (`users`, …) stay in `public` and
everything keeps FK-ing into them.

## AST change

`src/ast/model.rs` — add one field and two helpers; leave `collection_name()`
returning the **bare** name so existing identifier-derivation code is untouched:

```rust
pub struct Model {
    pub name: String,
    pub collection: Option<String>,
    pub schema: Option<String>,     // NEW — resolved from @schema / file default
    ...
}

impl Model {
    /// Bare table name (unchanged). Used for index/trigger/function identifiers.
    pub fn collection_name(&self) -> String { /* as today */ }

    /// Schema-qualified name for use anywhere SQL references the *table*.
    pub fn qualified_table_name(&self) -> String {
        match &self.schema {
            Some(s) => format!("{s}.{}", self.collection_name()),
            None => self.collection_name(),
        }
    }
}
```

## Generator changes

| Location | Today | Change |
|---|---|---|
| `repository.rs:166` | `TABLE_NAME = "{collection_name}"` | `= "{qualified_table_name}"` |
| `sql.rs:56` | `CREATE TABLE … {collection_name}` | `… {qualified_table_name}` |
| `sql.rs:229–231` | trigger/func names from `collection_name` | **keep bare** (`collection_name`) |
| `sql.rs:270` | `idx_{collection_name}_…` | **keep bare** for the identifier; `ON {qualified_table_name}` |
| `sql.rs:551/1156` (FK) | `REFERENCES {target.collection_name}` | `REFERENCES {target.qualified_table_name}` |
| `seeder.rs:135/385` | seed `INSERT/SELECT/DELETE` use `model.collection` | `model.qualified_table_name()` (seeds always reference the table) |

Checked and intentionally left bare (not table references in the SQL sense):
- `integration_test.rs` uses the collection only for the REST endpoint path
  (`/api/v1/{collection}`) — a URL, must NOT be schema-qualified.
- `event_store.rs` uses fixed infra table names (`domain_events`,
  `aggregate_snapshots`, runtime-overridable via `with_table_name`), not per-model
  collections — out of scope for per-model schema scoping.

Plus migration prelude: when any model in the module has a schema, emit
`CREATE SCHEMA IF NOT EXISTS {schema};` ahead of the `CREATE TABLE`s (mirrors the
existing enum-types prelude block in `sql.rs:762–779`).

## Cross-schema FK ordering

A FK whose target lives in another schema requires that schema's table to exist
first. The generator already has a deferred-FK / cycle-back-edge pass
(`sql.rs:933` "Deferred foreign keys for … module"); cross-schema references
should route through the same deferral so ordering is handled the same way as
intra-module cycles. Recommendation: **only allow hard FKs module→kernel**
(`public`); references between two module schemas stay logical (an indexed
`*_id` column, integrity in the app layer) to preserve module independence.

## Migration / compatibility notes

- Pure additive: models without `schema:` generate exactly as today (golden-path
  diff stays small — verify against the "5 files changed" regen baseline).
- Moving an existing table into a schema is a breaking DDL change for live DBs
  (`ALTER TABLE … SET SCHEMA`). For `bersihir-service` this is a non-issue: it is
  pre-prod, so regenerate clean rather than writing move-migrations.
- Portability caveat: a hardcoded schema name means two instances of the same
  module cannot coexist in one database. If schema-per-tenant is ever a goal, the
  schema must be config-driven (env/`application.yml`) rather than baked into the
  generated constant — worth leaving the resolver pluggable now even if it
  defaults to a fixed literal.

## Implementation status (Phase A — landed)

The `schema:` DSL key is implemented and tested in this crate:

- AST: `Model.schema` + `Model::qualified_table_name()` + `Model::audit_function_name()` (`src/ast/model.rs`).
- Parse: `schema:` on `YamlModel` (per-model) and `YamlModelSchema` (file-level
  default); the default is applied wherever models are converted
  (`converters.rs::into_models*`, `parser/mod.rs` wrappers). Per-model wins; an
  explicit empty string means public.
- Generators: `CREATE SCHEMA IF NOT EXISTS`, schema-qualified `CREATE TABLE` /
  index `ON` / audit-trigger `ON` / `ALTER TABLE` / down `DROP TABLE`, audit
  function qualified into the table's schema; index & trigger *names* stay bare.
  Same-module FK targets qualified via `get_relation_target_table_qualified`;
  cross-module (`ModuleRef`) targets stay bare. Repository `TABLE_NAME` qualified.
- Standalone audit-triggers generator: the `audit-triggers` target
  (`AuditTriggersGenerator`, a separate "add triggers to already-migrated tables"
  migration) is now schema-aware too — qualified function + `ON` target, bare
  trigger names — so it agrees with the inline `CREATE TABLE` triggers. (Fixed in
  0.2.25; previously it emitted bare names that collided in `public` / wouldn't
  resolve under the migration-time `public` search path.)
- Tests: 7 added (5 in `sql.rs`, 1 parser, 1 audit-triggers); full lib suite
  green (341).

### Internal shape (0.2.25)

- Schema-default precedence is filled by one helper, `Model::apply_schema_default`
  (per-model wins; `schema: ""` counts as set → `public`), called from the
  parser (`converters.rs`, `parser/mod.rs`) and `module_loader.rs` — no more
  hand-rolled `if schema.is_none()` copies.
- The bare and qualified FK-target resolvers share one
  `resolve_relation_target_table(…, qualify)`; only the `Custom` arm differs.

### Known limitations / follow-ups (not Phase A)

1. **M2M join tables are not schema-scoped** — `generate_join_tables` still emits
   bare names and bare `REFERENCES`. A join table referencing a schema-scoped
   table would emit a bare reference that only resolves if the target is on the
   migration-runner search path. Revisit when a module with M2M into a scoped
   table appears.
2. ~~Cross-module + `identity` kernel needs a migration-runner search_path.~~
   **RESOLVED by decision (2026-06-20): the shared kernel stays in `public`.** FK
   constraints are created at *migration* time under the default `public`
   search_path, so a bare cross-module `REFERENCES users` resolves trivially when
   `users` lives in `public`. Keeping the kernel in `public` (rather than a
   dedicated `identity` schema) avoids any migration-runner / DB `search_path`
   change. Each module still owns a private schema for its non-kernel tables;
   `public` is the shared-kernel namespace. (If a future kernel table ever moves
   out of `public`, this limitation returns and the runner would need the schema
   on its search_path.)
3. **Migration-diff snapshot is single-schema.** The snapshot built from the
   models (`migration_cmd/snapshot.rs`) keys tables by the *bare*
   `collection_name()` and `TableSnapshot` carries no schema, so two same-named
   tables in different schemas (e.g. `sapiens.notifications` vs
   `public.notifications`) collapse to one key and the diff can't track a scoped
   table. For schema-scoped modules use the regenerate-clean workflow rather than
   `migration` diffing against a live DB. Revisit if diff-based migration becomes
   the path for scoped modules.
4. **Legacy `.model.schema` files don't inherit the module-level default.** The
   `.model.schema` branch in `module_loader.rs` parses via the legacy DSL and
   does not apply `module_schema_default`, so a model in that format stays in
   `public` even when `index.model.yaml` sets a module `schema:`. All current
   modules use `*.model.yaml`; only relevant if a module mixes formats.
5. **Schema identifiers are not quoted or validated.** `qualified_table_name()` /
   `audit_function_name()` and the inline `CREATE SCHEMA` interpolate the schema
   string raw, so a reserved word or non-snake_case name would emit invalid DDL
   that a non-stopping runner swallows. Use plain snake_case schema names; add a
   parse-time check if user-supplied schemas ever become untrusted.

## Out of scope (already handled)

The `REFERENCES sapiens.users` silent-failure bug is independent and already
fixed on `metaphor-plugin-schema` HEAD — see the assertions in `sql.rs` tests
(`REFERENCES users` expected, `REFERENCES sapiens.users` forbidden, ~lines
1651–1713). This proposal does not touch kernel-table references; those stay
unqualified in `public`.
