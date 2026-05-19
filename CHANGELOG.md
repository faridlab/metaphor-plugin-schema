# Changelog

All notable changes to `metaphor-plugin-schema` are documented here.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
