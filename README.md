# Metaphor Plugin Schema

Schema-driven code generator for Rust, Kotlin Multiplatform, and TypeScript + React.

## Overview

`metaphor-schema` parses schema definition files (YAML or legacy DSL) and generates production-ready code across three platforms:

- **Rust Server** -- 38 targets: structs, SQL migrations, REST handlers, gRPC services, repositories, domain services, CQRS, event sourcing, and more
- **Kotlin Mobile** -- 17 targets: data classes, API clients, offline-first repositories, SQLDelight schemas, ViewModels, Compose components, navigation
- **Web App** -- 14 targets: React Query hooks, Zod schemas, form components, CRUD pages, Clean Architecture layers

## Quick Start

When invoked from inside a Metaphor workspace project directory (a subdir of any project listed in `metaphor.yaml`), the `MODULE` arg auto-detects from CWD. Pass it explicitly to target a different module — either the schema's `module:` value (e.g. `bersihir`, `sapiens`, `bucket`) or a workspace project name (`bersihir-service`, `backbone-sapiens`).

```bash
# Generate server-side Rust code (38 targets) for the current project
metaphor schema generate

# Generate Kotlin Multiplatform Mobile code into a workspace mobileapp project.
# Also walks `external_imports` and emits Kotlin for transitive schema-module deps
# (e.g. sapiens, bucket) in the same run; pass --no-deps to opt out.
metaphor schema generate:kotlin --output bersihir-mobile-laundry

# Generate TypeScript + React webapp code (14 targets)
metaphor schema generate:webapp sapiens
```

### Other Commands

```bash
# Validate schemas
metaphor schema validate sapiens

# Parse and inspect AST
metaphor schema parse libs/modules/sapiens/schema/

# Watch for changes and auto-regenerate
metaphor schema watch sapiens

# Generate database migrations
metaphor schema migration sapiens --preview

# Show changed schemas (git-aware)
metaphor schema changed sapiens

# Check schema drift against database
metaphor schema status sapiens
```

## Schema Example

```yaml
# libs/modules/sapiens/schema/user.model.yaml

models:
  - name: User
    collection: users
    soft_delete: true
    fields:
      id:
        type: uuid
        primary_key: true
      email:
        type: email
        required: true
        unique: true
      status:
        type: UserStatus
        required: true
      created_at:
        type: timestamp
        auto: true

enums:
  - name: UserStatus
    variants: [Active, Inactive, Suspended]

entities:
  User:
    model: User
    implements: [Auditable, SoftDeletable]
    methods:
      - name: verify_email
        mutates: true
        returns: "Result<(), UserError>"
```

## Documentation

| Document | Description |
|----------|-------------|
| [Getting Started](docs/getting-started.md) | Create your first schema and generate code |
| [CLI Reference](docs/cli-reference.md) | Every command, every flag, every option |
| [Schema Format](docs/schema-format.md) | YAML and legacy DSL syntax, types, attributes |
| [Rust Generation](docs/generate-rust.md) | 38 server-side generation targets |
| [Kotlin Generation](docs/generate-kotlin.md) | 17 Kotlin Multiplatform targets |
| [Webapp Generation](docs/generate-webapp.md) | 14 TypeScript + React targets |
| [DDD Guide](docs/ddd-guide.md) | Entities, value objects, domain services, event sourcing, authorization |

## Development

```bash
# Run tests
cargo test --package metaphor-schema

# Build
cargo build --package metaphor-schema
```

## License

MIT
