# Kotlin Multiplatform Mobile Code Generation

Deep-dive into the `metaphor schema generate:kotlin` pipeline. This generates Kotlin Multiplatform (KMP) code for Android and iOS mobile apps, producing a complete layered architecture.

## Quick Start

```bash
# From inside a workspace project dir (auto-detects MODULE)
metaphor schema generate:kotlin --output bersihir-mobile-laundry

# Same thing, MODULE explicit
metaphor schema generate:kotlin bersihir-service --output bersihir-mobile-laundry

# Schema-module name also works
metaphor schema generate:kotlin bersihir --output bersihir-mobile-laundry

# Subset of targets only
metaphor schema generate:kotlin --target entities,enums,repositories --output bersihir-mobile-laundry

# Skip transitive schema-module deps (otherwise the generator also walks
# `external_imports` and emits Kotlin for sapiens, bucket, etc.)
metaphor schema generate:kotlin --output bersihir-mobile-laundry --no-deps

# Raw filesystem path instead of a workspace project name
metaphor schema generate:kotlin --output-path /tmp/preview
```

---

## How MODULE and --output Resolve

Both arguments are workspace-aware when invoked inside a Metaphor workspace (a directory with `metaphor.yaml` somewhere up the tree).

### MODULE argument

In order:

1. **Auto-detect from CWD** — if omitted entirely, walks up from CWD until it matches a `metaphor.yaml` project's `path:`. Errors with the available project list if no match.
2. **Workspace project name** — e.g. `bersihir-service`, `backbone-sapiens`. Resolves to that project's `schema/` directory.
3. **Schema `module:` value** — e.g. `bersihir`, `sapiens`. Read from each project's `schema/models/index.model.yaml` and matched.
4. **Legacy fallback** — `<--module-path>/<MODULE>/schema` (kept for non-workspace layouts).

When MODULE was given as a project name (e.g. `bersihir-service`), the Kotlin generator translates it to the schema's declared `module:` value (`bersihir`) before passing it through to package-name generation. This avoids hyphens-in-Kotlin-package issues.

### --output vs --output-path

`--output` and `--output-path` are mutually exclusive. Pick the one that matches your intent:

| Flag | Argument | Use when |
|------|----------|----------|
| `--output`, `-o` | Workspace project name | You want generated code to land in a registered KMP app, e.g. `bersihir-mobile-laundry`. Resolves to `<project-path>/shared/src/commonMain/kotlin`. |
| `--output-path` | Raw filesystem path | Preview to `/tmp`, ad-hoc dump elsewhere, or generation outside a workspace. |

If `--output` doesn't match any `metaphor.yaml` project, the resolver also tries `apps/<name>/shared/src/commonMain/kotlin` on disk — apps that exist but aren't yet declared in `metaphor.yaml` still resolve. If neither lookup succeeds, the command errors with the list of available projects rather than silently creating a directory.

If neither flag is provided, the generator falls back to its built-in default (`apps/mobileapp/shared/src/commonMain`), which exists for non-workspace use only.

---

## Transitive Dependencies

By default `generate:kotlin` walks the primary module's schema for `external_imports[*].module` references plus any `depends_on` entries declared for the project in `metaphor.yaml`, then runs the generator once per dependency in the same invocation. Modules that resolve to a project without a `schema/` dir (e.g. crate-only deps like `backbone-framework`) are silently skipped; modules referenced via `external_imports` that don't exist on disk yet are skipped with a warning.

Pass `--no-deps` to opt out and generate only the primary module.

```bash
# Primary + transitive deps (default)
metaphor schema generate:kotlin --output bersihir-mobile-laundry

# Primary only
metaphor schema generate:kotlin --output bersihir-mobile-laundry --no-deps
```

---

## Generation Targets

| Target | Layer | Description |
|--------|-------|-------------|
| `all` | - | Generate all targets except `database` (default) |
| `entities` | Domain | Kotlin data classes for domain entities |
| `enums` | Domain | Sealed classes / enums |
| `repositories` | Domain | Repository interfaces |
| `usecases` | Application | Use case classes (application layer) |
| `app-services` | Application | Application service classes |
| `mappers` | Application | Data mappers between layers |
| `validators` | Application | Input validation logic |
| `api-clients` | Infrastructure | Ktor HTTP API clients |
| `offline-repositories` | Infrastructure | Offline-first repository implementations wrapping `*ApiClient` (cache-first reads, cache-aware writes, offline fallback) |
| `database` | Infrastructure | SQLDelight database schemas and queries. **Excluded from `all`** — request explicitly with `--target database`. Under the offline-first cache architecture, entities are stored as JSON in a generic cache table (`CacheDao`); the emitted `.sq` files land under `generated/sqldelight/` (not a SQLDelight source root) and are never compiled. |
| `sync` | Infrastructure | Offline sync managers |
| `view-models` | Presentation | MVI ViewModels (Decompose) |
| `components` | Presentation | Reusable Compose UI components |
| `navigation` | Presentation | Decompose navigation and routing |
| `theme` | Presentation | Material 3 theme definitions |
| `tests` | Testing | Test stubs (validator tests, ViewModel tests, API client mock tests) |

### Disabling targets app-wide (`metaphor.codegen.yaml`)

A consuming app can persistently skip Kotlin targets it fully hand-writes
(e.g. `offline-repositories`, `sync`) without editing the shared product schema
or its read-only upstream module schemas. Add a `disabled_targets:` list to the
app's `metaphor.codegen.yaml`:

```yaml
# <app-root>/metaphor.codegen.yaml
disabled_targets:
  - offline-repositories
  - sync
```

- Resolved from the **output project's** `metaphor.codegen.yaml` — the generator
  walks up from the resolved Kotlin source root to find the nearest ancestor
  holding the file (the generator-owned manifest below the source root is never
  matched).
- Applied **after** expanding `all`, so every module honors the app's choice —
  including read-only transitive deps you can't (or don't want to) edit.
- Entries are matched case-insensitively against target names; unknown or
  non-Kotlin names are ignored. A missing or unparseable file disables nothing.
- Disabled targets are reported on each run (`→ disabled targets (app
  metaphor.codegen.yaml): …`).

This complements the per-model `generators.disabled:` schema field (which gates
one model across all consumers); `disabled_targets` gates whole targets for one
app across all models and modules.

---

## Architecture

The generated code follows a **Clean Architecture** layout for Kotlin
Multiplatform. The tree is **module-first** under the `<base>.generated`
namespace (`generated/<module>/<layer>/…`):

```
shared/src/commonMain/kotlin/{base}/generated/{module}/
  ├── domain/
  │   ├── entity/          # Data classes (entities target)
  │   ├── enums/           # Sealed classes + the Metadata typealias (enums target)
  │   └── repository/      # Repository interfaces (repositories target)
  ├── application/
  │   ├── usecase/         # Use cases (usecases target)
  │   ├── service/         # App services (app-services target)
  │   ├── mapper/          # Data mappers (mappers target)
  │   └── validator/       # Validators (validators target)
  ├── infrastructure/
  │   ├── api/             # Ktor clients (api-clients target)
  │   ├── repository/
  │   │   └── offline/     # Offline<Entity>Repository.kt (offline-repositories target)
  │   ├── database/        # SQLDelight schemas (database target)
  │   └── sync/            # Sync managers (sync target)
  └── presentation/
      ├── viewmodel/       # MVI ViewModels (view-models target)
      ├── component/       # Compose components (components target)
      ├── navigation/      # Navigation (navigation target)
      └── theme/           # Material 3 theme (theme target)
```

---

## Package Detection

When `--package` is not provided, the tool auto-detects the Kotlin package name from your project in this order:

1. **`build.gradle.kts`** -- Reads the `namespace` declaration. A trailing
   module/platform segment is stripped to recover the shared base package
   (recognized suffixes: `.shared`, `.android`, `.mobile`, `.ios`,
   `.desktop`, `.web`, `.jvm`, `.js`, `.native`, `.common`) — e.g.
   `namespace = "com.example.mobile"` resolves to `com.example`.
2. **SQLDelight config** -- Reads the package from SQLDelight setup
3. **Existing Kotlin files** -- Scans for `package` declarations in existing source files
4. **Fallback** -- Uses a default package based on the project name

The detected package is the **true base** (the hand-written framework root,
e.g. `com.bersihir`). Generated code is never written directly under it —
everything lands under a generator-owned `<base>.generated` namespace, laid out
**module-first**, with the layer last:

```
{base_package}.generated.{module}.{layer}
# Example: com.bersihir.generated.sapiens.domain.entity
```

The constructor normalizes the package to exactly one trailing `.generated`
segment, so passing either the true base (`com.bersihir`) or an already-suffixed
value (`com.bersihir.generated`) yields the same result — it is never doubled.

### Framework vs generated split

The whole `generated/` subtree is **owned by the generator** and overwritten on
every regen — a `metaphor.codegen.yaml` manifest (`generated: ["**"]`) is dropped
at its root to make that explicit. **Never hand-edit a file inside it.**

Hand-written framework code lives *outside* the tree, in sibling packages under
the true base (`<base>.core`, `<base>.infrastructure.di`, the pagination
contracts, `OfflineFirstRepository`, …). Generated files import these framework
base classes from the true base package, while everything else stays under
`<base>.generated`. Customize generated code via extension functions,
subclasses, or wrapper DTOs in your own packages — never by editing a generated
file.

You can override this entirely with `--package`:

```bash
metaphor schema generate:kotlin sapiens --package com.myapp.sapiens
```

The `{module}` placeholder is supported:

```bash
metaphor schema generate:kotlin sapiens --package com.myapp.{module}
# Resolves to: com.myapp.sapiens
```

---

## Output Directory

Resolution rules are described in [How MODULE and --output Resolve](#how-module-and---output-resolve). The generator always adds the Kotlin package path structure underneath whatever output dir was resolved:

```
<output-root>/
  └── kotlin/
      └── com/
          └── myapp/
              └── generated/          # generator-owned; metaphor.codegen.yaml at this root
                  └── sapiens/         # module-first
                      ├── domain/
                      ├── application/
                      ├── infrastructure/
                      └── presentation/
```

Examples:

```bash
# Workspace project name → <project>/shared/src/commonMain/kotlin
metaphor schema generate:kotlin --output bersihir-mobile-laundry

# Raw filesystem path
metaphor schema generate:kotlin --output-path ./build/preview/kmp
```

---

## Technology Stack

The generated Kotlin code uses these libraries:

| Layer | Technology |
|-------|-----------|
| API Clients | **Ktor** (HTTP client with serialization) |
| Database | **SQLDelight** (type-safe SQL, offline-first) |
| ViewModels | **MVI pattern** with state management |
| Navigation | **Decompose** (lifecycle-aware routing) |
| UI | **Jetpack Compose** (Material 3) |
| Serialization | **kotlinx.serialization** |
| Async | **Kotlin Coroutines** + **Flow** |

---

## Generated Code Features

### Entities

- Kotlin `data class` annotated `@Serializable` (kotlinx.serialization)
- Field names are converted to **camelCase** and used as-is on the wire — no
  `@SerialName` annotation is emitted. The backend API serializes in camelCase,
  so the Kotlin property name and the JSON key match by default.
- Type mapping: `uuid` -> `String`, `timestamp` -> `Instant`, `date` -> `LocalDate`
- Automatic import generation
- Enum type references with proper package resolution
- **`@audit_metadata` fields** are emitted as the `Metadata` typealias
  (`typealias Metadata = Map<String, JsonElement?>`), not as raw `JsonElement`.
  This lets the derived helpers (e.g. `metadata["deleted_at"]`) compile and
  keeps DTOs / FormData / Mappers all in agreement on the type. The typealias
  itself is generated **once per module** into the module's enums package
  (`<base>.generated.<module>.domain.enums.Metadata`), so the generated tree is
  self-contained and compiles without a hand-written `Metadata` declaration.
- **Soft-delete derived helper.** When a model has `@soft_delete` *and* an
  `@audit_metadata` field, the entity gains a derived
  `val isDeleted: Boolean get() = metadata["deleted_at"] != null` helper.
  If the schema **also** declares an explicit `is_deleted` column, the helper
  is suppressed (the explicit column wins) so the entity has a single
  canonical `isDeleted` property and no duplicate-declaration error.

### API Clients

- Ktor HTTP client setup with JSON serialization
- CRUD methods (create, getById, list, update, delete)
- Base path is `"$baseUrl/api/v1/{collection}"` — the schema module is **not**
  included in the URL. Mount each module's routes under a flat
  `/api/v1/<collection>` namespace on the backend.
- Error handling with sealed result types
- **Pagination types are framework contracts, not generated.**
  `PaginatedResult` / `PaginatedApiResponse` / `BackendPaginatedResponse` /
  `PaginationMeta` are hand-written in the framework (consumed by the base
  `BaseCrudApiClient` / `OfflineFirstRepository`); the generator no longer emits
  a copy. Generated clients import them from the true base package
  (`<base>.infrastructure.pagination.…`, via the `{{framework base_package}}`
  helper). Their meta serializes as camelCase (`totalPages`, `hasNext`,
  `hasPrev`) to match the backend — no `@SerialName` remapping.

### Offline-First Repositories

One `Offline<Entity>Repository.kt` per model that wraps the matching
`<Entity>ApiClient` by extending `OfflineFirstRepository<T>`. The base class
handles cache-first reads, cache-aware writes (delete invalidates per-id and
list caches), and offline fallback — generated subclasses just wire the API
hooks (`fetchOneFromApi`, `fetchListFromApi`, `deleteFromApi`) and
serialization helpers.

- Lives at `infrastructure/repository/offline/` — **shared across modules**,
  not under a per-module folder, so all `Offline*Repository.kt` files share
  one DI lookup location.
- `entityType` is the model's `collection_name()` (snake_case_plural).
- TTL defaults to `CacheTTL.DEFAULT`. Tune per-entity by extending `CacheTTL`
  with a domain-specific constant.
- **Delta-sync is opt-in.** No `fetchListSinceFromApi` override is generated;
  the base class falls back to TTL caching unless you provide one. To enable
  it, extend `<Entity>ApiClient.getAll` with an `updatedSince: String?`
  parameter and override `fetchListSinceFromApi` in a companion
  `Offline<Entity>RepositoryCustom.kt` file marked with `// <<< CUSTOM`.
- **Filter helpers** (e.g. `getAllFiltered(outletId = ...)`) belong in the
  same `*RepositoryCustom.kt` companion alongside the delta-sync override.
- The `// <<< CUSTOM` marker is honored by the writer — files containing it
  are preserved untouched on regeneration.
- **Atomic batch operations** — `bulkDelete(ids)` plus, for soft-deletable
  entities, `bulkRestore(ids)`, `restoreAll()`, and `bulkPermanentDelete(ids)`.
  These are online-only (they require connectivity); on success the whole cache
  is invalidated since many rows change at once, so callers should refresh or
  re-observe afterwards. The matching methods are also declared on the
  `<Entity>Repository` interface.
- Skip per-model with `generators.disabled: [offlinerepositories]` in the
  schema, or whitelist-only with `generators.enabled: [offlinerepositories]`.

### Database (SQLDelight)

- SQLDelight `.sq` files with typed queries
- INSERT, SELECT, UPDATE, DELETE statements
- Migration support
- Offline-first data access

### Mappers

Two files per model, in the same `application/mappers/` package:

- `<Entity>DTO.kt` — the `@Immutable` `<Entity>DTO`, its `<Entity>ListDTO`
  typealias, and the `<Entity>FormData` data class.
- `<Entity>Mapper.kt` — the mapper itself, translating between the entity, its
  DTO, and its `FormData`. Extends `BaseEntityMapper<Entity, EntityDTO>`.

The DTO / FormData declarations are split into their own file so wide entities
keep each generated file under the 250-line size gate. Both files share the same
package, so the mapper references the DTO and FormData directly with no extra
import.

- **Required-but-optional-on-form fields use the catchable `required("field")`
  helper, not `!!`.** When a field is required by the entity (`!is_nullable`)
  but optional on the `FormData` (`form_is_nullable`) — e.g. a required enum,
  whose form default is `null` — `toEntity` asserts it with
  `formData.field.required("field")` instead of a raw `formData.field!!`. A bare
  `!!` throws an uncaught `NullPointerException` and crashes the app on a
  partially-filled form; `required(...)` raises a catchable, message-bearing
  validation error the UI can surface. The `core.mapper.required` import is
  emitted only when at least one field needs it.

### ViewModels

- MVI (Model-View-Intent) architecture
- State sealed classes
- Intent/Action processing
- Side effects handling

---

## Options Reference

### Positional

| Argument | Required | Description |
|----------|----------|-------------|
| `MODULE` | optional inside a workspace | Project name (`bersihir-service`), schema `module:` value (`bersihir`), or legacy direct path. Auto-detects from CWD when omitted inside a workspace. |

### Flags

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target`, `-t` | string(s) | `all` | Comma-separated targets |
| `--output`, `-o` | string | — | Workspace project name (resolves to `<project>/shared/src/commonMain/kotlin`). Mutually exclusive with `--output-path`. |
| `--output-path` | path | — | Raw filesystem output path. Mutually exclusive with `--output`. |
| `--module-path` | path | `libs/modules` | Legacy fallback for non-workspace layouts; ignored when a workspace is detected. |
| `--package`, `-p` | string | auto-detect | Kotlin package name (auto-detected from `build.gradle.kts` / SQLDelight / existing sources) |
| `--no-deps` | flag | — | Generate only the primary module; skip transitive `external_imports` / `depends_on` deps |
| `--skip-existing` | flag | — | Do not overwrite existing files |
| `--verbose`, `-v` | flag | — | Show detailed output (auto-detected MODULE, resolved schema path, output path) |

---

## Practical Examples

### Regenerate a mobile app after a schema change

From the backend project's directory (e.g. `apps/bersihir-service/`):

```bash
metaphor schema generate:kotlin --output bersihir-mobile-laundry
```

This auto-detects MODULE from CWD, generates the primary module (`bersihir`) plus its transitive deps (`sapiens`, `bucket`, etc.), and writes Kotlin into `apps/bersihir-mobile-laundry/shared/src/commonMain/kotlin`.

### Primary module only (skip deps)

```bash
metaphor schema generate:kotlin --output bersihir-mobile-laundry --no-deps
```

### Domain layer only

```bash
metaphor schema generate:kotlin --output bersihir-mobile-laundry \
  --target entities,enums,repositories
```

### Preview to a temp dir

```bash
metaphor schema generate:kotlin --output-path /tmp/kmp-preview --no-deps
```

### Preserve customized files

When you've manually edited generated files and don't want them overwritten:

```bash
metaphor schema generate:kotlin --output bersihir-mobile-laundry --skip-existing
```

### Debug auto-detection / package detection

```bash
metaphor schema generate:kotlin --output bersihir-mobile-laundry --verbose
```

Prints the auto-detected MODULE (if applicable), the resolved schema path, the resolved output path, and the detected Kotlin package + source.

### Generate a different module than the current project

```bash
# From inside any workspace project
metaphor schema generate:kotlin sapiens --output bersihir-mobile-laundry
metaphor schema generate:kotlin bucket  --output bersihir-mobile-laundry
```

Each module generates into its own package namespace under the same output root.

---

## Verifying generated code

After regenerating, compile against a real target — `compileKotlinMetadata`
alone is not sufficient. The metadata-only KMP check is permissive and will
miss e.g. duplicate-declaration errors, conflicting type signatures, and
sealed-class hierarchy mismatches. Run a JVM/Android target compile instead:

```bash
./gradlew :shared:compileDebugKotlinAndroid
# or, if you target iOS:
./gradlew :shared:compileKotlinIosSimulatorArm64
```

If you set up CI for the consumer workspace, prefer one of these over the
metadata target.
