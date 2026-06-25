# Changelog

All notable changes to `metaphor-plugin-schema` are documented here.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.3] — 2026-06-25

### Added

- **App-level `disabled_targets` in `metaphor.codegen.yaml` — skip Kotlin
  targets the consuming app hand-writes, across every module.** A mobile app
  that fully owns a target (e.g. `offline-repositories`, `sync`) can now opt out
  of generating it without touching the shared product schema or its read-only
  upstream module schemas. The Kotlin generator walks up from the resolved
  output (source root) to the nearest ancestor holding a `metaphor.codegen.yaml`,
  reads its `disabled_targets:` list, and removes those targets from the
  effective set — applied **after** `all` is expanded, so every module
  (including read-only transitive deps) honors the app's choice. Names are
  matched case-insensitively; unknown / non-Kotlin entries are ignored; a missing
  or unparseable file disables nothing. The skipped targets are echoed on each
  run. This complements the per-model `generators.disabled:` schema field: that
  gates one model for all consumers, while `disabled_targets` gates whole targets
  for one app across all models and modules.
  [`kotlin`](src/commands/kotlin.rs).

## [0.4.2] — 2026-06-24

### Changed

- **The `database` (per-entity SQLDelight `.sq`) target is no longer part of
  `all`.** Under the offline-first cache architecture, entities are persisted as
  JSON in a generic cache table (`CacheDao`), and the emitted `.sq` files land
  under `generated/sqldelight/` — which is not a SQLDelight source root — so they
  were never compiled. Dropping `database` from the default `all` set stops the
  generator from writing dead files on every run. The target is still available
  explicitly via `--target database`.
  [`config`](src/kotlin/config.rs).

## [0.4.1] — 2026-06-24

### Changed

- **Mapper generation now emits two files per entity, keeping each generated
  file under the 250-line size gate.** The DTO / FormData declarations
  (`<Entity>DTO`, the `<Entity>ListDTO` typealias, and `<Entity>FormData`) are
  split out of the mapper into a sibling `<Entity>DTO.kt` in the same
  `application/mappers/` package, so the mapper class in `<Entity>Mapper.kt`
  references them directly with no extra import. Wide entities that previously
  pushed the combined file over the limit now stay within it. `generate_mapper`
  returns the list of files it wrote (`Vec<PathBuf>`) instead of an
  `Option<PathBuf>`, and a new `mapper_dto` Handlebars template
  (`MAPPER_DTO_TEMPLATE`) backs the split.
  [`application`](src/kotlin/generators/application/mod.rs),
  [`templates`](src/kotlin/templates/mod.rs).
- **Compacted the navigation config and deep-link templates.** The `NavConfig`
  destinations, role-visibility companion, and `fromDeepLink` parser were
  condensed (one-line `data class` destinations, trimmed doc blocks) so the
  generated navigation files also stay under the size gate without changing
  behavior. [`templates`](src/kotlin/templates/mod.rs).

## [0.4.0] — 2026-06-24

### Changed

- **Generated Kotlin now lives under a module-first `<base>.generated`
  namespace, split from the hand-written framework.** Previously the generator
  emitted layer-first packages directly under the base
  (`com.bersihir.domain.sapiens`), interleaving generated files with
  hand-written ones. Generation now writes everything under a single
  generator-owned subtree — `<base>.generated.<module>.<layer>` (e.g.
  `com.bersihir.generated.sapiens.domain.entity`) — physically
  `kotlin/<base-path>/generated/…`. The constructor normalizes the package to
  exactly one trailing `.generated` segment (an already-suffixed value is
  accepted and not doubled), and a `metaphor.codegen.yaml` ownership manifest
  (`generated: ["**"]`) is dropped at the tree root so it's unmistakable that
  the whole subtree is overwritten on every regen. Hand-written code lives
  *outside* the tree (sibling packages like `<base>.core`,
  `<base>.infrastructure.di`) and customizes generated code via extension
  functions / subclasses / wrapper DTOs. A new `{{framework base_package}}`
  Handlebars helper (and `MobileGenerator::framework_package`) strips the
  trailing `.generated` so generated files still import the framework base
  classes (`core.*`, `OfflineFirstRepository`, `domain.types.*`, pagination
  contracts) from the true base package.
  [`mod`](src/kotlin/generators/mod.rs), [`templates`](src/kotlin/templates/mod.rs).
- **Module-first enum-import extraction.** With the namespace flipped to
  module-first, the entity generator derived the wrong module for enum imports
  (it took the second-to-last package segment, which is now the layer). It now
  strips the base prefix and takes the *first* segment after it, so an entity's
  `import <base>.generated.<module>.domain.enums.<Enum>` resolves to the
  entity's own module. [`entity`](src/kotlin/generators/domain/entity.rs).

### Added

- **Per-module `Metadata` typealias is now generated, making the tree
  self-contained.** Entities and mappers reference a `metadata: Metadata` audit
  field imported from `<base>.generated.<module>.domain.enums.Metadata`, but the
  typealias was never emitted (it isn't a schema enum), so the generated tree
  didn't compile on its own. The enums generator now emits one
  `Metadata.kt` per module (`typealias Metadata = Map<String, JsonElement?>`)
  into the module's enums package. [`enums`](src/kotlin/generators/domain/enums.rs).

### Fixed

- **Pagination meta now serializes as camelCase, matching the backend.** The
  generated `PaginatedApiResponse` / `PaginationMeta` carried
  `@SerialName("total_pages")` / `@SerialName("has_next")` / `@SerialName(
  "has_prev")`, expecting snake_case keys — but the backend serializes the whole
  response in camelCase (`totalPages`, `hasNext`, `hasPrev`). The `@SerialName`
  annotations (and the now-unused `kotlinx.serialization.SerialName` import) are
  dropped so the Kotlin property names match the wire keys directly; the
  generated API-client test fixtures were updated to camelCase to match.
  [`templates`](src/kotlin/templates/mod.rs).
- **Pagination types are no longer generated — they are framework contracts.**
  The generator emitted `PaginatedResult` / `PaginatedApiResponse` /
  `BackendPaginatedResponse` / `PaginationMeta` into each app's
  `infrastructure/pagination/`, guarded by a process-global `AtomicBool` so it
  only ran once per build. These types are consumed by the hand-written base
  `BaseCrudApiClient` / `OfflineFirstRepository`, so generating a second copy
  under `<base>.generated` split the type identity. The generator now stops
  emitting them entirely; repository, API-client, and offline-repository
  templates import them from the framework package via `{{framework
  base_package}}.infrastructure.pagination.…`.
  [`repository`](src/kotlin/generators/domain/repository.rs),
  [`templates`](src/kotlin/templates/mod.rs).

## [0.3.1] — 2026-06-21

### Fixed

- **Event-store generator now schema-qualifies its tables.** The repository,
  seeder, and migration generators already emit `schema.table` for modules that
  isolate their tables in a dedicated Postgres schema, but the event-store
  generator still emitted bare names: the `domain_events` / `aggregate_snapshots`
  table-name defaults and the hard-coded `projector_positions` references. When
  the module's schema isn't on the connection's `search_path`, generated event
  sourcing code failed with `relation "..." does not exist`. The generator now
  derives the module schema from the resolved models and prefixes all three, with
  bare names preserved for modules targeting `public`.

## [0.2.27] — 2026-06-20

### Added

- **`schema openapi-collect` — vendor composed modules' OpenAPI specs into a
  consumer app.** A consumer (typically a `backend-service`) composes several
  modules' routers but serves a single Swagger UI. Each module generates its own
  `schema/openapi/openapi.yaml`; this command copies them into the app so they
  can be embedded with `include_str!` and offered as additional Swagger specs. A
  copy (not a reference) is required because the service's build context is
  usually just the app directory — sibling `modules/` aren't reachable at build
  time.
  - Driven by an `openapi_vendor` section in the app's `metaphor.codegen.yaml`:
    `dest` (destination dir relative to the app root) and an optional `modules`
    list (defaults to the app's `depends_on` from `metaphor.yaml`).
  - Each spec lands at `<dest>/<short>.openapi.yaml`, where `<short>` strips any
    `backbone-` prefix (`backbone-sapiens` → `sapiens.openapi.yaml`). Modules
    missing a generated spec are skipped with a warning.
  - Run from the app directory (or pass the app name); rebuild the app afterward
    to embed the refreshed specs.

## [0.2.26] — 2026-06-20

### Fixed

- **Seeders ignored per-model schema, so seeds targeted the wrong table under a
  scoped module.** The seeder generator emitted bare `INSERT` / `SELECT` /
  `DELETE` against the table name, which resolved to `public` instead of the
  model's declared schema. Seed statements are now schema-qualified, matching the
  migration and repository generators. Unscoped models are unchanged.

## [0.2.25] — 2026-06-20

### Added

- **Per-module Postgres schema scoping.** Models can now declare a Postgres
  `schema:` so a module's tables live in their own namespace (`sapiens.*`,
  `bucket.*`) instead of a flat `public`, removing cross-module table-name
  collisions. Resolution order (most specific wins): per-model `schema:` →
  file-level `schema:` (top of a `*.model.yaml`) → module-level `schema:`
  (`index.model.yaml`) → none (= `public`, unchanged). An explicit empty string
  (`schema: ""`) pins a model to `public`, overriding any inherited default —
  used to keep shared kernel tables (`users`, RBAC) in `public` while the rest
  of the module is scoped.
  - `Model` gained `schema`, `qualified_table_name()`, and
    `audit_function_name()`.
  - The SQL generator emits `CREATE SCHEMA IF NOT EXISTS`, schema-qualified
    `CREATE TABLE` / index `ON` / audit-trigger `ON` / `ALTER TABLE` / down
    `DROP TABLE`, and qualifies the audit trigger function into the table's
    schema. Index and trigger *names* stay bare (Postgres scopes them to the
    table's schema automatically). Same-module FK `REFERENCES` targets are
    schema-qualified; cross-module references stay bare and resolve via the
    search path.
  - Repository `TABLE_NAME` is schema-qualified (`backbone_orm` interpolates it
    raw into `FROM`, so `schema.table` resolves directly).
  - Requires the consumer connection pool's `search_path` to include the module
    schema (`search_path = <module>, public`) for hand-written raw SQL.

### Fixed

- **The standalone audit-triggers generator ignored per-model schema, so its
  migration disagreed with the inline `CREATE TABLE` triggers.**
  `AuditTriggersGenerator` (the `audit-triggers` target, which emits a separate
  "add triggers to already-migrated tables" migration) still produced bare
  `CREATE FUNCTION <table>_audit_timestamp()` and `CREATE TRIGGER … ON <table>`.
  For a schema-scoped model that function name collided across schemas in
  `public`, and the `ON <table>` clause wouldn't resolve under the migration-time
  `public` search path. It now emits the schema-qualified function
  (`<schema>.<table>_audit_timestamp()`) and qualified `ON` targets while keeping
  trigger names bare, matching `SqlGenerator::generate_audit_triggers`. Unscoped
  models are unchanged.

### Internal

- Centralized the file/module schema-default fill into
  `Model::apply_schema_default`, replacing four hand-rolled `if schema.is_none()`
  copies across the parser (`converters.rs`, `parser/mod.rs`) and module loader.
- Collapsed the duplicated FK target resolver into a single
  `resolve_relation_target_table(…, qualify)` shared by the bare
  (`get_relation_target_table`) and qualified variants, so only the `Custom` arm
  differs.

### Known limitations

- Many-to-many join tables are not schema-scoped (tracked in the proposal).
- The migration-diff snapshot is single-schema (keyed by the bare table name);
  regenerate clean rather than diffing for schema-scoped modules.
- The legacy `.model.schema` format does not inherit the module-level `schema:`.
- Schema identifiers are interpolated without quoting/validation — use plain
  snake_case names.

## [0.2.24] — 2026-06-19

### Fixed

- **Cross-module foreign keys were schema-qualified with the module name
  (`REFERENCES sapiens.users`), so the constraint failed to create.** A relation
  whose target lives in another module (e.g. `@foreign_key(sapiens.User.id)`)
  emitted `REFERENCES <module>.<table>`, treating the module name as a Postgres
  schema. Modules are composed into a single schema (`public`), so that
  reference points at a schema that doesn't exist — and because migration
  runners typically don't stop on error, the `ALTER TABLE … ADD CONSTRAINT …`
  failed silently and the foreign key was never created. The SQL generator now
  emits the bare, conventionally-derived table name (`REFERENCES users`) for
  cross-module references, so the reference resolves via the search path.

## [0.2.23] — 2026-06-15

### Fixed

- **Single-entity API client methods returned the raw `{ success, data }`
  envelope instead of the bare entity, so detail/edit forms bound to envelope
  keys instead of real fields.** List endpoints were already unwrapped via
  `toPaginated`, but `getById`, `create`, `update`, `patch`, `upsert`,
  `getDeletedById`, and `restore` passed the response straight through `handle<T>`,
  leaking the server's `{ success, data }` wrapper. Added a `handleEntity<T>`
  helper to the shared runtime that unwraps the `{ success, data }` envelope
  (the list shape minus pagination), throwing `CrudApiError` when `success` is
  false, and tolerating an already-bare entity for forward-compat. All
  single-entity `BaseCrudApiClient` / `SoftDeleteCrudApiClient` methods now route
  through it, so callers receive the entity directly.

## [0.2.22] — 2026-06-08

### Fixed

- **OpenAPI generator emitted model schemas in the wrong place, leaving every
  `#/components/schemas/{Model}` $ref dangling.** `generate_spec` opened
  `components.schemas`, then called `write_common_schemas` (which closes
  `schemas:` and opens sibling `parameters:`/`responses:`/`requestBodies:`),
  and only *then* wrote the per-model schemas — so `Account`, `CreateAccountRequest`,
  `{Model}List`, etc. nested under `requestBodies` instead of `schemas`. Swagger UI
  reported "Could not resolve reference" for every entity. Fixed by writing the
  model schemas BEFORE the common sections, so all schema definitions sit under
  `components.schemas`. The generated spec now resolves with zero dangling refs.

## [0.2.21] — 2026-06-08

### Changed

- **Generated request DTOs now accept snake_case input in addition to the
  canonical camelCase wire name.** `Create`/`Update`/`Patch{Entity}Dto` keep
  `#[serde(rename_all = "camelCase")]` (so `providerId` stays the documented
  format) but each multi-word field also gets `#[serde(alias = "<snake_name>")]`,
  letting clients send `provider_id` or `providerId` interchangeably. Response
  DTOs are unchanged (output stays camelCase), so this is non-breaking. The alias
  is emitted only when the snake and camel forms differ (multi-word fields).

## [0.2.20] — 2026-06-07

### Changed

- **CUSTOM-block preservation now applies to every webapp writer, not just the
  TS schema files.** 0.2.19 preserved `// <<< CUSTOM … // END CUSTOM` blocks only
  in the `{Entity}.schema.ts` path; every other generated file still went through
  a plain `fs::write` and lost any hand-authored block content on regen. A new
  drop-in `preserve_and_write` helper wraps `preserve_custom_blocks` + `fs::write`,
  and every webgen writer (domain, application, infrastructure, presentation,
  contracts) now calls it in place of `fs::write`. Behaviour is unchanged for
  files that contain no CUSTOM markers — they are written through verbatim — so
  this is a safe, uniform substitution rather than a per-file opt-in.
  [`preserve_and_write`](src/webgen/custom_blocks.rs).

## [0.2.19] — 2026-06-07

### Added

- **`// <<< CUSTOM … // END CUSTOM` blocks are preserved across webapp regen.**
  The webapp generators emit these markers but write each file fresh, so any
  hand-authored content inside a block (e.g. a `listSchema` added to a generated
  `{Entity}.schema.ts`) was previously lost on the next `schema generate:webapp`.
  A new `preserve_custom_blocks` merge keeps the **generator's marker placement**
  and only substitutes each block's body, matched by its open-marker header line.
  This differs from the Rust `mod.rs` merge — which re-anchors single-line markers
  and would misfire on nested brace structures — and is correct for the TS schema
  files, where the block sits at a fixed, generator-controlled spot. A missing
  file or a file with no CUSTOM blocks passes through unchanged.
  [`custom_blocks`](src/webgen/custom_blocks.rs),
  [`entity_schema`](src/webgen/generators/domain/entity_schema.rs).

## [0.2.18] — 2026-06-07

### Added

- **`EntityRepoMeta::relations()` emitted from to-one relations.** Each `@one`
  relation that carries a `@foreign_key` and points at another model in the
  schema is collected into a generated `relations()` override returning
  `&[(relation_name, target_table, local_fk)]`. The relation name and local FK
  are emitted as camelCase response keys; the target table comes from the target
  model's collection name. This gives handlers the metadata to expand sibling
  records via `?include=`. The override is only emitted when the model actually
  declares includable to-one relations. The relation's `@foreign_key` is read
  locally (accepting both `String` and `Ident` argument spellings) so a bare FK
  name resolves here without widening the shared `Relation::foreign_key()` — which
  would change FK-constraint emission in migration generation.
  [`rust`](src/generators/rust.rs).

## [0.2.17] — 2026-06-06

### Added

- **Field-level security attributes `@private` and `@owner`.** A field tagged
  `@private` is collected into a generated `EntityRepoMeta::private_fields()`
  override, and a field tagged `@owner` becomes the `EntityRepoMeta::owner_field()`
  override. Both are emitted as camelCase response keys so they line up with the
  serialized body that backbone-core's `apply_field_security` prunes for
  non-owner/non-root callers. The overrides are only emitted when the model
  actually declares such fields (`private_fields()` when at least one `@private`
  field exists, `owner_field()` when an `@owner` column exists).
  [`rust`](src/generators/rust.rs).

## [0.2.16] — 2026-06-06

### Changed

- **Nullable response fields now serialize as explicit `null` by default.**
  Previously every optional field on a generated DTO/entity response carried
  `#[serde(skip_serializing_if = "Option::is_none")]`, so `None` values were
  omitted from the JSON body entirely. The default is now reversed: nullable
  fields serialize as present-with-`null`, giving typed clients a stable
  response shape. Opt back into omission per field with the new `@omit_if_none`
  attribute, for cases where absence is semantically meaningful. Applies to both
  the response DTO generator and the entity serializer.
  [`dto`](src/generators/dto.rs), [`rust`](src/generators/rust.rs).

## [0.2.15] — 2026-06-05

### Changed

- **Webgen hook parser now accepts both hook authoring forms.** `parse_content`
  tries the rich map-based form (`RawHookSchema`) first, and on failure falls
  back to the canonical `parse_hook_yaml_flexible` parser — which also accepts
  the list/sequence authoring form (`YamlHookSchema`). The canonical schema is
  converted into webgen's `HookSchema` via two new helpers (`from_canonical`,
  with `convert_canonical_state_machine` / `convert_canonical_permission`), and
  the existing map-based path is factored into `build_from_raw`. This keeps
  webgen aligned with the backend codegen: both now accept the same hook grammar,
  so any authored `*.hook.yaml` parses in every generator regardless of which
  spelling (map or list) it uses.
  [`hook`](src/webgen/parser/hook.rs).

## [0.2.14] — 2026-06-05

### Changed

- **Webgen trigger actions accept both the bare-string and struct spellings.**
  Entries under a trigger's `actions:` list were previously required to be plain
  strings (`- send_email(...)`); they now also deserialize from the struct form
  (`- action: foo` / `- type: foo`, optionally with `params`), matching how
  actions are authored elsewhere in `*.hook.yaml`. The raw form is an untagged
  `Simple | Detailed` enum (`RawTriggerAction`), and `name()` yields the action
  name regardless of which spelling was used. Extra keys are ignored by webgen
  but preserved in the schema source.
  [`state_machine`](src/webgen/ast/state_machine.rs),
  [`hook`](src/webgen/parser/hook.rs).

## [0.2.13] — 2026-06-05

### Changed

- **Webgen hook parser now mirrors the canonical permission/validation
  grammar.** The `*.hook.yaml` parser was carrying a narrower shape than the
  rest of the schema toolchain; it now accepts the same fields the canonical
  `YamlPermissionAction` grammar exposes:
  - **Permission rules (`allow:` / `deny:`).** Each entry may be either a bare
    action string (`- all`, `- read`) or a full struct with `if` (condition),
    `only` (restrict to these fields), and `except` (all fields except these).
    `PermissionRule` gains `only` and `except`; the raw form is now an untagged
    `Simple | Full` enum so both spellings deserialize.
    [`state_machine`](src/webgen/ast/state_machine.rs).
  - **Validation rules (`rules:`).** `code` is now optional and a new optional
    `severity` (`error`, `warning`) field is parsed — warnings commonly omit a
    code. The parser also keys each rule by its map name instead of dropping it,
    so `ValidationRule.name` is populated.
    [`hook`](src/webgen/parser/hook.rs).

### Fixed

- Validation rules parsed from `*.hook.yaml` no longer lose their name (the
  map key is now carried through to `ValidationRule.name`).

## [0.2.12] — 2026-06-04

### Added

- **Atomic batch mutations across every generated API surface.** Beyond the
  existing `bulkCreate`/`upsert`, the CRUD stack now emits a full set of
  id-based batch operations that run in a single atomic transaction:
  - `bulkDelete(ids)` — soft-delete many by id (`POST /delete/bulk`).
  - `bulkUpdate(items)` — full-update many, one `{ id } & Update` payload per
    item (`PUT /bulk`).
  - `bulkPatch(body)` — partial-update many, either a shared `{ ids, patch }`
    or per-id `{ items: [{ id, patch }] }` (`PATCH /bulk`).

  For soft-deletable entities the trash surface also gains:
  - `bulkRestore(ids)` — restore many by id (`POST /restore/bulk`).
  - `restoreAll()` — restore every soft-deleted row (`POST /restore/all`).
  - `bulkPermanentDelete(ids)` — purge many from trash by id
    (`DELETE /trash/bulk`).

  Operations that return affected rows resolve to a `BulkResult<T>` envelope
  (`items`, `total`, `failed`, `errors`); count-only operations return a small
  count object (`soft_deleted` / `restored` / `permanently_deleted`).

  Generated through every layer:
  - **Web (TS).** Wired through every layer, matching the 0.2.9
    bulk-create/upsert pattern. New `bulkDelete`/`bulkUpdate`/`bulkPatch` on
    `CrudService` and `BaseCrudApiClient`, with `bulkRestore`/`restoreAll`/
    `bulkPermanentDelete` on the soft-delete variants, plus the exported
    `BulkResult<T>` envelope. The `makeCrudUseCases`, `makeSoftDeleteUseCases`,
    and `makeCrudAppService` factories expose the same operations (with
    `BULK_DELETE_*` / `BULK_UPDATE_*` / `BULK_PATCH_*` / `BULK_RESTORE_*` /
    `RESTORE_ALL_*` / `BULK_PERMANENT_DELETE_*` error codes), and each entity
    now exports `bulkDelete{Entity}UseCase`, `bulkUpdate{Entity}UseCase`,
    `bulkPatch{Entity}UseCase` (and, for soft-delete entities,
    `bulkRestore{Entity}UseCase`, `restoreAll{Entity}UseCase`,
    `bulkPermanentDelete{Entity}UseCase`).
    [`shared_runtime`](src/webgen/generators/shared_runtime.rs),
    [`usecase`](src/webgen/generators/application/usecase.rs).
  - **OpenAPI.** `PUT`/`PATCH` on the collection plus `/delete/bulk`,
    `/restore/bulk`, `/restore/all`, `/trash/bulk` paths, a shared `BatchIds`
    request body, and a `{Model}BulkResult` schema; module-index `$ref`s wired
    for each batch path. [`openapi`](src/generators/openapi.rs).
  - **gRPC.** `BulkDelete`/`BulkRestore`/`RestoreAll`/`BulkPermanentDelete`/
    `BulkUpdate`/`BulkPatch` RPCs with shared `Bulk{Model}Response` and
    `BulkMutate{Model}Response` messages. [`grpc`](src/generators/grpc.rs).
  - **GraphQL.** `{module}_bulk_delete_*`, `{module}_bulk_restore_*`,
    `{module}_restore_all_*`, and `{module}_bulk_permanent_delete_*`
    mutations. [`graphql`](src/generators/graphql.rs).
  - **Kotlin.** `bulkDelete`/`bulkRestore`/`restoreAll`/`bulkPermanentDelete`
    on the repository interface and the offline repository (online-only; the
    cache is fully invalidated on success). [`templates`](src/kotlin/templates/mod.rs).
  - **Integration tests.** `test_bulk_delete`/`test_bulk_restore`/
    `test_restore_all` exercised against created entities in `run_all`.
    [`integration_test`](src/generators/integration_test.rs).

## [0.2.11] — 2026-06-04

### Fixed

- **List query params now alias camelCase keys to snake_case on the wire.** The
  generated TS API stays idiomatic camelCase (`sortBy`, `sortOrder`), but the
  backend list endpoints parse snake_case query params and treat any
  unrecognized key as a column filter — so a raw `sortOrder` landed in the
  filter map and produced `column "sortorder" does not exist`.
  [`buildQuery()`](src/webgen/generators/shared_runtime.rs) now maps
  `sortBy → sort_by` and `sortOrder → sort_order` via a `QUERY_KEY_ALIASES`
  table before appending; all other keys pass through unchanged.

## [0.2.10] — 2026-06-02

### Added

- **API-root mounting for the product module.** The generated REST clients now
  distinguish the app's *primary* (product) module from its dependency
  (backbone) modules. The primary module's collections mount at the API root —
  `/api/v1/{collection}` with no module segment — while backbone modules keep
  mounting under `/api/v1/{module}/{collection}`. In workspace "app" mode this
  is resolved automatically (the primary module is flagged as the API root);
  the single-module command keeps the module-segmented layout. Exposed on
  [`Config`](src/webgen/config.rs) as the `api_root` field and the
  `with_api_root(bool)` builder, and consumed by
  [`BaseCrudApiClient.basePath()`](src/webgen/generators/shared_runtime.rs) and
  the per-entity [`api_client`](src/webgen/generators/infrastructure/api_client.rs)
  generator (an empty `module` collapses the segment).

### Changed

- **`BaseCrudApiClient.basePath()` now builds `/api/v1` explicitly.** The base
  path is composed as `${API_BASE_URL}/api/${API_VERSION}{segment}/{collection}`,
  where `{segment}` is empty for the API-root (product) module and `/{module}`
  for backbone modules.

## [0.2.9] — 2026-06-02

### Added

- **Bulk-create and upsert across the whole CRUD stack.** The shared runtime now
  carries `bulkCreate(inputs)` (POST `/bulk`) and `upsert(input)` (POST `/upsert`)
  through every layer: `CrudService`/`CrudRepository` ports, `BaseCrudApiClient`,
  `BaseRepositoryImpl`, the `crudUseCases` factory (`BULK_CREATE_*` / `UPSERT_*`
  error codes), and `crudAppService`. Each entity now also exports
  `bulkCreate{Entity}UseCase` and `upsert{Entity}UseCase`. `createMany` is now
  implemented on top of `bulkCreate` (one round-trip instead of N parallel
  `create` calls). See
  [`shared_runtime`](src/webgen/generators/shared_runtime.rs) and
  [`usecase`](src/webgen/generators/application/usecase.rs).
- **Soft-delete auto-detection by Backbone convention.** Beyond an explicit
  `soft_delete: true`, an entity is now treated as soft-deletable when it carries
  an audit `metadata` field or a `deleted_at` field — matching the Backbone
  backend which exposes the trash endpoints (`listDeleted` / `restore` /
  `emptyTrash`) for those entities. See
  [`ModelParser`](src/webgen/parser/model.rs).

### Changed

- **Soft-delete use cases now mirror the backend trash surface.** The
  `makeSoftDeleteUseCases` factory drops the redundant `softDelete` (a soft
  delete is just the normal `delete`) and now emits `listDeleted`, `restore`,
  `emptyTrash`, and `permanentDelete`. Each entity exports
  `list{Entity}DeletedUseCase`, `restore{Entity}UseCase`,
  `emptyTrash{Entity}UseCase`, and `permanentDelete{Entity}UseCase`.
- **`emptyTrash` endpoint moved from `DELETE /trash` to `DELETE /empty`** to
  match the Backbone REST contract.

## [0.2.8] — 2026-06-02

### Added

- **Shared, framework-free CRUD runtime emitted once into `shared/`.** A new
  [`shared_runtime`](src/webgen/generators/shared_runtime.rs) module emits a set
  of generic bases — an injectable HTTP transport (`shared/http`), pagination
  types (`shared/types`), and the generic CRUD building blocks under
  `shared/crud/` (`CrudService`, `CrudRepository`, `BaseCrudApiClient`,
  `BaseRepositoryImpl`, `crudUseCases`, `crudAppService`) plus `shared/entity`
  helpers. The whole tree is pure TypeScript (no `zod`/React/framework imports),
  so the contracts purity guard still holds. The HTTP transport defaults to the
  global `fetch` and can be swapped once at startup via `setHttpRequest(...)`
  (e.g. to route through `ky` for shared auth/refresh). Emitted by the
  `ContractsGenerator` into `<output>/shared/`.

### Changed

- **Generated per-entity files are now thin wrappers over the shared runtime.**
  Domain entities/repositories/services, infrastructure API clients and
  repository impls, and application use-cases/app-services now extend or call the
  generic `shared/crud/` bases instead of repeating ~300 lines of identical CRUD
  per entity. Net effect across the generators: ~1700 lines of duplicated
  boilerplate removed in favor of the shared bases, with no change to the
  generated public surface.

## [0.2.7] — 2026-06-02

### Added

- **New `contracts` target — a pure, framework-free domain "genotype".** A
  deliberately slim subset of `domain` that emits *only* the framework-agnostic
  contracts every target shares: entity types, Zod schemas + inferred DTOs
  (Create/Update/Patch), enums, and repository **ports** — pure TypeScript whose
  sole external import is `zod`. No React Query hooks, MUI/Mantine forms, pages,
  use cases, or repository implementations. It is **opt-in only** (never included
  by `--target all`, to avoid colliding with the framework-coupled `domain`
  output) and is requested via `--target contracts` (aliases: `pure`,
  `genotype`). Intended for webapps that hand-write their own runtime *phenotype*
  (e.g. Mantine + TanStack Query) on top of the generated port. New
  [`ContractsGenerator`](src/webgen/generators/contracts/mod.rs) and
  `Target::Contracts` variant; wired through `Generator::generate_contracts_layer`.
- **Workspace "app" mode for `generate:webapp`.** Mirroring the kotlin/mobile
  generator, passing an **app name** to `--output` (a single path segment that
  resolves to a workspace app, or `apps/<name>/`) now resolves the app's
  `src/generated/` dir and the module set from `metaphor.yaml` (the primary
  module + its transitive `depends_on` / `external_imports`), then fans out —
  one command regenerates everything for an app, no per-app script. The primary
  module is auto-detected from the CWD project when omitted. Modules referenced
  as deps but absent from the workspace are skipped with a warning. See
  [`Workspace::webapp_output_for_app`](src/commands/workspace.rs) and
  [`webapp::run`](src/commands/webapp.rs).
- **`--schema-dir` flag** — point the generator at an explicit schema root
  (containing `models/`, `hooks/`) instead of the default
  `libs/modules/<module>/schema`, letting the logical module name stay clean
  (e.g. `bersihir`) while the schema lives elsewhere (e.g.
  `apps/bersihir-service/schema`). Backed by `Config::schema_dir_override` /
  `Config::with_schema_dir`.
- **`--import-alias` flag** (default `@/generated`) — the import root alias
  generated application/infrastructure code uses to reference the generated tree.
  Backed by `Config::import_root` / `Config::with_import_root`.
- **`--with-grpc` flag** (off by default) — also emit gRPC clients
  (nice-grpc-web); the REST API client is always generated. Backed by
  `Config::enable_grpc` / `Config::with_grpc`.

### Changed

- **`generate:webapp` default `--target` is now
  `contracts,application,infrastructure`** (the framework-free Clean Architecture
  stack) instead of `all`. The legacy MUI/hooks output (`domain`, `hooks`,
  `forms`, `pages`, `types`, `all`) is now opt-in.
- **`<MODULE>` is now optional** on `generate:webapp` — in workspace "app" mode
  it is auto-detected from the current project dir and fanned out across module
  deps.

## [0.2.6] — 2026-06-01

### Fixed

- **Kotlin mappers no longer emit a crash-prone `!!` for required-but-
  optional-on-form fields.** When a field is required by the entity
  (`!is_nullable`) but nullable on the generated `FormData`
  (`form_is_nullable`) — most commonly a required enum, whose form default is
  `null` — `toEntity` previously asserted it with a raw `formData.field!!`,
  which throws an **uncaught** `NullPointerException` and crashes the app when
  the user submits a partially-filled form. The mapper now emits
  `formData.field.required("field")`, a catchable, message-bearing validation
  helper the UI can surface. The `import {base}.core.mapper.required` is added
  only when at least one field needs it (gated by a new `needs_required` flag).
  See [`kotlin/generators/application/mod.rs`](src/kotlin/generators/application/mod.rs)
  and [`kotlin/templates/mod.rs`](src/kotlin/templates/mod.rs).

## [0.2.5] — 2026-06-01

### Fixed

- **Generated `lib.rs` now raises the macro recursion limit
  (RUST-GEN-001).** `generate_lib_rs` emits a crate-level
  `#![recursion_limit = "1024"]` inner attribute (mirroring the
  user-owned `main.rs`) immediately after the header doc-comments. Deeply
  nested `serde_json::json!{}` literals in generated domain policies
  exceed Rust's default limit of 128 when the crate is compiled as a
  **library** — which is what integration tests in `tests/**` link — so
  `cargo build` / `cargo check --bin` passed while `cargo test
  --test …` failed with *"recursion limit reached while expanding
  `$crate::json_internal!`"*. See
  [`module.rs::generate_lib_rs`](src/generators/module.rs).
- **`to_snake_case` keeps mixed-case acronyms intact.** `"OAuthProvider"`
  now converts to `oauth_provider` instead of `o_auth_provider`. `OAuth`
  is structurally identical to a CamelCase word (upper, upper, lower…),
  so it cannot be split correctly by casing rules alone; it is normalized
  via a small known-acronym table before conversion. Other acronyms
  (`MFADevice` → `mfa_device`, `HTTPRequest` → `http_request`, etc.) are
  unchanged. See [`webgen/parser/proto.rs`](src/webgen/parser/proto.rs).
- **Single-quoted attribute argument strings are unquoted.** The webapp
  model parser only treated double quotes as string delimiters, so
  `@default('value')` parsed to `'value'` (quotes retained). Both single-
  and double-quoted strings are now recognized, with the opening quote
  tracked so the matching closer ends the string. See
  [`webgen/parser/model.rs`](src/webgen/parser/model.rs).
- **Kotlin android `namespace` base-package detection covers more module
  suffixes.** `parse_android_namespace` previously stripped only
  `.shared` / `.android`, so `namespace = "com.example.mobile"` resolved
  to `com.example.mobile` instead of the base `com.example`. The
  recognized module/platform suffixes now also include `.mobile`, `.ios`,
  `.desktop`, `.web`, `.jvm`, `.js`, `.native`, and `.common`. See
  [`kotlin/package_detector.rs`](src/kotlin/package_detector.rs).

### Internal

- Refreshed generator unit/integration tests that had drifted from the
  current code-gen output (BackboneCrudHandler wiring, `GenericCrudRepository`
  newtype + `impl_crud_repository!` macro, trigger/registry type aliases,
  `from_state` state-machine construction, model-driven domain-policy and
  auth generators) and fixed the `resolve_package` doctest import. No
  change to generated output.

## [0.2.4] — 2026-05-25

### Fixed

- **Generator marker now lands in the first lines of enum-prelude
  migrations.** The `create_<table>_table.up.sql` template was emitting
  the `DO $$ … CREATE TYPE …` enum prelude *before* the
  `-- Generated by metaphor-schema` header. For tables with several
  enum-typed columns, the marker drifted past line 10 — outside the
  scan window used by [`is_generator_authored_migration`](src/commands/schema/migrations.rs#L32)
  — so the `migration_cleanup` sweep saw the file as hand-written and
  refused to delete its renamed predecessor on `generate -f`. The
  header is now written up front (above the enum prelude) and the
  duplicate header emitted by `generate_table` is stripped, keeping
  the marker reliably within the first three lines regardless of how
  many enums the model declares.

## [0.2.3] — 2026-05-25

### Added

- **`metaphor schema doctor`.** New read-only subcommand that scans every
  `.rs` file under the consumer root for references to handler-route
  symbols (`create_<name>_routes`, `create_<name>_read_routes`,
  `create_protected_<name>_routes`, …) that the generator won't emit
  because the model opted out via `config.generators.disabled: [handler,
  …]`. Findings are tagged `user-owned` (the caller must edit) vs
  `generator-managed` (next `generate -f` will heal). Exits non-zero on
  drift so it slots into CI. See
  [docs/cli-reference.md § `metaphor schema doctor`](docs/cli-reference.md#metaphor-schema-doctor).
- **Model-level `description:` field.** Optional free-form string on
  every model. Informational only — ignored by all generators today,
  but reserved so schemas can carry intent without tripping the new
  strict parser.

### Changed

- **Strict model parsing (`deny_unknown_fields`).** `YamlModel` and the
  per-model `config:` block now reject unknown keys at parse time. The
  common foot-gun this catches: writing `disabled: [handler]` at the
  top level of a model when it belongs under `config.generators.disabled`.
  Previously the misplaced key was silently dropped and only surfaced
  later as "why isn't per-model gating working?". Now the parse fails
  with `unknown field 'disabled', expected one of …` pointing at the
  offending model.
- **Parse errors expose the full anyhow chain.** Module loader now
  formats schema-file errors with `{:#}` so serde's inner message
  (e.g. the `unknown field` line above) surfaces instead of being
  hidden behind the generic top-level context.

## [0.2.2] — 2026-05-25

### Fixed

- **`--force` cleanup now sweeps paired `.down.sql` files.** Previously the
  stale-migration pass only considered `.up.sql` candidates, so when an up
  was deleted (or renumbered) its down survived as a phantom — eventually
  causing duplicate-sequence noise and broken rollbacks. The pass now
  treats `NNN_*.down.sql` as in-scope, keeps a down whenever its up was
  just generated, and skips the `-- Generated by metaphor-schema` header
  check for downs (they're paired with ups by construction and don't
  carry the marker). Hand-written up-files and `user_owned` matches are
  still preserved.
- **Module aggregator and routes-composer skip handler-disabled models.**
  When a model opted out of the `handler` generator via per-model
  `generators.disabled`, the generated `mod.rs` / routes-composer still
  imported and called `create_<name>_routes`, breaking compilation. Both
  generators now filter the model list through the same
  `model_skips_target(Handler)` check used elsewhere.

## [0.2.1] — 2026-05-25

### Added

- **Per-model generator overrides.** Individual model entries now honour
  `enabled` / `disabled` lists, either as a direct `generators:` field or
  wrapped under `config.generators:` to mirror the file-level shape.
  Models that opt out of a target are dropped wholesale — no file emitted
  and no entry in the generated parent `mod.rs`. See
  [docs/schema-format.md § Generators Configuration](docs/schema-format.md#generators-configuration).

### Fixed

- **SQL per-model migrations are now self-contained for enums.** Each
  table migration prepends `CREATE TYPE ... IF NOT EXISTS` for every
  enum its fields reference. Previously a new enum declared alongside a
  new model could be silently skipped on non-`--force` regens because
  the consolidated `create_enums.up.sql` already existed.
- **Rust generator** emitted a duplicate `#[cfg(feature = "openapi")]`
  attribute above the `utoipa::ToSchema` import in some files.

## [0.2.0] — 2026-05-20

### Added

- **`metaphor.codegen.yaml` manifest** with a `user_owned` glob list. Files
  matched by these globs are skipped wholesale during generation — neither
  read, merged, nor written. Lets application code own files inside the
  generator's output tree without losing them on regen. The `--force`
  migration cleanup pass also honours the manifest. Missing manifest →
  empty allowlist, preserving the previous behaviour for repos that
  haven't adopted it.
- **Hand-written migration protection** under `--force`. The cleanup pass
  now only deletes files that carry the `-- Generated by metaphor-schema`
  header; audit-trigger migrations, backfills, and ad-hoc data fixes
  survive even when they share the `NNN_*.up.sql` numbering convention.

### Changed

- **Internal refactor: `src/commands/schema.rs` (3562 LOC) → 29 focused
  files** under `src/commands/schema/`. No public-API changes; the CLI
  surface (`metaphor schema {generate,parse,validate,…}`) is unchanged.
  The schema command is now organised by responsibility:
  - `mod.rs` — top-level dispatcher and `SchemaAction` enum.
  - `generate/` — orchestrator split into named phases
    (`change_detect`, `announce`, `load`, `migration_cleanup`, `write`,
    `post_check`).
  - `merge/` — merge strategies split into `yaml_config`, `seed`, and
    `custom_blocks/{markers, single_marker, paired_methods, unprotected}`.
  - `migration_cmd/` — `migration` and `status` commands sharing a
    `snapshot` helper.
  - Per-command files: `parse`, `validate`, `diff`, `watch`, `changed`.
  - Shared helpers: `discovery`, `module_loader`, `migrations`, `manifest`.

  No file now exceeds 290 LOC. Each submodule keeps its own tests next
  to the code they cover.

### Tests

- Fix `test_deep_merge_yaml_recursive_merge` — the assertion expected
  `port: 3000` to come back as a YAML String, but `serde_yaml` parses
  unquoted integers as Number. The merge behaviour was always correct;
  only the assertion was wrong.
- Replace `test_missing_end_custom_truncation` with
  `test_large_paired_custom_block_preserved_in_full` to match the
  intentional removal of the 200-line truncation cap in 0.1.21
  (commit `11723ed`). Large paired CUSTOM blocks now survive merge
  intact.

## [0.1.21] — 2026-05-17

### Fixed

- **merge:** `// <<< CUSTOM` blocks now use an anchor-trailing-comma
  heuristic to decide whether they belong **inside** the enclosing
  container (struct fields, enum variants, `pub use { … }` lists) or
  **after** it (module-scope items like `pub mod`, `pub use`, `pub fn`,
  `impl …`). The merger inspects the first real content line of the
  block instead of relying on the anchor alone. See
  [docs/generate-rust.md → Preserving Custom Code](docs/generate-rust.md#preserving-custom-code-on-regeneration).
- **graphql:** deduplicate the `Service` import in resolver modules
  when a module has multiple entities; emit `soft_delete<Entity>` /
  `restore<Entity>` mutations for `SoftDeletable` models instead of a
  hard `delete<Entity>`; always append the `Schema` suffix to merged
  roots (`SchemaQuery` / `SchemaMutation`) to avoid colliding with the
  per-entity `Query` / `Mutation` types in the host service.
- **config:** write to `src/config/generated.rs` (not `src/config.rs`)
  and rename the emitted struct from `Config` to `ModuleConfig` so a
  hand-written `src/config/mod.rs` can re-export it and consumer crates
  can `use crate::config::ModuleConfig` from multiple modules without
  name collisions.
- **module:** respect `generators.disabled: [grpc]` / `[graphql]` when
  emitting `lib.rs` — drop the router-mount, gRPC service registration,
  and `MergedObject!` entries for disabled transports. Insert a
  `// <<< CUSTOM` placeholder immediately after `with_database(...)` so
  consumers have a stable, merge-safe slot for extra setup. Rename the
  per-module merged GraphQL roots to `SchemaQuery` / `SchemaMutation`.
- **dto / rust:** gate `use utoipa::ToSchema;` behind
  `#[cfg(feature = "openapi")]` so modules that opt out of the
  `openapi` feature compile without pulling in `utoipa`.
- **handler:** use `middleware::AuthContext` from the framework crate,
  destructure `Extension<AuthContext>` at the handler boundary, extract
  the bearer token from the `Authorization` header, and add
  `Send + Sync` bounds on async dependencies so generated handlers
  compose with Axum's `Router`.

## [0.1.20] — 2026-05-16

### Fixed

- **schema:** dedup regenerated migrations by **identity**, not exact
  filename. Migration filenames are timestamped on every regen
  (`20260426220110_create_provider_staff_table.up.sql`), so a naive
  `exists()` check always missed and wrote duplicates. The generator
  now treats any sibling under `migrations/` with the same
  `_<identity>.{up,down}.sql` suffix as "already migrated" and skips
  it. `--force` bypasses the check.

### Docs

- **kotlin:** document SWR `observeAll` inheritance in the offline
  repository template.

## [0.1.19] — 2026-05-15

### Docs

- **kotlin:** document the `offline-repositories` generation target.

## [0.1.18] — 2026-05-15

### Added

- **kotlin:** `offline-repositories` generation target — emits
  `Offline<Entity>Repository.kt` subclasses of `OfflineFirstRepository<T>`
  with cache-first reads, cache-aware writes, and TTL caching. Delta-sync
  is opt-in via a `// <<< CUSTOM` companion file.

### Docs

- **kotlin:** document camelCase field generation and the flat
  `/api/v1` base path.

## [0.1.17] — 2026-05-14

### Fixed

- **kotlin/api:** drop the module prefix from the API client base path.
- **kotlin/entity:** drop `@SerialName` generation; the API uses
  camelCase end-to-end.

### Docs

- **kotlin:** document the `Metadata` typealias and `isDeleted` helper
  behavior.

## [0.1.16] — 2026-05-13

### Fixed

- **kotlin:** scope `NavConfig` parent ref via `../` in handlebars
  `each` blocks.
- **kotlin:** suppress derived `isDeleted` when an explicit
  `is_deleted` column exists on the model.
- **kotlin:** emit `@audit_metadata` fields as the `Metadata` typealias.

## [0.1.15] — 2026-05-12

### Added

- **schema / kotlin:** workspace-aware `MODULE` resolution. The
  positional `MODULE` argument auto-detects from CWD via
  `metaphor.yaml` lookup; Kotlin generation resolves `--output` to a
  workspace project name and walks transitive `external_imports` /
  `depends_on` deps.
- New `workspace` module for `metaphor.yaml`-aware project lookups.

### Docs

- Document workspace-aware MODULE auto-detect and the kotlin
  `--output` flag.

## [0.1.14] — 2026-05-11

### Changed

- **sql:** share the migration timestamp helper across generators (no
  user-visible change).

## [0.1.13] — 2026-05-10

### Added

- **sql:** emit timestamped paired up/down migrations with foreign keys
  inlined into the `CREATE TABLE` statement.
- **sql:** expression indexes on audit-metadata `created_at` /
  `updated_at` keys.

### Docs

- Document partial indexes and the audit-metadata `WHERE` rewrite.

## [0.1.12] — 2026-05-09

### Fixed

- Rewrite audit-metadata keys in partial-index `WHERE` clauses
  (`metadata->>'created_by'` form), matching the JSONB shape on disk.

## [0.1.11] — 2026-05-08

### Fixed

- Exclude doc comments (`/// … // <<< CUSTOM …`) from CUSTOM-marker
  detection — previously, prose mentioning the marker could leak stale
  content into the merged output.

## [0.1.10] — 2026-05-07

### Added

- Separate read/write route generators for CRUD handlers.

### Fixed

- Emit the `grpc` module unconditionally; keep `graphql` feature-gated.

## [0.1.9] — 2026-05-07

### Fixed

- Declare per-entity service modules as `pub`.
- Emit generated routes into `routes/generated.rs` to avoid `E0761`.

### Changed

- Simplify event-stream methods to `fetch_all + stream::iter`.

## [0.1.8] — 2026-05-06

### Fixed

- Skip the `ServiceGuard` alias when a sibling `{Name}Service` entity
  exists.

### Changed

- Consolidate `to_snake_case` into shared utils with acronym support.

---

Older versions are not retroactively chronicled — see `git log` for
pre-0.1.8 history.
