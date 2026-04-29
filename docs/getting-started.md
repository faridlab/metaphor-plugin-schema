# Getting Started

This guide walks you through creating your first schema and generating code for Rust, Kotlin, and webapp targets.

## Prerequisites

- Metaphor CLI installed
- A Metaphor project with the standard module layout

## Project Structure

The schema generator supports two layouts:

### Standalone (`libs/modules/`)

```
your-project/
  ├── libs/
  │   └── modules/
  │       └── sapiens/           # Your module
  │           └── schema/        # Schema files go here
  │               ├── user.model.yaml
  │               ├── user.hook.yaml
  │               └── order.model.yaml
  ├── apps/
  │   ├── webapp/                # Webapp output (TypeScript + React)
  │   └── mobileapp/             # Mobile output (Kotlin)
  └── Cargo.toml
```

### Metaphor workspace (`metaphor.yaml` at root)

When `metaphor.yaml` exists, the generator reads it to discover projects and their schema directories. Modules may live anywhere — typically `apps/<service>/schema/` for backend-service projects and `modules/<module>/schema/` for upstream modules. The MODULE arg accepts either a workspace project name or the schema's `module:` field, and auto-detects from CWD when omitted. See [generate-rust.md → How MODULE Resolves](generate-rust.md#how-module-resolves) for the full lookup order.

## Step 1: Create Your First Schema

Create `libs/modules/sapiens/schema/user.model.yaml`:

```yaml
models:
  - name: User
    collection: users
    fields:
      id:
        type: uuid
        primary_key: true
      email:
        type: email
        required: true
        unique: true
      username:
        type: string
        required: true
      status:
        type: UserStatus
        required: true
      created_at:
        type: timestamp
        auto: true
      updated_at:
        type: timestamp
        auto: true

enums:
  - name: UserStatus
    variants:
      - Active
      - Inactive
      - Suspended
```

## Step 2: Validate the Schema

Check your schema for errors before generating:

```bash
metaphor schema validate sapiens
```

## Step 3: Generate Server-Side Code (Rust)

Generate all Rust server-side targets:

```bash
metaphor schema generate sapiens
```

Or use the explicit shortcut:

```bash
metaphor schema generate:rust sapiens
```

This generates 38 targets including Rust structs, SQL migrations, REST handlers, gRPC services, repositories, and more. To generate only specific targets:

```bash
metaphor schema generate sapiens --target rust,sql,repository,handler
```

Preview what would be generated without writing files:

```bash
metaphor schema generate sapiens --dry-run
```

## Step 4: Generate Kotlin Mobile Code

Generate Kotlin Multiplatform code for mobile apps. Inside a Metaphor workspace, MODULE auto-detects from CWD and `--output` accepts the mobileapp project's name:

```bash
metaphor schema generate:kotlin --output mobileapp
```

In a standalone (non-workspace) project, pass MODULE and use `--output-path` for a raw filesystem path:

```bash
metaphor schema generate:kotlin sapiens --output-path ./apps/mobileapp/shared/src/commonMain
```

Either form generates data classes, repository interfaces, API clients, ViewModels, and more for the KMP stack. The generator also walks `external_imports` and emits Kotlin for transitive schema-module dependencies in the same run; pass `--no-deps` to opt out. To generate only specific layers:

```bash
metaphor schema generate:kotlin --output mobileapp --target entities,enums,repositories
```

## Step 5: Generate Webapp Code

Generate TypeScript + React code for the webapp:

```bash
metaphor schema generate:webapp sapiens
```

This generates React Query hooks, Zod validation schemas, form components, CRUD pages, and Clean Architecture layers. To generate only specific targets:

```bash
metaphor schema generate:webapp sapiens --target hooks,schemas,forms,pages
```

## Incremental Generation

### Git-Aware Changed Detection

Only regenerate code for schemas that changed since the last commit:

```bash
metaphor schema generate sapiens --changed
```

Compare against a specific branch:

```bash
metaphor schema generate sapiens --changed --base main
```

### Model Filtering

Generate code for specific models only:

```bash
metaphor schema generate sapiens --models User,Order --lenient
```

The `--lenient` flag allows generation to proceed even if filtered models reference types from excluded models.

### Hook and Workflow Filtering

```bash
metaphor schema generate sapiens --hooks UserHooks --lenient
metaphor schema generate sapiens --workflows OrderProcessing --lenient
```

## Adding DDD Features

Enhance your schema with Domain-Driven Design extensions:

```yaml
# Add to your user.model.yaml

value_objects:
  Email:
    inner_type: String
    validation: email_format

entities:
  User:
    model: User
    implements: [Auditable, SoftDeletable]
    value_objects:
      email: Email
    methods:
      - name: verify_email
        mutates: true
        returns: "Result<(), UserError>"
      - name: can_login
        returns: bool
    invariants:
      - "email must be unique"
```

See the [DDD Guide](ddd-guide.md) for comprehensive documentation on entities, value objects, domain services, event sourcing, and authorization.

## Watch Mode

During development, watch for schema changes and auto-regenerate:

```bash
metaphor schema watch sapiens
```

## Next Steps

- [CLI Reference](cli-reference.md) -- All commands and flags
- [Schema Format](schema-format.md) -- Complete schema syntax reference
- [Rust Generation](generate-rust.md) -- All 38 server-side targets
- [Kotlin Generation](generate-kotlin.md) -- KMP mobile targets
- [Webapp Generation](generate-webapp.md) -- TypeScript + React targets
- [DDD Guide](ddd-guide.md) -- Domain-Driven Design features
