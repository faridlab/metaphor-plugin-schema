# CLI Reference

Complete reference for all `metaphor schema` commands, flags, and options.

## Command Overview

| Command | Description |
|---------|-------------|
| `metaphor schema parse` | Parse schema files and display AST |
| `metaphor schema validate` | Validate schema files for correctness |
| `metaphor schema validate-workspace` | Validate cross-module foreign keys across every module |
| `metaphor schema generate` | Generate server-side Rust code (31+ targets) |
| `metaphor schema generate:rust` | Alias for `schema generate` |
| `metaphor schema generate:kotlin` | Generate Kotlin Multiplatform Mobile code |
| `metaphor schema generate:webapp` | Generate TypeScript + React webapp code |
| `metaphor schema diff` | Show diff between schema and generated code |
| `metaphor schema watch` | Watch schema files and regenerate on changes |
| `metaphor schema migration` | Generate database migrations from schema changes |
| `metaphor schema changed` | Show which schema files have changed (git-aware) |
| `metaphor schema status` | Show schema drift between definitions and database |
| `metaphor schema doctor` | Find hand-written references to handlers the generator won't emit |
| `metaphor schema openapi-collect` | Vendor composed modules' generated OpenAPI specs into a consumer app |

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

## `metaphor schema validate-workspace`

Validate every **cross-module** foreign key in the workspace.

`metaphor schema validate` loads one module at a time, so it cannot see the other side
of a `@foreign_key(corpus.Organization.id)` — a reference to an entity that does not
exist passes validation and reaches the generator. This command loads every module
listed in `metaphor.yaml`, builds a registry of module → entities, and reports each
reference that dangles.

Both **direct model fields** and **shared-type fields** are checked. It takes no
arguments and must be run from inside a workspace (anywhere at or below the directory
holding `metaphor.yaml`).

```bash
metaphor schema validate-workspace
```

Modules are keyed by their schema `module:` name (`corpus`), not the project directory
name (`backbone-corpus`). A project with no `schema/` directory is skipped; a module
that fails to parse is reported as a note and skipped, so a single broken module does
not hide dangling references in the rest.

Exits non-zero when any reference dangles — safe to use as a CI gate.

### Examples

```bash
# Check the whole workspace
metaphor schema validate-workspace

# Gate a CI job on cross-module integrity
metaphor schema validate-workspace || exit 1
```

Sample failure output:

```
Validating cross-module foreign keys
  scanned 24 module(s), 46 cross-module reference(s) (direct fields + shared types)
  Error: sapiens.Employee field 'organization_id' has @foreign_key(corpus.Organization...) but module 'corpus' has no entity 'Organization' (phantom cross-module reference)

cross-module validation failed with 1 dangling reference(s)
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
| `--force`, `-f` | flag | - | Overwrite existing files. Migration identity and timestamp stability are owned by a single authoritative **timestamp stabilization** pass that runs first under both plain `generate` and `--force` (the write phase is a dumb writer that just applies the plain `exists()` gate). Timestamp decisions are keyed on a migration's **base name** (the identity shared by its up/down pair): generator-authored migrations keep their on-disk timestamp (rewritten in place — both `.up.sql` and `.down.sql` unified on one timestamp), new ones get a timestamp strictly greater than all existing ones, hand-written migrations are never reused (they get a fresh timestamp and are left for cleanup to preserve), and duplicate-timestamp corruption is pinned to the minimum timestamp deterministically. The cleanup pass then sweeps stale `.up.sql` **and** their paired `.down.sql` files; hand-written migrations (no `-- Generated by metaphor-schema` header) and `user_owned` matches are preserved. |
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

### App-level disabled targets

The output app can skip targets it hand-writes by listing them under
`disabled_targets:` in its `metaphor.codegen.yaml` (resolved from the nearest
ancestor of the Kotlin source root). They are removed from the effective target
set after `all` is expanded, so every module — including read-only deps — honors
the choice. See [generate-kotlin.md § Disabling targets app-wide](generate-kotlin.md#disabling-targets-app-wide-metaphorcodegenyaml).

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

## `metaphor schema doctor`

Find drift between hand-written aggregator/composer files and the
current schema. Specifically, scans every `.rs` file under `src/` and
`tests/` for references to handler-route symbols that the generator
won't emit because the model opted out via per-model
`config.generators.disabled: [handler, …]` (or because the model was
renamed/removed).

Read-only; never writes files. Exits non-zero when drift is found, so
it slots into CI as a guard. Run it **before** `metaphor schema
generate -f` to know up-front which hand-written imports and
`.merge(...)` calls you'll need to delete.

```bash
metaphor schema doctor [MODULE]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `MODULE` | Module name (auto-detected from CWD if omitted) |

### Detection

For every model whose `Handler` target is disabled, the doctor builds
the set of forbidden symbol names:

- `create_<name>_routes`
- `create_<name>_read_routes`
- `create_<name>_write_routes`
- `create_protected_<name>_routes`
- `create_<name>_transition_routes`

Any reference to one of these names in a non-comment line is reported
as drift, tagged by whether the containing file matches a `user_owned`
glob in `metaphor.codegen.yaml`:

- **user-owned** — you must delete the import / `.merge(...)` call.
- **generator-managed (re-emit will overwrite)** — likely stale from a
  pre-fix regen; `metaphor schema generate -f` will heal it.

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | No drift detected |
| `1` | Drift found (report printed to stdout) |
| `2` | Schema load/parse failure |

### Examples

```bash
# Check drift in the current module
metaphor schema doctor

# Check a specific module
metaphor schema doctor sapiens
```

---

## `metaphor schema openapi-collect`

Vendor composed modules' generated OpenAPI specs into a consumer app so it can
serve them from a single Swagger UI.

A consumer (typically a `backend-service`) composes several modules' routers but
exposes one Swagger UI. Each module generates its own
`schema/openapi/openapi.yaml`; this command **copies** those specs into the app
(not references them) so they can be embedded with `include_str!` and offered as
additional Swagger specs. A copy is required because the service's build context
is usually just the app directory — sibling `modules/` aren't reachable at build
time.

Run it from the app directory (or pass the app name). Rebuild the app afterward
to embed the refreshed specs.

```bash
metaphor schema openapi-collect [MODULE]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `MODULE` | Consumer app/project name (auto-detected from CWD if omitted) |

### Configuration

Driven by an `openapi_vendor` section in the app's `metaphor.codegen.yaml`:

```yaml
openapi_vendor:
  dest: src/presentation/http/openapi          # destination dir, relative to the app root
  modules: [backbone-sapiens, backbone-bucket] # optional; defaults to the app's depends_on
```

Each module's spec lands at `<dest>/<short>.openapi.yaml`, where `<short>` is the
module name with any `backbone-` prefix stripped (e.g. `backbone-sapiens` →
`sapiens.openapi.yaml`). A module whose `schema/openapi/openapi.yaml` is missing
is skipped with a warning (generate it first with
`metaphor schema generate --target openapi --force`).

### Examples

```bash
# Collect specs for the app owning the current directory
metaphor schema openapi-collect

# Collect specs for a named app
metaphor schema openapi-collect bersihir-service
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
