# CLI Reference

Complete reference for all `metaphor schema` commands, flags, and options.

## Command Overview

| Command | Description |
|---------|-------------|
| `metaphor schema parse` | Parse schema files and display AST |
| `metaphor schema validate` | Validate schema files for correctness |
| `metaphor schema generate` | Generate server-side Rust code (31+ targets) |
| `metaphor schema generate:rust` | Alias for `schema generate` |
| `metaphor schema generate:kotlin` | Generate Kotlin Multiplatform Mobile code |
| `metaphor schema generate:webapp` | Generate TypeScript + React webapp code |
| `metaphor schema diff` | Show diff between schema and generated code |
| `metaphor schema watch` | Watch schema files and regenerate on changes |
| `metaphor schema migration` | Generate database migrations from schema changes |
| `metaphor schema changed` | Show which schema files have changed (git-aware) |
| `metaphor schema status` | Show schema drift between definitions and database |

---

## `metaphor schema parse`

Parse schema files and output the Abstract Syntax Tree (AST) for debugging and inspection.

```bash
metaphor schema parse [PATH] [OPTIONS]
```

### Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `PATH` | `.` | Path to schema directory or file |

### Options

| Flag | Values | Default | Description |
|------|--------|---------|-------------|
| `--format`, `-f` | `json`, `pretty` | `pretty` | Output format |

### Examples

```bash
# Parse YAML schemas in a directory
metaphor schema parse libs/modules/sapiens/schema/

# JSON output for piping to other tools
metaphor schema parse libs/modules/sapiens/schema/ --format json

# Parse a single file
metaphor schema parse libs/modules/sapiens/schema/user.model.yaml
```

---

## `metaphor schema validate`

Validate schema files for correctness and consistency. Performs comprehensive checks including:

- Schema syntax and structure
- Type references and model relationships
- DDD entity-model associations
- Value object field types
- Domain service dependency resolution
- Authorization permission/role consistency

```bash
metaphor schema validate [MODULE] [OPTIONS]
```

### Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `MODULE` | `.` | Module name or path to schema directory |

### Options

| Flag | Description |
|------|-------------|
| `--warnings`, `-w` | Show warnings in addition to errors |

### Examples

```bash
# Validate a module
metaphor schema validate sapiens

# Include warnings
metaphor schema validate sapiens --warnings
```

---

## `metaphor schema generate`

Generate server-side Rust code from schema definitions. This is the primary generation command, producing up to 38 different code targets organized by architectural layer.

```bash
metaphor schema generate [MODULE] [OPTIONS]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `MODULE` | optional inside a workspace | Module to generate code for. Accepts a workspace project name (`bersihir-service`), the schema's `module:` value (`bersihir`), or a legacy direct path. Auto-detects from CWD when omitted inside a Metaphor workspace. See [Module resolution](generate-rust.md#how-module-resolves) for the full lookup order. |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target`, `-t` | string | `all` | Comma-separated generation targets (see [generate-rust.md](generate-rust.md)) |
| `--output`, `-o` | path | module root | Output directory |
| `--dry-run` | flag | - | Show what would be generated without writing files |
| `--force`, `-f` | flag | - | Overwrite existing files |
| `--split` | flag | - | Split output into multiple files (e.g., one OpenAPI file per entity) |
| `--changed` | flag | - | Only generate for schemas that changed (git-aware) |
| `--base` | string | `HEAD` | Git reference for change detection (e.g., `main`, `origin/main`, `HEAD~3`) |
| `--validate` | flag | - | Run `cargo check` after generation to verify compilation |
| `--models` | string | - | Filter: only generate for specific models (comma-separated) |
| `--hooks` | string | - | Filter: only generate for specific hooks (comma-separated) |
| `--workflows` | string | - | Filter: only generate for specific workflows (comma-separated) |
| `--lenient` | flag | - | Skip strict validation (useful with `--models`/`--hooks`/`--workflows` filters) |

### Examples

```bash
# Auto-detect MODULE from CWD (when run from a project dir)
metaphor schema generate

# Explicit MODULE — workspace project name
metaphor schema generate sapiens

# Or schema `module:` value
metaphor schema generate bucket

# Generate specific targets only
metaphor schema generate --target proto,rust,sql,repository,handler

# Dry run to preview output
metaphor schema generate --dry-run

# Force overwrite existing files
metaphor schema generate --force

# Generate only for changed schemas (CI-friendly)
metaphor schema generate --changed --validate

# Generate only for specific models
metaphor schema generate --models Customer,Order --lenient

# Compare against a specific git branch
metaphor schema generate --changed --base main

# Split OpenAPI specs per entity
metaphor schema generate --target openapi --split
```

---

## `metaphor schema generate:rust`

Shortcut alias for `metaphor schema generate`. Accepts all the same options, including the optional MODULE arg.

```bash
metaphor schema generate:rust [MODULE] [OPTIONS]
```

All flags are identical to `metaphor schema generate`. See above.

### Examples

```bash
metaphor schema generate:rust --target rust,sql,repository
metaphor schema generate:rust sapiens --changed --base main
```

---

## `metaphor schema generate:kotlin`

Generate Kotlin Multiplatform Mobile code from schema definitions. Produces code for the KMP stack including Ktor API clients, SQLDelight database schemas, Decompose navigation, and MVI ViewModels. Walks `external_imports` and `metaphor.yaml` `depends_on` to also generate transitive schema-module dependencies in the same invocation; pass `--no-deps` to opt out.

```bash
metaphor schema generate:kotlin [MODULE] [OPTIONS]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `MODULE` | optional inside a workspace | Workspace project name (`bersihir-service`), schema `module:` value (`bersihir`), or legacy direct path. Auto-detects from CWD when omitted. See [Module resolution](generate-kotlin.md#how-module-and---output-resolve). |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target`, `-t` | string(s) | `all` | Comma-separated generation targets (see [generate-kotlin.md](generate-kotlin.md)) |
| `--output`, `-o` | string | — | Workspace project name (resolves to `<project>/shared/src/commonMain/kotlin`). Mutually exclusive with `--output-path`. |
| `--output-path` | path | — | Raw filesystem output path. Mutually exclusive with `--output`. |
| `--module-path` | path | `libs/modules` | Legacy fallback for non-workspace layouts; ignored when a workspace is detected. |
| `--package`, `-p` | string | auto-detect | Kotlin package name (auto-detects from `build.gradle.kts`) |
| `--no-deps` | flag | — | Skip transitive schema-module dependencies; generate only the primary module |
| `--skip-existing` | flag | — | Skip files that already exist on disk |
| `--verbose`, `-v` | flag | — | Verbose output (auto-detected MODULE, resolved schema path, output path) |

### Package Auto-Detection

When `--package` is not provided, the tool automatically detects the Kotlin package from:

1. `build.gradle.kts` namespace declaration
2. SQLDelight configuration
3. Existing Kotlin source files

### Examples

```bash
# Auto-detect MODULE from CWD, write to a workspace mobileapp project
metaphor schema generate:kotlin --output bersihir-mobile-laundry

# Explicit MODULE (project name)
metaphor schema generate:kotlin bersihir-service --output bersihir-mobile-laundry

# Or schema `module:` value
metaphor schema generate:kotlin sapiens --output bersihir-mobile-laundry

# Generate only domain layer
metaphor schema generate:kotlin --output bersihir-mobile-laundry \
  --target entities,enums,repositories

# Skip transitive deps; generate only the primary module
metaphor schema generate:kotlin --output bersihir-mobile-laundry --no-deps

# Raw filesystem path (e.g. preview to /tmp)
metaphor schema generate:kotlin --output-path /tmp/kmp-preview

# Custom package name
metaphor schema generate:kotlin --output bersihir-mobile-laundry --package com.myapp.{module}

# Skip files that have been customized
metaphor schema generate:kotlin --output bersihir-mobile-laundry --skip-existing

# Verbose output for debugging
metaphor schema generate:kotlin --output bersihir-mobile-laundry --verbose
```

---

## `metaphor schema generate:webapp`

Generate TypeScript + React webapp code from schema definitions. Produces React Query hooks, Zod validation schemas, form components, CRUD pages, and Clean Architecture layers.

```bash
metaphor schema generate:webapp <MODULE> [OPTIONS]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `MODULE` | Module name to generate code for |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target`, `-t` | string | `all` | Comma-separated generation targets (see [generate-webapp.md](generate-webapp.md)) |
| `--entity` | string | - | Generate code for a specific entity only |
| `--output`, `-o` | path | `apps/webapp/src/` | Output directory |
| `--dry-run` | flag | - | Show what would be generated without writing files |
| `--force`, `-f` | flag | - | Overwrite existing files |

### Examples

```bash
# Generate all webapp targets
metaphor schema generate:webapp sapiens

# Generate only hooks and forms
metaphor schema generate:webapp sapiens --target hooks,forms

# Generate for a single entity
metaphor schema generate:webapp sapiens --entity Customer

# Preview what would be generated
metaphor schema generate:webapp sapiens --dry-run

# Force regenerate everything
metaphor schema generate:webapp sapiens --force

# Generate only Clean Architecture domain layer
metaphor schema generate:webapp sapiens --target domain
```

---

## `metaphor schema diff`

Show the diff between current schema definitions and existing generated code. Useful for reviewing what would change before regenerating.

```bash
metaphor schema diff <MODULE> [OPTIONS]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `MODULE` | Module name |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--base` | string | `HEAD` | Base git reference for comparison |

### Examples

```bash
metaphor schema diff sapiens
metaphor schema diff sapiens --base main
```

---

## `metaphor schema watch`

Watch schema files for changes and automatically regenerate code. Useful during development for live code generation.

```bash
metaphor schema watch <MODULE> [OPTIONS]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `MODULE` | Module name to watch |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target`, `-t` | string | `all` | Comma-separated generation targets |
| `--output`, `-o` | path | module root | Output directory |

### Examples

```bash
# Watch and regenerate all targets
metaphor schema watch sapiens

# Watch and regenerate only Rust structs and SQL
metaphor schema watch sapiens --target rust,sql
```

---

## `metaphor schema migration`

Generate database migration SQL from schema changes. Compares the current schema against a snapshot or live database to produce incremental migration scripts.

```bash
metaphor schema migration <MODULE> [OPTIONS]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `MODULE` | Module name |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--output`, `-o` | path | - | Output file for the migration SQL |
| `--destructive` | flag | - | Include destructive changes (DROP statements) |
| `--database-url` | string | `$DATABASE_URL` | Database URL for live introspection |
| `--preview` | flag | - | Preview migration SQL without writing files |
| `--safe-only` | flag | - | Only generate safe operations (skip destructive changes) |

### Examples

```bash
# Generate migration (safe operations only)
metaphor schema migration sapiens --safe-only

# Preview what migration would look like
metaphor schema migration sapiens --preview

# Include destructive changes
metaphor schema migration sapiens --destructive

# Output to file
metaphor schema migration sapiens --output migrations/0001_initial.sql

# Use specific database for introspection
metaphor schema migration sapiens --database-url postgres://localhost/mydb
```

---

## `metaphor schema changed`

Show which schema files have changed using git change detection. Useful for CI pipelines and selective generation.

```bash
metaphor schema changed [MODULE] [OPTIONS]
```

### Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `MODULE` | - | Module name (optional; shows all modules if omitted) |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--base` | string | `HEAD` | Base git reference for comparison |
| `--outputs` | flag | - | Show affected output files |
| `--targets` | flag | - | Show affected generation targets |

### Examples

```bash
# Show all changed schemas
metaphor schema changed

# Show changes for a specific module
metaphor schema changed sapiens

# Show what output files would be affected
metaphor schema changed sapiens --outputs

# Show which targets need regeneration
metaphor schema changed sapiens --targets

# Compare against main branch
metaphor schema changed --base main
```

---

## `metaphor schema status`

Show schema drift between YAML definitions and the database/snapshot. Read-only check that shows what migrations would be needed without generating any files. Useful for CI checks and status monitoring.

```bash
metaphor schema status <MODULE> [OPTIONS]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `MODULE` | Module name |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--database-url` | string | `$DATABASE_URL` | Database URL for live introspection |

### Examples

```bash
# Check schema drift
metaphor schema status sapiens

# Check against specific database
metaphor schema status sapiens --database-url postgres://localhost/mydb
```

---

## Alternative Command Forms

The CLI supports multiple invocation styles for the same functionality:

| Command | Equivalent To |
|---------|--------------|
| `metaphor schema generate sapiens` | Primary form |
| `metaphor schema generate:rust sapiens` | Alias for `schema generate` |
| `metaphor schema kotlin generate sapiens` | Full subcommand form for Kotlin |
| `metaphor schema generate:kotlin sapiens` | Shortcut for Kotlin |
| `metaphor schema generate:webapp sapiens` | Shortcut for webapp |

## Global Help

```bash
# Top-level help
metaphor schema --help

# Command-specific help
metaphor schema generate --help
metaphor schema generate:kotlin --help
metaphor schema generate:webapp --help
metaphor schema migration --help
```
