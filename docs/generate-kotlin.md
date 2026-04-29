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
| `all` | - | Generate all targets (default) |
| `entities` | Domain | Kotlin data classes for domain entities |
| `enums` | Domain | Sealed classes / enums |
| `repositories` | Domain | Repository interfaces |
| `usecases` | Application | Use case classes (application layer) |
| `app-services` | Application | Application service classes |
| `mappers` | Application | Data mappers between layers |
| `validators` | Application | Input validation logic |
| `api-clients` | Infrastructure | Ktor HTTP API clients |
| `database` | Infrastructure | SQLDelight database schemas and queries |
| `sync` | Infrastructure | Offline sync managers |
| `view-models` | Presentation | MVI ViewModels (Decompose) |
| `components` | Presentation | Reusable Compose UI components |
| `navigation` | Presentation | Decompose navigation and routing |
| `theme` | Presentation | Material 3 theme definitions |
| `tests` | Testing | Test stubs (validator tests, ViewModel tests, API client mock tests) |

---

## Architecture

The generated code follows a **Clean Architecture** layout for Kotlin Multiplatform:

```
shared/src/commonMain/kotlin/{package}/
  ├── domain/
  │   ├── entity/          # Data classes (entities target)
  │   ├── enum/            # Sealed classes (enums target)
  │   └── repository/      # Repository interfaces (repositories target)
  ├── application/
  │   ├── usecase/         # Use cases (usecases target)
  │   ├── service/         # App services (app-services target)
  │   ├── mapper/          # Data mappers (mappers target)
  │   └── validator/       # Validators (validators target)
  ├── infrastructure/
  │   ├── api/             # Ktor clients (api-clients target)
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

1. **`build.gradle.kts`** -- Reads the `namespace` declaration
2. **SQLDelight config** -- Reads the package from SQLDelight setup
3. **Existing Kotlin files** -- Scans for `package` declarations in existing source files
4. **Fallback** -- Uses a default package based on the project name

The detected package is used as the base, with layer and module suffixes appended:

```
{base_package}.{layer}.{module}
# Example: com.bersihir.domain.sapiens
```

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
              └── sapiens/
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

- Kotlin `data class` with proper serialization annotations
- Type mapping: `uuid` -> `String`, `timestamp` -> `Instant`, `date` -> `LocalDate`
- Automatic import generation
- Enum type references with proper package resolution
- **`@audit_metadata` fields** are emitted as the `Metadata` typealias
  (`typealias Metadata = Map<String, JsonElement?>`), not as raw `JsonElement`.
  This lets the derived helpers (e.g. `metadata["deleted_at"]`) compile and
  keeps DTOs / FormData / Mappers all in agreement on the type.
- **Soft-delete derived helper.** When a model has `@soft_delete` *and* an
  `@audit_metadata` field, the entity gains a derived
  `val isDeleted: Boolean get() = metadata["deleted_at"] != null` helper.
  If the schema **also** declares an explicit `is_deleted` column, the helper
  is suppressed (the explicit column wins) so the entity has a single
  canonical `isDeleted` property and no duplicate-declaration error.

### API Clients

- Ktor HTTP client setup with JSON serialization
- CRUD methods (create, getById, list, update, delete)
- Error handling with sealed result types
- Pagination support

### Database (SQLDelight)

- SQLDelight `.sq` files with typed queries
- INSERT, SELECT, UPDATE, DELETE statements
- Migration support
- Offline-first data access

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
