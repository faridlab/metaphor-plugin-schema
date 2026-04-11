# Plugin Schema Generator

Schema-driven code generator for Metaphor Framework.

## Overview

`metaphor-schema` parses schema definition files and generates **31 different targets** including:

- **Data Layer (5)**: Protocol Buffers, Rust structs, SQL migrations, repositories, repository traits
- **Business Logic (11)**: Services, domain services, use cases, authentication, events, state machines, validators, permissions, specifications, CQRS, computed fields
- **API Layer (4)**: REST handlers (Axum), gRPC services (Tonic with streaming), OpenAPI specifications, DTOs
- **Infrastructure (11)**: Triggers, workflow orchestration, modules, configuration, value objects, projections, event store, exports, integration adapters, event subscriptions, API versioning

## Installation

The schema CLI is available as part of Metaphor Framework:

```bash
# Via metaphor CLI (recommended)
metaphor schema --help

# Or directly
./target/release/metaphor-schema schema --help
```

## Usage

### Parse Schema Files

Parse and display AST for debugging:

```bash
# Parse YAML schemas
metaphor schema parse libs/modules/sapiens/schema/

# Legacy DSL parsing
metaphor schema parse-legacy libs/modules/sapiens/schema/

# JSON output
metaphor schema parse libs/modules/sapiens/schema/ --format json
```

### Validate Schemas

Validate schema files for errors:

```bash
metaphor schema validate sapiens

# Include warnings
metaphor schema validate sapiens --warnings
```

### Generate Code

Generate code from schema definitions:

```bash
# Generate all 31 targets
metaphor schema generate sapiens

# Generate specific targets
metaphor schema generate sapiens --target proto,rust,sql,repository,handler

# Dry run (show what would be generated)
metaphor schema generate sapiens --dry-run

# Force overwrite existing files
metaphor schema generate sapiens --force

# Generate only changed schemas (git-aware)
metaphor schema generate sapiens --changed
```

## Generator Improvements

### PascalCase Type Naming

Custom enum type names are automatically converted to **PascalCase** to follow Rust naming conventions.

**Schema Definition:**
```yaml
enums:
  - name: relationship_type
    values: [lead, customer, partner]
```

**Generated Code:**
```rust
// Automatically converted to PascalCase
pub enum RelationshipType {
    Lead,
    Customer,
    Partner,
}
```

**Benefits:**
- ✅ Zero compilation errors from type naming
- ✅ Consistent Rust naming conventions
- ✅ No manual corrections needed

### Public Module Visibility

All entity and enum modules are automatically declared as `pub mod` to enable cross-module access.

### Auto-Generated Imports

Validators automatically include necessary imports like `chrono::{DateTime, Utc}`.

### Soft Delete Support

Models can enable soft-delete functionality:

```yaml
models:
  - name: User
    soft_delete: true
```

**Generated Features:**
- Automatic `deleted_at` timestamp field
- Trash listing endpoints
- Restore endpoint
- Repository methods with soft-delete filtering

### Foreign Key Support

Relations support foreign key actions for database integrity:

```yaml
relations:
  customer:
    type: Customer
    attributes: ["@one", "@foreign_key(customer_id)", "@on_delete(restrict)"]

  items:
    type: OrderItem[]
    attributes: ["@one_to_many", "@on_delete(cascade)"]
```

**Supported Actions:**
- `@on_delete(cascade)` - Auto-delete child records
- `@on_delete(restrict)` - Prevent deletion if referenced
- `@on_delete(set_null)` - Set FK to NULL
- `@on_delete(set_default)` - Set FK to default value

## Generation Targets

### Data Layer (5 generators)
| Target | Aliases | Description |
|--------|---------|-------------|
| `proto` | `protobuf` | Protocol Buffer definitions |
| `rust` | - | Rust structs and enums |
| `sql` | `migration`, `migrations` | PostgreSQL migrations |
| `repository` | `repo` | Repository implementations |
| `repository-trait` | `repo-trait` | Repository trait definitions |

### Business Logic (11 generators)
| Target | Aliases | Description |
|--------|---------|-------------|
| `service` | `services`, `svc` | Application service layer |
| `domain-service` | `domain_service`, `domain-svc` | Domain services |
| `usecase` | `usecases`, `use-case`, `interactor` | Clean Architecture use cases |
| `auth` | `authentication`, `authorization` | Auth/authorization logic |
| `events` | `domain-events`, `messaging` | Domain event handling |
| `state-machine` | `statemachine`, `sm` | State machine implementations |
| `validator` | `validation` | Validation logic |
| `permission` | `permissions`, `perm` | Permission checks |
| `specification` | `spec`, `specifications` | Business rule specifications |
| `cqrs` | `command`, `query`, `queries` | CQRS implementations |
| `computed` | `computed-fields`, `virtual` | Computed field logic |

### API Layer (4 generators)
| Target | Aliases | Description |
|--------|---------|-------------|
| `handler` | `handlers`, `rest` | Axum REST handlers |
| `grpc` | `tonic` | Tonic gRPC services (with streaming) |
| `openapi` | `swagger` | OpenAPI 3.0 specifications |
| `dto` | `dtos`, `data-transfer` | DTOs for requests/responses |

### Infrastructure (11 generators)
| Target | Aliases | Description |
|--------|---------|-------------|
| `trigger` | `triggers` | Event/trigger handlers |
| `workflow` | `flow`, `flows`, `saga`, `orchestration` | Workflow orchestration |
| `module` | `mod`, `lib` | Module-level code |
| `config` | `configuration`, `settings` | Configuration code |
| `value-object` | `value_object`, `vo` | Value object definitions |
| `projection` | `projections`, `read-model` | CQRS read models |
| `event-store` | `event_store`, `eventstore` | Event sourcing store |
| `export` | `exports`, `public-api` | Public API exports |
| `integration` | `acl`, `anti-corruption` | Integration adapters |
| `event-subscription` | `subscription`, `subscriptions` | Event handlers |
| `versioning` | `version`, `api-version` | API versioning |

### Special Targets
| Target | Description |
|--------|-------------|
| `all` | Generate all 31 targets |

## Schema Formats

The system supports **both YAML and legacy DSL formats**:

### YAML Format (Recommended)
- `*.model.yaml` - Entity definitions with shared types composition
- `*.hook.yaml` - Entity lifecycle behaviors (state machines, rules, triggers)
- `*.workflow.yaml` - Multi-step business processes (Saga pattern)

### Legacy DSL (Backward Compatible)
- `*.model.schema` - Custom DSL format
- `*.workflow.schema` - Custom DSL format

## Supported Types

### Primitive Types
| Type | Description |
|------|-------------|
| `string` | UTF-8 string |
| `int`, `int32` | 32-bit integer |
| `int64` | 64-bit integer |
| `float`, `float32` | 32-bit float |
| `float64` | 64-bit float |
| `bool` | Boolean |
| `uuid` | UUID v4 |
| `datetime` | Timestamp |
| `date` | Date only |
| `time` | Time only |
| `decimal` | Decimal number |
| `json` | JSON data |
| `bytes` | Binary data |
| `email` | Email address |
| `url` | URL |
| `phone` | Phone number |
| `slug` | URL slug |
| `ip`, `ipv4`, `ipv6` | IP address |
| `mac` | MAC address |
| `duration` | Time duration |
| `money` | Monetary value |
| `percentage` | Percentage |
| `markdown` | Markdown text |
| `html` | HTML content |

### Field Attributes
| Attribute | Description |
|-----------|-------------|
| `@id` | Primary key |
| `@unique` | Unique constraint |
| `@required` | Not nullable |
| `@default(value)` | Default value |
| `@min(n)` | Minimum length/value |
| `@max(n)` | Maximum length/value |
| `@pattern(regex)` | Regex pattern |
| `@foreign_key(Model.field)` | Foreign key reference |
| `@soft_delete` | Enable soft-delete |
| `@on_delete(action)` | Foreign key delete action |
| `@on_update(action)` | Foreign key update action |

### Relation Attributes
| Attribute | Values | Description |
|-----------|--------|-------------|
| `@one` | One-to-one |
| `@many` | One-to-many |
| `@many_to_many` | Many-to-many |
| `@belongs_to` | Inverse relation |
| `@references(column)` | Custom referenced column |
| `@join_table(name)` | Many-to-many join table |
| `@join_fk(column)` | Many-to-many FK column |
| `@foreign_key(column)` | Explicit FK column name |

## Architecture

```
metaphor-schema/
├── src/
│   ├── ast/           # Abstract Syntax Tree definitions
│   │   ├── mod.rs
│   │   ├── model.rs   # Model AST nodes
│   │   └── expressions.rs
│   ├── parser/        # Schema parser
│   │   ├── mod.rs
│   │   ├── lexer.rs   # Logos-based tokenizer
│   │   └── *.parser.rs
│   ├── resolver/      # Type and reference resolution
│   │   ├── mod.rs
│   │   ├── type_resolver.rs
│   │   └── validator.rs
│   ├── generators/    # Code generators
│   │   ├── mod.rs
│   │   ├── proto.rs      # Protocol Buffers
│   │   ├── rust.rs       # Rust structs
│   │   ├── sql.rs        # SQL migrations
│   │   ├── repository.rs # Repository pattern
│   │   ├── state_machine.rs
│   │   ├── validator.rs
│   │   ├── permission.rs
│   │   ├── handler.rs    # REST handlers
│   │   ├── grpc.rs       # gRPC services
│   │   ├── openapi.rs    # OpenAPI spec
│   │   ├── trigger.rs    # Event triggers
│   │   └── ...
│   ├── commands/      # CLI commands
│   │   ├── mod.rs
│   │   └── schema.rs
│   ├── lib.rs
│   └── main.rs
└── tests/
    └── fixtures/      # Test schema files
```

## Development

### Running Tests

```bash
cargo test --package metaphor-schema
```

### Building

```bash
cargo build --package metaphor-schema
```

## License

MIT
