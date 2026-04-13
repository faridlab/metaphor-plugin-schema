# Kotlin Multiplatform Mobile Code Generation

Deep-dive into the `metaphor schema generate:kotlin` pipeline. This generates Kotlin Multiplatform (KMP) code for Android and iOS mobile apps, producing a complete layered architecture.

## Quick Start

```bash
# Generate all Kotlin targets
metaphor schema generate:kotlin sapiens

# Generate only domain layer
metaphor schema generate:kotlin sapiens --target entities,enums,repositories

# Custom output directory
metaphor schema generate:kotlin sapiens --output ./my-app/shared/src/commonMain/
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

Default output: `apps/mobileapp/shared/src/commonMain/`

The generator adds the Kotlin package path structure automatically:

```
apps/mobileapp/shared/src/commonMain/
  └── kotlin/
      └── com/
          └── myapp/
              └── sapiens/
                  ├── domain/
                  ├── application/
                  ├── infrastructure/
                  └── presentation/
```

Override with `--output`:

```bash
metaphor schema generate:kotlin sapiens --output ./my-app/shared/src/commonMain/
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
- Metadata field support for audit tracking

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

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target`, `-t` | string(s) | `all` | Comma-separated targets |
| `--module-path` | path | `libs/modules` | Where `libs/modules/` is located |
| `--output`, `-o` | path | `apps/mobileapp/shared/src/commonMain/` | Output directory |
| `--package`, `-p` | string | auto-detect | Kotlin package name |
| `--skip-existing` | flag | - | Do not overwrite existing files |
| `--verbose`, `-v` | flag | - | Show detailed output |

---

## Practical Examples

### Full Generation

```bash
metaphor schema generate:kotlin sapiens
```

### Domain Layer Only

```bash
metaphor schema generate:kotlin sapiens --target entities,enums,repositories
```

### Infrastructure Layer Only

```bash
metaphor schema generate:kotlin sapiens --target api-clients,database,sync
```

### Preserve Customized Files

When you've manually edited generated files and don't want them overwritten:

```bash
metaphor schema generate:kotlin sapiens --skip-existing
```

### Debug Package Detection

```bash
metaphor schema generate:kotlin sapiens --verbose
```

This prints the detected package name and source.

### Multiple Modules

```bash
metaphor schema generate:kotlin sapiens
metaphor schema generate:kotlin commerce
metaphor schema generate:kotlin messaging
```

Each module generates into its own package namespace.
