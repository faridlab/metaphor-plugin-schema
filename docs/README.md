# Metaphor Schema Documentation

Comprehensive documentation for the Metaphor schema-driven code generator.

## Overview

`metaphor-schema` parses schema definition files and generates production-ready code across **three platforms**:

| Pipeline | Command | Output | Targets |
|----------|---------|--------|---------|
| **Rust Server** | `metaphor schema generate` | Rust, SQL, Proto, gRPC, REST | 38 targets |
| **Kotlin Mobile** | `metaphor schema generate:kotlin` | Kotlin Multiplatform (KMP) | 17 targets |
| **Web App** | `metaphor schema generate:webapp` | TypeScript + React | 14 targets |

## Architecture

```
  Schema Files                    Code Generators
  ┌──────────────┐
  │ *.model.yaml │──┐
  │ *.hook.yaml  │──┤    ┌────────┐    ┌──────────┐    ┌─────────────┐
  │ *.workflow   │──┼───>│ Parser │───>│ Resolver │───>│ Generators  │
  │   .yaml      │  │    └────────┘    └──────────┘    ├─────────────┤
  │              │  │         │              │          │ Rust (38)   │
  │ *.model      │──┤    Lexer +        Type &         │ Kotlin (17) │
  │   .schema    │──┘    YAML parse    Reference       │ Webapp (14) │
  └──────────────┘                     Resolution      └─────────────┘
```

**Flow**: Schema files are parsed into an Abstract Syntax Tree (AST), resolved for type references and cross-module dependencies, then fed into platform-specific generators.

## Documentation Map

| Document | Description |
|----------|-------------|
| [Getting Started](getting-started.md) | Create your first schema and generate code |
| [CLI Reference](cli-reference.md) | Every command, every flag, every option |
| [Schema Format](schema-format.md) | YAML and legacy DSL syntax, types, attributes |
| [Rust Generation](generate-rust.md) | 38 server-side targets (proto, rust, sql, handler, grpc, ...) |
| [Kotlin Generation](generate-kotlin.md) | 17 KMP mobile targets (entities, api-clients, offline-repositories, view-models, ...) |
| [Webapp Generation](generate-webapp.md) | 14 TypeScript + React targets (hooks, forms, pages, ...) |
| [DDD Guide](ddd-guide.md) | Entities, value objects, domain services, event sourcing, authorization |
| [Changelog](../CHANGELOG.md) | Per-version history of fixes, features, and breaking changes |

## Key Concepts

### Modules

A **module** is a self-contained domain unit with its own schemas, generated code, and configuration. Modules live under `libs/modules/{module_name}/`.

```
libs/modules/sapiens/
  ├── schema/                  # Schema definitions
  │   ├── user.model.yaml
  │   ├── user.hook.yaml
  │   └── order.model.yaml
  └── src/                     # Generated Rust code
      ├── entity/
      ├── repository/
      ├── handler/
      └── mod.rs
```

### Schema File Types

| File | Purpose |
|------|---------|
| `*.model.yaml` | Data models, enums, DDD extensions (entities, VOs, services) |
| `*.hook.yaml` | Lifecycle behaviors: state machines, rules, triggers |
| `*.workflow.yaml` | Multi-step business processes (Saga pattern) |
| `index.model.yaml` | Module-level configuration |

### Schema Formats

**YAML** (recommended) supports the full feature set including DDD extensions. **Legacy DSL** (`.model.schema`, `.workflow.schema`) is supported for backward compatibility but does not support DDD features.

### Generation Pipelines

- **`metaphor schema generate`** / **`generate:rust`** -- Server-side Rust code with 38 targets across data, business logic, API, and infrastructure layers
- **`metaphor schema generate:kotlin`** -- Kotlin Multiplatform Mobile with 17 targets (Ktor, SQLDelight, Decompose, Compose)
- **`metaphor schema generate:webapp`** -- TypeScript + React with 14 targets (React Query, Zod, CRUD pages, Clean Architecture)

### Type System

30 primitive types with automatic mapping to Rust, PostgreSQL, Protocol Buffer, and Kotlin types. See [Schema Format](schema-format.md#primitive-types) for the full type table.
