# Domain-Driven Design Guide

This guide covers the DDD extensions available in YAML schema files. These features enhance generated code beyond basic CRUD by adding rich domain behavior, event sourcing, and fine-grained access control.

> DDD extensions are only available in the YAML format (`*.model.yaml`). The legacy DSL does not support these features.

## Overview

DDD extensions are defined alongside standard `models` and `enums` in your `*.model.yaml` files:

| Section | Purpose |
|---------|---------|
| `entities` | Enhanced models with methods, invariants, and trait implementations |
| `value_objects` | Composite or wrapper types with domain-specific validation |
| `domain_services` | Stateless/stateful services with dependencies and methods |
| `event_sourced` | Event sourcing configuration with snapshots |
| `authorization` | RBAC permissions, roles, and ABAC policies |

---

## Entities

Entities extend a `model` with behavior. They bridge the gap between a plain data struct and a rich domain object.

```yaml
entities:
  User:
    model: User                      # References a model defined in `models:`
    implements:                      # Traits the entity implements
      - Auditable
      - SoftDeletable
    value_objects:                   # Map fields to value object types
      email: Email
    methods:
      - name: verify_email
        mutates: true                # Method modifies entity state
        returns: "Result<(), UserError>"
        description: "Marks the user's email as verified"
      - name: can_login
        returns: bool
        description: "Checks if user is allowed to login"
      - name: suspend
        mutates: true
        async: true                  # Async method
        returns: "Result<(), UserError>"
      - name: reactivate
        mutates: true
        params:                      # Method parameters
          reason: String
        returns: "Result<(), UserError>"
    invariants:                      # Business rules as documentation
      - "email must be unique"
      - "password_hash must be at least 60 characters"
      - "status cannot be deleted if user has active sessions"
```

### Entity Properties

| Property | Type | Description |
|----------|------|-------------|
| `model` | string | Name of the model this entity wraps (must exist in `models:`) |
| `implements` | list | Traits: `Auditable`, `SoftDeletable`, custom traits |
| `value_objects` | map | Maps model fields to value object types |
| `methods` | list | Entity methods (see below) |
| `invariants` | list | Business rule descriptions (used in documentation and validation) |

### Method Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `name` | string | required | Method name (snake_case) |
| `mutates` | bool | `false` | Whether the method modifies entity state (`&mut self` vs `&self`) |
| `async` | bool | `false` | Whether the method is async |
| `params` | map | `{}` | Named parameters with types |
| `returns` | string | `()` | Return type |
| `description` | string | - | Documentation string |

---

## Value Objects

Value objects are domain-specific types that carry validation and behavior. There are two kinds:

### Composite Value Objects

Multi-field types with their own methods:

```yaml
value_objects:
  Money:
    fields:
      amount:
        type: decimal
        required: true
      currency:
        type: string
        required: true
    methods:
      - name: add
        params:
          other: Money
        returns: "Result<Money, Error>"
      - name: is_positive
        returns: bool
        const: true                  # Const method (&self, no side effects)

  Address:
    fields:
      street:
        type: string
        required: true
      city:
        type: string
        required: true
      postal_code:
        type: string
        required: true
      country:
        type: string
        required: true
    methods:
      - name: format
        returns: String
```

### Wrapper Value Objects

Single-value newtypes with validation:

```yaml
value_objects:
  Email:
    inner_type: String               # Wraps a single value
    validation: email_format          # Built-in validation rule
    methods:
      - name: domain
        returns: "&str"
```

### Composite vs Wrapper

| Property | Composite | Wrapper |
|----------|-----------|---------|
| `fields` | Required (map of fields) | Not used |
| `inner_type` | Not used | Required (the wrapped type) |
| `validation` | Not used | Optional validation rule name |
| `methods` | Optional | Optional |

### Linking Value Objects to Entities

Map entity fields to value object types in the `value_objects` section of an entity:

```yaml
entities:
  Order:
    model: Order
    value_objects:
      total: Money                   # Order.total field uses Money VO
      shipping_address: Address      # Order.shipping_address uses Address VO
      billing_address: Address
```

This tells the generators to use the value object type instead of the raw field type, enabling type-safe domain modeling.

---

## Domain Services

Domain services encapsulate business logic that doesn't belong to a single entity. They can declare dependencies on repositories and other services.

```yaml
domain_services:
  OrderService:
    stateless: false                 # Whether the service holds state
    dependencies:
      - OrderRepository
      - CustomerRepository
      - PaymentService
      - InventoryService
    methods:
      - name: place_order
        async: true
        params:
          customer_id: Uuid
          items: "Vec<OrderItem>"
        returns: "Result<Order, OrderError>"
      - name: process_payment
        async: true
        params:
          order_id: Uuid
          payment_method: PaymentMethod
        returns: "Result<PaymentResult, OrderError>"

  CustomerService:
    stateless: false
    dependencies:
      - CustomerRepository
      - EmailService
    methods:
      - name: register
        async: true
        params:
          email: Email
          name: String
        returns: "Result<Customer, CustomerError>"
      - name: send_welcome_email
        async: true
        params:
          customer_id: Uuid
        returns: "Result<(), CustomerError>"
```

### Service Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `stateless` | bool | `true` | Whether the service is stateless (affects struct generation) |
| `dependencies` | list | `[]` | Repository and service dependencies (injected via constructor) |
| `methods` | list | `[]` | Service methods (same format as entity methods) |

---

## Event Sourcing

Configure event sourcing for aggregate roots. This affects the `event-store`, `events`, and `projection` generators.

```yaml
event_sourced:
  Order:
    events:
      - OrderPlaced
      - OrderConfirmed
      - OrderShipped
      - OrderDelivered
      - OrderCancelled
    snapshot:
      enabled: true
      every_n_events: 50             # Take snapshot every 50 events

  Customer:
    events:
      - CustomerRegistered
      - CustomerActivated
      - CustomerSuspended
    snapshot:
      enabled: true
      every_n_events: 100
```

### Event Sourcing Properties

| Property | Type | Description |
|----------|------|-------------|
| `events` | list | Domain event names for this aggregate |
| `snapshot.enabled` | bool | Enable snapshot storage for faster rebuilds |
| `snapshot.every_n_events` | int | Snapshot frequency |

### Impact on Generated Code

| Generator | What Changes |
|-----------|-------------|
| `events` | Generates event structs for each listed event |
| `event-store` | Generates event store with append/load/snapshot support |
| `projection` | Generates CQRS read model projections from events |
| `rust` | Adds event application methods to entity structs |

---

## Authorization

Define RBAC (Role-Based Access Control) and ABAC (Attribute-Based Access Control) configurations.

### Permissions

Define resource-action permission pairs:

```yaml
authorization:
  permissions:
    users:
      - read
      - create
      - update
      - delete
      - manage
    documents:
      - read
      - create
      - update
      - delete
      - share
      - archive
    reports:
      - read
      - create
      - export
```

Permissions are referenced as `resource.action` (e.g., `users.read`, `documents.share`).

### Roles

Define roles with permissions, hierarchy levels, and inheritance:

```yaml
  roles:
    guest:
      permissions:
        - "documents.read"
      level: 0
      description: "Anonymous or unauthenticated access"

    user:
      permissions:
        - "users.read"
        - "documents.read"
        - "documents.create"
      level: 10
      inherits: guest              # Inherits all guest permissions

    editor:
      permissions:
        - "documents.update"
        - "documents.share"
      level: 30
      inherits: user

    admin:
      permissions:
        - "users.*"                # Wildcard: all user permissions
        - "documents.*"
        - "reports.*"
      level: 80

    superadmin:
      permissions:
        - "*"                      # All permissions on all resources
      level: 100
```

### Role Properties

| Property | Type | Description |
|----------|------|-------------|
| `permissions` | list | Permission strings (`resource.action` or wildcards) |
| `level` | int | Numeric hierarchy level (higher = more privileged) |
| `inherits` | string | Parent role to inherit permissions from |
| `description` | string | Human-readable role description |

### ABAC Policies

Define attribute-based policies for fine-grained access control:

```yaml
  policies:
    # Owner-based access
    document_owner:
      type: any                    # Match ANY rule (OR logic)
      description: "Document owners have full access"
      rules:
        - owner:
            resource: Document
            field: owner_id        # Resource field to check
            actor_field: id        # Actor field to compare against

    # Department-based access
    department_access:
      type: all                    # Match ALL rules (AND logic)
      rules:
        - permission: "documents.read"
        - condition: "actor.department_id == resource.department_id"

    # Role-based restriction
    confidential_access:
      type: all
      rules:
        - role: manager
        - condition: "resource.classification in ['Confidential', 'Secret']"

    # Time-based access
    business_hours_only:
      type: all
      rules:
        - permission: "documents.delete"
        - condition: "current_hour >= 9 && current_hour <= 17"
        - condition: "current_day_of_week >= 1 && current_day_of_week <= 5"

    # MFA requirement
    sensitive_operations:
      type: all
      rules:
        - permission: "users.delete"
        - condition: "actor.mfa_verified == true"
```

### Policy Properties

| Property | Type | Description |
|----------|------|-------------|
| `type` | `any` / `all` | Match logic: `any` = OR (at least one rule), `all` = AND (all rules) |
| `description` | string | Human-readable description |
| `rules` | list | Rule conditions (see below) |

### Rule Types

| Rule | Description | Example |
|------|-------------|---------|
| `owner` | Check resource ownership | `owner: { resource: X, field: owner_id }` |
| `permission` | Require a specific permission | `permission: "docs.read"` |
| `role` | Require a specific role | `role: manager` |
| `condition` | Arbitrary expression | `condition: "actor.age >= 18"` |

---

## Complete Example

Here is a complete DDD schema combining all features:

```yaml
# order.model.yaml

models:
  - name: Order
    collection: orders
    fields:
      id:
        type: uuid
        primary_key: true
      customer_id:
        type: uuid
        required: true
      status:
        type: OrderStatus
        required: true
      total:
        type: decimal
      shipping_address:
        type: json
      created_at:
        type: timestamp
        auto: true

enums:
  - name: OrderStatus
    variants: [Pending, Confirmed, Shipped, Delivered, Cancelled]

value_objects:
  Money:
    fields:
      amount: { type: decimal, required: true }
      currency: { type: string, required: true }
    methods:
      - name: add
        params: { other: Money }
        returns: "Result<Money, Error>"

  Email:
    inner_type: String
    validation: email_format

entities:
  Order:
    model: Order
    implements: [Auditable]
    value_objects:
      total: Money
    methods:
      - name: confirm
        mutates: true
        returns: "Result<(), OrderError>"
      - name: cancel
        mutates: true
        params: { reason: String }
        returns: "Result<(), OrderError>"
    invariants:
      - "total must be positive"

domain_services:
  OrderService:
    dependencies: [OrderRepository, PaymentService]
    methods:
      - name: place_order
        async: true
        params: { customer_id: Uuid, items: "Vec<OrderItem>" }
        returns: "Result<Order, OrderError>"

event_sourced:
  Order:
    events: [OrderPlaced, OrderConfirmed, OrderShipped, OrderCancelled]
    snapshot: { enabled: true, every_n_events: 50 }

authorization:
  permissions:
    orders: [read, create, update, cancel]
  roles:
    customer:
      permissions: ["orders.read", "orders.create"]
      level: 10
    admin:
      permissions: ["orders.*"]
      level: 80
  policies:
    order_owner:
      type: any
      rules:
        - owner: { resource: Order, field: customer_id }
```

## Which Generators Use DDD Features

| DDD Feature | Consumed By Generators |
|-------------|----------------------|
| `entities` | `rust`, `domain-service`, `handler`, `grpc`, `openapi`, `dto` |
| `value_objects` | `value-object`, `rust`, `dto` |
| `domain_services` | `domain-service`, `service`, `module` |
| `event_sourced` | `events`, `event-store`, `projection`, `event-subscription` |
| `authorization` | `auth`, `handler`, `grpc` |
