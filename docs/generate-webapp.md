# Webapp Code Generation (TypeScript + React)

Deep-dive into the `metaphor schema generate:webapp` pipeline. This generates TypeScript and React code for web applications, producing React Query hooks, Zod validation schemas, form components, CRUD pages, and full Clean Architecture layers.

## Quick Start

```bash
# Generate all webapp targets
metaphor schema generate:webapp sapiens

# Generate only hooks and forms
metaphor schema generate:webapp sapiens --target hooks,forms

# Generate for a single entity
metaphor schema generate:webapp sapiens --entity Customer

# Preview output
metaphor schema generate:webapp sapiens --dry-run
```

---

## Generation Targets

### Base Targets

These targets work with proto-generated types and basic schema definitions:

| Target | Aliases | Description |
|--------|---------|-------------|
| `all` | - | Generate all targets (default) |
| `hooks` | - | React Query hooks (queries and mutations) |
| `schemas` | `zod` | Zod validation schemas |
| `forms` | - | React form components |
| `pages` | `crud` | CRUD page components (list, create, edit, detail) |
| `types` | - | TypeScript type definitions from proto |
| `routing` | `routes`, `nav` | Routing and navigation configuration |

### Enhanced Targets

These targets use YAML schemas for richer code generation:

| Target | Aliases | Description |
|--------|---------|-------------|
| `workflows` | `workflow` | Workflow UI components (step wizards, progress) |
| `state-machines` | `state-machine`, `states` | State machine UI (status badges, transitions) |
| `enhanced-crud` | `enhanced` | Enhanced CRUD with field-level customization |

### Clean Architecture Layers

Full architectural layers driven by DDD schema definitions:

| Target | Aliases | Description |
|--------|---------|-------------|
| `domain` | `ddd` | Entity types, Zod schemas, domain services, specifications, commands, queries, events |
| `presentation` | `ui` | Forms, tables, pages, detail views |
| `application` | `app`, `usecases` | Use cases, application services |
| `infrastructure` | `infra`, `api` | API clients, repository implementations |

---

## Architecture

When using `all` or the Clean Architecture layer targets, the generated code follows this structure:

```
apps/webapp/src/
  └── modules/
      └── {module}/
          ├── domain/
          │   ├── entities/       # TypeScript interfaces and types
          │   ├── validators/     # Zod validation schemas
          │   ├── services/       # Domain service interfaces
          │   ├── specifications/ # Business rule specifications
          │   ├── commands/       # Command objects
          │   ├── queries/        # Query objects
          │   └── events/         # Domain event types
          ├── application/
          │   ├── usecases/       # Use case implementations
          │   └── services/       # Application service implementations
          ├── presentation/
          │   ├── forms/          # React form components
          │   ├── tables/         # Data table components
          │   ├── pages/          # Page components (list, create, edit, detail)
          │   └── detail/         # Detail view components
          ├── infrastructure/
          │   ├── api/            # API client implementations
          │   └── repositories/   # Repository implementations
          ├── hooks/              # React Query hooks
          ├── types/              # Shared TypeScript types
          ├── workflows/          # Workflow UI components
          ├── state-machines/     # State machine UI components
          └── routing/            # Route definitions
```

---

## Base vs Enhanced Targets

| Category | Source Data | Targets |
|----------|-----------|---------|
| **Base** | Proto files + basic YAML | `hooks`, `schemas`, `forms`, `pages`, `types`, `routing` |
| **Enhanced** | Full YAML schemas (DDD) | `workflows`, `state-machines`, `enhanced-crud`, `domain`, `presentation`, `application`, `infrastructure` |

Base targets can work with minimal schema definitions. Enhanced targets leverage DDD features like state machines, workflows, entities, value objects, and domain services to generate richer, more complete code.

---

## Output Directory

Default: `apps/webapp/src/`

Override with `--output`:

```bash
metaphor schema generate:webapp sapiens --output ./frontend/src/
```

Each target writes to its own subdirectory (see the `dir_name` column):

| Target | Output Directory |
|--------|-----------------|
| `hooks` | `hooks/` |
| `schemas` | `validators/` |
| `forms` | `components/` |
| `pages` | `pages/` |
| `types` | `types/` |
| `workflows` | `workflows/` |
| `state-machines` | `state-machines/` |
| `routing` | `routing/` |
| `enhanced-crud` | `enhanced/` |
| `domain` | `domain/` |
| `presentation` | `presentation/` |
| `application` | `application/` |
| `infrastructure` | `infrastructure/` |

---

## Entity Filtering

Use `--entity` to generate code for a single entity instead of all entities in the module:

```bash
# Only generate Customer-related code
metaphor schema generate:webapp sapiens --entity Customer
```

This is useful for:
- Regenerating a single entity after schema changes
- Reducing generation time during development
- Debugging generation for a specific model

---

## Generated Code Features

### React Query Hooks

- `useList{Entity}` -- Paginated list query
- `useGet{Entity}` -- Single entity query by ID
- `useCreate{Entity}` -- Create mutation
- `useUpdate{Entity}` -- Update mutation
- `useDelete{Entity}` -- Delete mutation
- Automatic cache invalidation
- Optimistic updates

### Zod Validation Schemas

- `create{Entity}Schema` -- Validation for create forms
- `update{Entity}Schema` -- Validation for edit forms
- Type-safe form validation
- Field-level error messages

### Form Components

- Auto-generated form fields based on schema types
- Field type mapping (text input, number input, select, date picker, etc.)
- Validation integration with Zod schemas
- Create and edit form variants

### CRUD Pages

- **List page** -- Data table with pagination, sorting, filtering
- **Create page** -- Form with validation
- **Edit page** -- Pre-populated form with validation
- **Detail page** -- Read-only entity view with relations

### Workflow UI

- Step wizard components
- Progress indicators
- State transition buttons
- Conditional step rendering

### State Machine UI

- Status badge components
- Available transition buttons
- Role-based action visibility
- State history display

---

## Options Reference

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target`, `-t` | string | `all` | Comma-separated targets |
| `--entity` | string | - | Generate for a specific entity only |
| `--output`, `-o` | path | `apps/webapp/src/` | Output directory |
| `--dry-run` | flag | - | Preview without writing files |
| `--force`, `-f` | flag | - | Overwrite existing files |

---

## Practical Examples

### Full Generation

```bash
metaphor schema generate:webapp sapiens
```

### Only API Integration Layer

Generate hooks and types for connecting to the backend:

```bash
metaphor schema generate:webapp sapiens --target hooks,types,schemas
```

### Only UI Components

Generate forms and pages:

```bash
metaphor schema generate:webapp sapiens --target forms,pages
```

### Clean Architecture Domain Layer

Generate the full DDD domain layer:

```bash
metaphor schema generate:webapp sapiens --target domain
```

### Single Entity Regeneration

After modifying the Order schema:

```bash
metaphor schema generate:webapp sapiens --entity Order --force
```

### Dry Run for CI

Preview what would be generated:

```bash
metaphor schema generate:webapp sapiens --dry-run
```

### Multiple Target Groups

Generate Clean Architecture layers:

```bash
metaphor schema generate:webapp sapiens --target domain,application,presentation,infrastructure
```
