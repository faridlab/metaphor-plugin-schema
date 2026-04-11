//! YAML-to-AST conversion: impl blocks for all YamlXxx types

use crate::ast::model::{
    Attribute, AttributeValue, EnumDef, EnumVariant, Field, Index, IndexType, Model, Relation,
    RelationType,
    Entity, EntityMethod, ValueObject, ValueObjectMethod,
    DomainService, ServiceDependency, ServiceMethod,
    EventSourcedConfig, SnapshotConfig,
    UseCase,
    DomainEvent, EventStorage, EventField,
    Projection, ProjectionStorage, SourceEvent, ProjectionField, ProjectionIndex,
    AppService, AppServiceMethod,
    Handler, HandlerRetryPolicy, Subscription,
    Integration, IntegrationMethod,
    Presentation, HttpConfig, RouteGroup, Endpoint, GrpcConfig, GrpcService, GrpcMethod,
    Dto, DtoField, ComputedDtoField,
    Versioning,
    RepositoryTrait, TraitMethod,
};
use crate::ast::types::{TypeRef, PrimitiveType};
use crate::ast::authorization::{
    AuthorizationConfig, RoleDefinition, PolicyDefinition, PolicyType, PolicyRule,
    ResourcePolicy, ResourcePolicyRule, AbacAttributes, AbacPolicy,
};
use crate::ast::hook::{
    Action, ActionKind, ActionType, ComputedField, FieldRestriction, Permission, PermissionAction,
    Rule, RuleWhen, State, StateMachine, Transition, Trigger, TriggerEvent, Hook,
};
use crate::ast::workflow::{
    Workflow, WorkflowTrigger, WorkflowConfig, TransactionMode, Step, StepType,
    ActionStep, WaitStep, WaitEvent, ConditionStep, ParallelStep, LoopStep,
    SubprocessStep, HumanTaskStep, TransitionStep, TerminalStep,
    TransactionGroupStep, CompensationStep, CompensationType,
    RetryPolicy, BackoffStrategy, TimeoutAction, ContextVariable,
    StepOutcome, StepFailure, ConditionBranch, ParallelBranch,
    JoinStrategy, TaskConfig, TaskForm, TaskFormField, TaskTimeoutAction,
    TaskTimeoutActionType, TerminalStatus, EmitConfig, LogConfig, LogLevel,
    CompensationAction, WorkflowHandler,
};
use crate::ast::expressions::Expression;
use indexmap::IndexMap;

use super::types::*;
use super::helpers::*;

// ============================================================================
// Conversion to AST
// ============================================================================

impl YamlModelSchema {
    /// Convert to AST models
    pub fn into_models(self) -> Vec<Model> {
        // Convert file-level types to IndexMap format
        let file_types = self.types_as_indexmap();
        self.models
            .into_iter()
            .map(|m| m.into_model_with_context(&IndexMap::new(), &file_types))
            .collect()
    }

    /// Convert to AST models with shared types context
    pub fn into_models_with_context(
        self,
        shared_types: &IndexMap<String, IndexMap<String, YamlField>>,
    ) -> Vec<Model> {
        // Convert file-level types to IndexMap format
        let file_types = self.types_as_indexmap();
        self.models
            .into_iter()
            .map(|m| m.into_model_with_context(shared_types, &file_types))
            .collect()
    }

    /// Convert to AST enums
    pub fn into_enums(self) -> Vec<EnumDef> {
        self.enums.into_iter().map(|e| e.into_enum()).collect()
    }

    /// Convert file-level types (Vec<YamlTypeDef>) to IndexMap format
    fn types_as_indexmap(&self) -> IndexMap<String, IndexMap<String, YamlField>> {
        let mut result = IndexMap::new();
        for type_def in &self.types {
            result.insert(type_def.name.clone(), type_def.fields.clone());
        }
        result
    }

    // ==========================================================================
    // DDD & AUTHORIZATION CONVERSION METHODS
    // ==========================================================================

    /// Convert to AST entities
    pub fn into_entities(self) -> Vec<Entity> {
        self.entities
            .into_iter()
            .map(|(name, e)| e.into_entity(name))
            .collect()
    }

    /// Convert to AST value objects
    pub fn into_value_objects(self) -> Vec<ValueObject> {
        self.value_objects
            .into_iter()
            .map(|(name, vo)| vo.into_value_object(name))
            .collect()
    }

    /// Convert to AST domain services
    pub fn into_domain_services(self) -> Vec<DomainService> {
        self.domain_services
            .into_iter()
            .map(|(name, ds)| ds.into_domain_service(name))
            .collect()
    }

    /// Convert to AST event sourced configs
    pub fn into_event_sourced(self) -> Vec<EventSourcedConfig> {
        self.event_sourced
            .into_iter()
            .map(|(name, es)| es.into_event_sourced(name))
            .collect()
    }

    /// Convert to AST authorization config
    pub fn into_authorization(self) -> Option<AuthorizationConfig> {
        self.authorization.map(|a| a.into_authorization())
    }
}

impl YamlModel {
    /// Convert to AST Model
    pub fn into_model(self) -> Model {
        self.into_model_with_context(&IndexMap::new(), &IndexMap::new())
    }

    /// Convert to AST Model with shared types context
    /// - shared_types: Module-level shared types from index.model.yaml
    /// - local_types: Model-local types defined in the model's types section
    pub fn into_model_with_context(
        self,
        shared_types: &IndexMap<String, IndexMap<String, YamlField>>,
        local_types: &IndexMap<String, IndexMap<String, YamlField>>,
    ) -> Model {
        let mut model = Model::new(&self.name);
        model.collection = self.collection.clone();

        // Add soft_delete attribute if enabled in YAML schema
        if self.soft_delete == Some(true) {
            model.attributes.push(Attribute::new("soft_delete"));
        }

        // Combine local types with shared types for lookup (local takes precedence)
        let mut all_types = shared_types.clone();
        for (name, fields) in local_types {
            all_types.insert(name.clone(), fields.clone());
        }
        // Also add model-local types from self.types
        for (name, fields) in &self.types {
            all_types.insert(name.clone(), fields.clone());
        }

        // 1. First, inject extended fields from shared types (these become table columns)
        for type_name in &self.extends {
            if let Some(type_fields) = all_types.get(type_name) {
                for (field_name, yaml_field) in type_fields {
                    // Check if field already exists (explicit field takes precedence)
                    if !self.fields.contains_key(field_name) {
                        let mut field = yaml_field.clone().into_field(field_name.clone());
                        // Mark field as inherited from shared type
                        field.attributes.push(Attribute::new("inherited").with_arg(
                            AttributeValue::String(type_name.clone())
                        ));
                        model.fields.push(field);
                    }
                }
            }
        }

        // 2. Convert model's own fields
        for (name, yaml_field) in self.fields {
            // Check if field type references a shared/local type (becomes JSONB)
            let field_type_name = yaml_field.get_type_name();
            if let Some(type_name) = &field_type_name {
                if all_types.contains_key(type_name) {
                    // This is a shared/local type - convert to JSONB field
                    let field = yaml_field.into_jsonb_field(name, type_name, &all_types);
                    model.fields.push(field);
                    continue;
                }
            }
            // Regular field conversion
            model.fields.push(yaml_field.into_field(name));
        }

        // Convert relations
        for (name, yaml_rel) in self.relations {
            model.relations.push(yaml_rel.into_relation(name));
        }

        // Auto-create relations from fields with @foreign_key attribute
        // This handles cases where a field has @foreign_key but no explicit relation defined
        let mut existing_relation_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for rel in &model.relations {
            existing_relation_names.insert(rel.name.clone());
        }

        for field in &model.fields {
            // Check if field has @foreign_key attribute
            if let Some(fk_attr) = field.attributes.iter().find(|a| a.name == "foreign_key") {
                // Parse the foreign key value (e.g., "core.Country.id" or "Country.id")
                if let Some((_, fk_value)) = fk_attr.args.first() {
                    if let AttributeValue::String(fk_str) = fk_value {
                        // Parse "Module.Entity.column" or "Entity.column"
                        let parts: Vec<&str> = fk_str.split('.').collect();
                        if parts.len() >= 2 {
                            // Get entity name (e.g., "Country" from "core.Country.id")
                            let entity_name = if parts.len() == 3 {
                                parts[1] // core.Country.id -> Country
                            } else {
                                parts[0] // Country.id -> Country
                            };

                            // Get module name if present (e.g., "core" from "core.Country.id")
                            let module_name = if parts.len() == 3 {
                                Some(parts[0].to_string())
                            } else {
                                None
                            };

                            // Create relation name from field name (strip _id suffix if present)
                            let rel_name = field.name.strip_suffix("_id").unwrap_or(&field.name);

                            // Skip if relation already exists
                            if existing_relation_names.contains(rel_name) {
                                continue;
                            }

                            // Create TypeRef for the target
                            let target = if let Some(mod_name) = module_name {
                                // Cross-module reference: TypeRef::ModuleRef
                                TypeRef::ModuleRef {
                                    module: mod_name,
                                    name: entity_name.to_string(),
                                }
                            } else {
                                // Same-module reference: TypeRef::Custom
                                TypeRef::Custom(entity_name.to_string())
                            };

                            // Determine relation type based on field type
                            // If field is uuid/single value, it's a @one (belongs to)
                            // Arrays would be @many (has many), but FK fields are typically single values
                            let relation_type = RelationType::One;

                            // Create the relation
                            let relation = Relation {
                                name: rel_name.to_string(),
                                target,
                                relation_type,
                                attributes: vec![
                                    Attribute::new("foreign_key").with_arg(AttributeValue::String(field.name.clone())),
                                ],
                                ..Default::default()
                            };

                            model.relations.push(relation);
                            existing_relation_names.insert(rel_name.to_string());
                        }
                    }
                }
            }
        }

        // Convert indexes
        for yaml_idx in self.indexes {
            model.indexes.push(yaml_idx.into_index());
        }

        // Propagate per-model generator config (disabled blacklist + enabled whitelist)
        if let Some(gen_cfg) = self.generators {
            if let Some(disabled) = gen_cfg.disabled {
                model.disabled_generators = disabled;
            }
            if let Some(enabled) = gen_cfg.enabled {
                model.enabled_generators = enabled;
            }
        }

        // Auto-inject metadata field for models with soft_delete but no explicit @audit_metadata field.
        // This ensures the SQL generator creates the metadata JSONB column and audit triggers
        // even when the model doesn't explicitly extend [Metadata] or declare a metadata field.
        let has_audit_metadata = model.fields.iter().any(|f| f.has_attribute("audit_metadata"));
        let has_soft_delete = model.has_attribute("soft_delete");
        if has_soft_delete && !has_audit_metadata {
            use crate::ast::{Attribute, PrimitiveType, TypeRef};
            let mut metadata_field = Field::new("metadata", TypeRef::Primitive(PrimitiveType::Json));
            metadata_field.attributes.push(Attribute::new("audit_metadata"));
            metadata_field.attributes.push(
                Attribute::new("default").with_arg(AttributeValue::String("{}".to_string()))
            );
            model.fields.push(metadata_field);
        }

        model
    }
}

impl YamlField {
    /// Get the type name as a string (for checking against shared types)
    pub fn get_type_name(&self) -> Option<String> {
        let type_str = match self {
            YamlField::Simple(s) => s.as_str(),
            YamlField::Full { field_type, .. } => field_type.as_str(),
        };

        // Strip optional/array markers to get base type name
        let type_str = type_str.trim();
        let type_str = type_str.strip_suffix('?').unwrap_or(type_str);
        let type_str = type_str.strip_suffix("[]").unwrap_or(type_str);

        // Check if it's a primitive type - if so, return None
        if PrimitiveType::from_str(type_str).is_some() {
            return None;
        }

        // Return the custom type name
        Some(type_str.to_string())
    }

    /// Convert to AST Field as a JSONB type (for shared/local type references)
    /// This generates a JSONB column with validation attributes from the type definition
    pub fn into_jsonb_field(
        self,
        name: String,
        type_name: &str,
        all_types: &IndexMap<String, IndexMap<String, YamlField>>,
    ) -> Field {
        let (is_optional, is_array) = match &self {
            YamlField::Simple(s) => (s.ends_with('?'), s.ends_with("[]")),
            YamlField::Full { field_type, .. } => {
                (field_type.ends_with('?'), field_type.ends_with("[]"))
            }
        };

        // Create JSONB type reference
        let json_type = TypeRef::Primitive(PrimitiveType::Json);
        let type_ref = if is_array {
            TypeRef::Array(Box::new(json_type))
        } else if is_optional {
            TypeRef::Optional(Box::new(json_type))
        } else {
            json_type
        };

        let mut attrs = Vec::new();

        // Mark this field as a structured JSONB type with the type name
        attrs.push(
            Attribute::new("jsonb_type")
                .with_arg(AttributeValue::String(type_name.to_string())),
        );

        // Collect validation attributes from the type definition fields
        // Special handling for Metadata type: also copy @audit_metadata attribute
        if let Some(type_fields) = all_types.get(type_name) {
            // Check if this is the special Metadata type
            if type_name == AUDIT_METADATA_TYPE_NAME {
                // Check if it has the single "metadata" field with @audit_metadata (resolved from shared types)
                if type_fields.len() == 1 && type_fields.contains_key("metadata") {
                    if let Some(YamlField::Full { attributes, .. }) = type_fields.get("metadata") {
                        // Copy @audit_metadata and other special attributes from the resolved type
                        for attr_str in attributes {
                            if attr_str == "@audit_metadata" || attr_str.starts_with("@default(") {
                                if let Some(attr) = parse_attribute_string(attr_str) {
                                    attrs.push(attr);
                                }
                            }
                        }
                    }
                }
                // For Metadata type, always add @audit_metadata attribute for timestamp management
                if !attrs.iter().any(|a| a.name == "audit_metadata") {
                    attrs.push(Attribute::new("audit_metadata"));
                }
                // For Metadata type, generate schema with actual audit fields (created_at, updated_at, etc.)
                // instead of the wrapper "metadata" field
                let schema_json = r#"{"created_at":{"type":"datetime","optional":false},"updated_at":{"type":"datetime","optional":false},"deleted_at":{"type":"datetime","optional":true},"created_by":{"type":"uuid","optional":true},"updated_by":{"type":"uuid","optional":true},"deleted_by":{"type":"uuid","optional":true}}"#;
                attrs.push(
                    Attribute::new("jsonb_schema")
                        .with_arg(AttributeValue::String(schema_json.to_string())),
                );
            } else {
                // Store field schema for validation generation
                let schema_json = serialize_type_fields_to_json(type_fields);
                attrs.push(
                    Attribute::new("jsonb_schema")
                        .with_arg(AttributeValue::String(schema_json)),
                );
            }
        } else {
            // Type not found in shared/local types - but if it's Metadata, still add audit_metadata
            if type_name == AUDIT_METADATA_TYPE_NAME {
                attrs.push(Attribute::new("audit_metadata"));
                // Add the schema for Metadata type
                let schema_json = r#"{"created_at":{"type":"datetime","optional":false},"updated_at":{"type":"datetime","optional":false},"deleted_at":{"type":"datetime","optional":true},"created_by":{"type":"uuid","optional":true},"updated_by":{"type":"uuid","optional":true},"deleted_by":{"type":"uuid","optional":true}}"#;
                attrs.push(
                    Attribute::new("jsonb_schema")
                        .with_arg(AttributeValue::String(schema_json.to_string())),
                );
            }
        }

        // Add any additional attributes from the field definition itself
        if let YamlField::Full { attributes, description, .. } = &self {
            for attr_str in attributes {
                if let Some(attr) = parse_attribute_string(attr_str) {
                    attrs.push(attr);
                }
            }
            if let Some(desc) = description {
                attrs.push(Attribute::new("description").with_arg(AttributeValue::String(desc.clone())));
            }
        }

        let _attr_names: Vec<&str> = attrs.iter().map(|a| a.name.as_str()).collect();

        let mut field = Field::new(name, type_ref);
        field.attributes = attrs;
        field
    }

    /// Convert to AST Field
    pub fn into_field(self, name: String) -> Field {
        match self {
            YamlField::Simple(type_str) => {
                let (type_ref, attrs) = parse_type_string(&type_str);
                let mut field = Field::new(name, type_ref);
                field.attributes = attrs;
                field
            }
            YamlField::Full {
                field_type,
                attributes,
                description,
            } => {
                let (type_ref, mut attrs) = parse_type_string(&field_type);
                // Parse additional attributes
                for attr_str in attributes {
                    if let Some(attr) = parse_attribute_string(&attr_str) {
                        attrs.push(attr);
                    }
                }
                // Add description if present
                if let Some(desc) = description {
                    attrs.push(Attribute::new("description").with_arg(AttributeValue::String(desc)));
                }
                let mut field = Field::new(name, type_ref);
                field.attributes = attrs;
                field
            }
        }
    }
}

impl YamlRelation {
    /// Convert to AST Relation
    pub fn into_relation(self, name: String) -> Relation {
        let (target, relation_type) = parse_relation_type(&self.target_type);
        let mut attrs = Vec::new();

        for attr_str in self.attributes {
            if let Some(attr) = parse_attribute_string(&attr_str) {
                attrs.push(attr);
            }
        }

        Relation {
            name,
            target,
            relation_type,
            attributes: attrs,
            ..Default::default()
        }
    }
}

impl YamlIndex {
    /// Convert to AST Index
    pub fn into_index(self) -> Index {
        let index_type = match self.index_type.to_lowercase().as_str() {
            "unique" => IndexType::Unique,
            "fulltext" => IndexType::Fulltext,
            "gin" => IndexType::Gin,
            _ => IndexType::Index,
        };

        let mut attrs = Vec::new();
        if let Some(name) = &self.name {
            attrs.push(Attribute::new("name").with_arg(AttributeValue::String(name.clone())));
        }
        if let Some(where_clause) = &self.where_clause {
            attrs.push(Attribute::new("where").with_arg(AttributeValue::String(where_clause.clone())));
        }

        Index {
            index_type,
            fields: self.fields,
            name: self.name,
            attributes: attrs,
            ..Default::default()
        }
    }
}

impl YamlEnum {
    /// Convert to AST EnumDef
    pub fn into_enum(self) -> EnumDef {
        let mut enum_def = EnumDef::new(&self.name);

        for yaml_variant in self.variants {
            enum_def.variants.push(yaml_variant.into_variant());
        }

        enum_def
    }
}

impl YamlEnumVariant {
    /// Convert to AST EnumVariant
    pub fn into_variant(self) -> EnumVariant {
        match self {
            YamlEnumVariant::Simple(name) => EnumVariant {
                name,
                ..Default::default()
            },
            YamlEnumVariant::Full {
                name,
                value,
                description,
                default,
            } => {
                let mut attrs = Vec::new();
                if let Some(desc) = description {
                    attrs.push(Attribute::new("description").with_arg(AttributeValue::String(desc)));
                }
                if default == Some(true) {
                    attrs.push(Attribute::new("default"));
                }
                EnumVariant {
                    name,
                    value,
                    attributes: attrs,
                    ..Default::default()
                }
            }
        }
    }
}

impl YamlHookSchema {
    /// Convert to AST Hook
    pub fn into_hook(self) -> Hook {
        let mut hook = Hook::new(&self.name, &self.model);

        // Convert state machine
        if let Some(yaml_states) = self.states {
            hook.state_machine = Some(yaml_states.into_state_machine());
        }

        // Convert rules
        for (name, yaml_rule) in self.rules {
            hook.rules.push(yaml_rule.into_rule(name));
        }

        // Convert permissions
        for (role, yaml_perm) in self.permissions {
            hook.permissions.push(yaml_perm.into_permission(role));
        }

        // Convert triggers
        for (event, yaml_trigger) in self.triggers {
            hook.triggers.push(yaml_trigger.into_trigger(&event));
        }

        // Convert computed fields
        for (name, expr_str) in self.computed {
            hook.computed_fields.push(ComputedField {
                name,
                expression: Expression::Raw(expr_str),
                ..Default::default()
            });
        }

        hook
    }
}

impl YamlStateMachine {
    /// Convert to AST StateMachine
    pub fn into_state_machine(self) -> StateMachine {
        let mut sm = StateMachine {
            field: self.field,
            ..Default::default()
        };

        // Convert states
        for (name, yaml_state) in self.values {
            sm.states.push(yaml_state.into_state(name));
        }

        // Convert transitions
        for (name, yaml_trans) in self.transitions {
            sm.transitions.push(yaml_trans.into_transition(name));
        }

        sm
    }
}

impl YamlState {
    /// Convert to AST State
    pub fn into_state(self, name: String) -> State {
        match self {
            YamlState::Simple(marker) => {
                let mut state = State::new(&name);
                if let Some(m) = marker {
                    if m.contains("initial") {
                        state.initial = true;
                    }
                    if m.contains("final") {
                        state.final_state = true;
                    }
                }
                state
            }
            YamlState::Full {
                initial,
                final_state,
                on_enter,
                on_exit,
            } => {
                let mut state = State::new(&name);
                state.initial = initial.unwrap_or(false);
                state.final_state = final_state.unwrap_or(false);
                state.on_enter = on_enter.into_iter().map(|a| a.into_action()).collect();
                state.on_exit = on_exit.into_iter().map(|a| a.into_action()).collect();
                state
            }
        }
    }
}

impl YamlTransition {
    /// Convert to AST Transition
    pub fn into_transition(self, name: String) -> Transition {
        Transition {
            name,
            from: self.from.into_vec(),
            to: self.to,
            allowed_roles: self.roles,
            guard: self.condition.map(Expression::Raw),
            ..Default::default()
        }
    }
}

impl YamlRule {
    /// Convert to AST Rule
    pub fn into_rule(self, name: String) -> Rule {
        let when = self
            .when
            .into_iter()
            .map(|w| match w.to_lowercase().as_str() {
                "create" => RuleWhen::Create,
                "update" => RuleWhen::Update,
                "delete" => RuleWhen::Delete,
                _ => RuleWhen::Always,
            })
            .collect();

        Rule {
            name,
            when,
            condition: Expression::Raw(self.condition),
            message: self.message,
            code: self.code,
            ..Default::default()
        }
    }
}

impl YamlPermission {
    /// Convert to AST Permission
    pub fn into_permission(self, role: String) -> Permission {
        let mut actions = Vec::new();

        for yaml_action in self.allow {
            actions.push(yaml_action.into_permission_action(true));
        }

        for yaml_action in self.deny {
            actions.push(yaml_action.into_permission_action(false));
        }

        Permission {
            role,
            actions,
            ..Default::default()
        }
    }
}

impl YamlPermissionAction {
    /// Convert to AST PermissionAction
    pub fn into_permission_action(self, allowed: bool) -> PermissionAction {
        match self {
            YamlPermissionAction::Simple(action_name) => PermissionAction {
                action: ActionType::from_str(&action_name),
                allowed,
                ..Default::default()
            },
            YamlPermissionAction::Full {
                action,
                only,
                except,
                condition,
            } => PermissionAction {
                action: ActionType::from_str(&action),
                allowed,
                fields: only
                    .map(FieldRestriction::Only)
                    .or(except.map(FieldRestriction::Except)),
                condition: condition.map(Expression::Raw),
                ..Default::default()
            },
        }
    }
}

impl YamlTrigger {
    /// Convert to AST Trigger
    pub fn into_trigger(self, event_name: &str) -> Trigger {
        let event = TriggerEvent::from_str(event_name).unwrap_or(TriggerEvent::AfterCreate);

        Trigger {
            event,
            actions: self.actions.into_iter().map(|a| a.into_action()).collect(),
            condition: self.condition.map(Expression::Raw),
            ..Default::default()
        }
    }
}

impl YamlAction {
    /// Convert to AST Action
    pub fn into_action(self) -> Action {
        match self {
            YamlAction::Simple(s) => {
                let (kind, args) = parse_action_string(&s);
                Action {
                    action_type: kind,
                    args,
                    ..Default::default()
                }
            }
            YamlAction::Full { action_type, params } => Action {
                action_type: ActionKind::from_str(&action_type),
                args: params
                    .into_iter()
                    .map(|(_, v)| Expression::Raw(format!("{:?}", v)))
                    .collect(),
                ..Default::default()
            },
        }
    }
}

// ============================================================================
// Workflow Conversion to AST (Multi-step Business Processes)
// ============================================================================

impl YamlWorkflowSchema {
    /// Convert to AST Workflow
    pub fn into_workflow(self) -> Workflow {
        let mut workflow = Workflow::new(&self.name);
        workflow.description = self.description;
        workflow.version = self.version;

        // Convert trigger
        if let Some(trigger) = self.trigger {
            workflow.trigger = trigger.into_trigger();
        }

        // Convert config
        if let Some(config) = self.config {
            workflow.config = config.into_config();
        }

        // Convert context
        for (name, value) in self.context {
            workflow.context.insert(name.clone(), ContextVariable {
                name,
                initial_value: Some(yaml_value_to_expr(value)),
                type_hint: None,
            });
        }

        // Convert steps
        workflow.steps = self.steps.into_iter().map(|s| s.into_step()).collect();

        // Convert handlers
        workflow.on_success = self.on_success.into_iter().map(|h| h.into_handler()).collect();
        workflow.on_failure = self.on_failure.into_iter().map(|h| h.into_handler()).collect();

        // Convert compensation
        workflow.compensation = self.compensation.into_iter().map(|c| c.into_compensation()).collect();

        workflow
    }
}

impl YamlWorkflowTrigger {
    /// Convert to AST WorkflowTrigger
    pub fn into_trigger(self) -> WorkflowTrigger {
        WorkflowTrigger {
            event: self.event,
            endpoint: self.endpoint,
            schedule: self.schedule,
            extract: self.extract.into_iter().collect(),
            ..Default::default()
        }
    }
}

impl YamlWorkflowConfig {
    /// Convert to AST WorkflowConfig
    pub fn into_config(self) -> WorkflowConfig {
        WorkflowConfig {
            timeout: self.timeout,
            transaction_mode: self.transaction_mode
                .map(|m| match m.to_lowercase().as_str() {
                    "atomic" => TransactionMode::Atomic,
                    "hybrid" => TransactionMode::Hybrid,
                    _ => TransactionMode::Saga,
                })
                .unwrap_or_default(),
            retry_policy: self.retry_policy.map(|r| r.into_policy()),
            on_timeout: self.on_timeout
                .map(|t| match t.to_lowercase().as_str() {
                    "compensate" => TimeoutAction::Compensate,
                    "continue" => TimeoutAction::Continue,
                    _ => TimeoutAction::Cancel,
                })
                .unwrap_or_default(),
            persistence: self.persistence.unwrap_or(true),
        }
    }
}

impl YamlRetryPolicy {
    /// Convert to AST RetryPolicy
    pub fn into_policy(self) -> RetryPolicy {
        RetryPolicy {
            max_attempts: self.max_attempts.unwrap_or(3),
            backoff: self.backoff.map(|b| b.into_backoff()).unwrap_or_default(),
        }
    }
}

impl YamlBackoff {
    /// Convert to AST BackoffStrategy
    pub fn into_backoff(self) -> BackoffStrategy {
        match self {
            YamlBackoff::Simple(s) => match s.to_lowercase().as_str() {
                "linear" => BackoffStrategy::Linear { initial: "1s".to_string() },
                "exponential" => BackoffStrategy::Exponential {
                    initial: "1s".to_string(),
                    max: "1m".to_string(),
                },
                _ => BackoffStrategy::Fixed(s),
            },
            YamlBackoff::Full { backoff_type, initial, max } => {
                match backoff_type.to_lowercase().as_str() {
                    "linear" => BackoffStrategy::Linear {
                        initial: initial.unwrap_or_else(|| "1s".to_string()),
                    },
                    "exponential" => BackoffStrategy::Exponential {
                        initial: initial.unwrap_or_else(|| "1s".to_string()),
                        max: max.unwrap_or_else(|| "1m".to_string()),
                    },
                    _ => BackoffStrategy::Fixed(initial.unwrap_or_else(|| "1s".to_string())),
                }
            }
        }
    }
}

impl YamlStep {
    /// Convert to AST Step
    pub fn into_step(self) -> Step {
        let step_type = self.determine_step_type();

        Step {
            name: self.name,
            step_type,
            condition: self.condition.map(Expression::Raw),
            on_success: self.on_success.map(|o| o.into_outcome()),
            on_failure: self.on_failure.map(|f| f.into_failure()),
            ..Default::default()
        }
    }

    fn determine_step_type(&self) -> StepType {
        // Check explicit type first
        if let Some(ref t) = self.step_type {
            match t.to_lowercase().as_str() {
                "action" => return StepType::Action(self.clone().into_action_step()),
                "wait" => return StepType::Wait(self.clone().into_wait_step()),
                "condition" => return StepType::Condition(self.clone().into_condition_step()),
                "parallel" => return StepType::Parallel(self.clone().into_parallel_step()),
                "loop" => return StepType::Loop(self.clone().into_loop_step()),
                "subprocess" => return StepType::Subprocess(self.clone().into_subprocess_step()),
                "human_task" => return StepType::HumanTask(self.clone().into_human_task_step()),
                "transition" => return StepType::Transition(self.clone().into_transition_step()),
                "transaction_group" => return StepType::TransactionGroup(self.clone().into_transaction_group()),
                "terminal" => return StepType::Terminal(self.clone().into_terminal_step()),
                _ => {}
            }
        }

        // Infer type from fields
        if self.action.is_some() {
            StepType::Action(self.clone().into_action_step())
        } else if self.wait_for.is_some() {
            StepType::Wait(self.clone().into_wait_step())
        } else if self.conditions.is_some() {
            StepType::Condition(self.clone().into_condition_step())
        } else if self.branches.is_some() {
            StepType::Parallel(self.clone().into_parallel_step())
        } else if self.foreach.is_some() {
            StepType::Loop(self.clone().into_loop_step())
        } else if self.flow.is_some() {
            StepType::Subprocess(self.clone().into_subprocess_step())
        } else if self.task.is_some() {
            StepType::HumanTask(self.clone().into_human_task_step())
        } else if self.transition.is_some() {
            StepType::Transition(self.clone().into_transition_step())
        } else if self.status.is_some() {
            StepType::Terminal(self.clone().into_terminal_step())
        } else {
            // Default to action
            StepType::Action(self.clone().into_action_step())
        }
    }

    fn into_action_step(self) -> ActionStep {
        let mut params = std::collections::HashMap::new();
        if let Some(p) = self.params {
            for (k, v) in p {
                params.insert(k, yaml_value_to_expr(v));
            }
        }

        ActionStep {
            action: self.action.unwrap_or_default(),
            entity: self.entity,
            id: self.id.map(Expression::Raw),
            params,
            rules: self.rules.unwrap_or_default(),
            idempotency_key: self.idempotency_key.map(Expression::Raw),
            compensation: self.compensation.map(|c| CompensationAction {
                action: c.action,
                params: c.params.map(|p| p.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()).unwrap_or_default(),
            }),
        }
    }

    fn into_wait_step(self) -> WaitStep {
        let wait_for = self.wait_for.unwrap_or(YamlWaitFor {
            event: None,
            events: None,
            condition: None,
            timeout: None,
        });

        // Convert multi-event format to AST events array
        let events = wait_for.events.unwrap_or_default()
            .into_iter()
            .map(|e| {
                let mut set = std::collections::HashMap::new();
                if let Some(s) = e.set {
                    for (k, v) in s {
                        set.insert(k, Expression::Raw(v));
                    }
                }
                WaitEvent {
                    event: e.event,
                    condition: e.condition.map(Expression::Raw),
                    next: e.next,
                    set,
                }
            })
            .collect();

        WaitStep {
            event: wait_for.event,
            events,
            condition: wait_for.condition.map(Expression::Raw),
            timeout: wait_for.timeout,
            on_event: self.on_event.map(|o| o.into_outcome()),
            on_timeout: self.on_timeout.map(|o| o.into_outcome()),
        }
    }

    fn into_condition_step(self) -> ConditionStep {
        let mut conditions: Vec<ConditionBranch> = self.conditions
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.into_branch())
            .collect();

        // Add default branch if specified
        if let Some(default_value) = self.default {
            let default_branch = match default_value {
                // Simple string: just the next step name
                serde_yaml::Value::String(next) => ConditionBranch {
                    condition: None,  // No condition = else branch
                    next,
                    set: std::collections::HashMap::new(),
                },
                // Object with next and optional set
                serde_yaml::Value::Mapping(map) => {
                    let next = map.get(serde_yaml::Value::String("next".to_string()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    let mut set = std::collections::HashMap::new();
                    if let Some(serde_yaml::Value::Mapping(set_map)) = map.get(serde_yaml::Value::String("set".to_string())) {
                        for (k, v) in set_map {
                            if let serde_yaml::Value::String(key) = k {
                                set.insert(key.clone(), yaml_value_to_expr(v.clone()));
                            }
                        }
                    }
                    ConditionBranch {
                        condition: None,
                        next,
                        set,
                    }
                },
                _ => ConditionBranch::default(),
            };
            conditions.push(default_branch);
        }

        ConditionStep { conditions }
    }

    fn into_parallel_step(self) -> ParallelStep {
        ParallelStep {
            branches: self.branches
                .unwrap_or_default()
                .into_iter()
                .map(|b| b.into_branch())
                .collect(),
            join: self.join.map(|j| match j.to_lowercase().as_str() {
                "any" => JoinStrategy::Any,
                "all" => JoinStrategy::All,
                _ => JoinStrategy::All,
            }).unwrap_or_default(),
            on_complete: self.on_complete.map(|o| o.into_outcome()),
        }
    }

    fn into_loop_step(self) -> LoopStep {
        LoopStep {
            foreach: Expression::Raw(self.foreach.unwrap_or_default()),
            as_var: self.as_var.unwrap_or_else(|| "item".to_string()),
            index_var: self.index_var,
            steps: self.steps.unwrap_or_default().into_iter().map(|s| s.into_step()).collect(),
            on_complete: self.on_complete.map(|o| o.into_outcome()),
        }
    }

    fn into_subprocess_step(self) -> SubprocessStep {
        let mut params = std::collections::HashMap::new();
        if let Some(p) = self.params {
            for (k, v) in p {
                params.insert(k, yaml_value_to_expr(v));
            }
        }

        SubprocessStep {
            workflow: self.flow.unwrap_or_default(),
            params,
            wait: self.wait.unwrap_or(true),
        }
    }

    fn into_human_task_step(self) -> HumanTaskStep {
        let task = self.task.unwrap_or(YamlTaskConfig {
            title: String::new(),
            description: None,
            assignee: None,
            assignee_role: None,
            department: None,
            form: None,
            timeout: None,
            reminder: None,
        });

        HumanTaskStep {
            task: TaskConfig {
                title: Expression::Raw(task.title),
                description: task.description.map(Expression::Raw),
                assignee: task.assignee.map(Expression::Raw),
                assignee_role: task.assignee_role,
                department: task.department.map(Expression::Raw),
                form: task.form.map(|f| f.into_form()),
                timeout: task.timeout,
                reminder: task.reminder,
            },
            on_complete: self.conditions.unwrap_or_default().into_iter().map(|c| c.into_branch()).collect(),
            on_timeout: self.on_timeout.map(|o| TaskTimeoutAction {
                action: o.action.map(|a| match a.to_lowercase().as_str() {
                    "escalate" => TaskTimeoutActionType::Escalate,
                    "reassign" => TaskTimeoutActionType::Reassign,
                    "auto_approve" => TaskTimeoutActionType::AutoApprove,
                    "auto_reject" => TaskTimeoutActionType::AutoReject,
                    _ => TaskTimeoutActionType::Cancel,
                }).unwrap_or_default(),
                params: std::collections::HashMap::new(),
                next: o.next,
            }),
        }
    }

    fn into_transition_step(self) -> TransitionStep {
        let mut params = std::collections::HashMap::new();
        if let Some(p) = self.params {
            for (k, v) in p {
                params.insert(k, yaml_value_to_expr(v));
            }
        }

        TransitionStep {
            entity: self.entity.unwrap_or_default(),
            id: Expression::Raw(self.id.unwrap_or_default()),
            transition: self.transition.unwrap_or_default(),
            params,
        }
    }

    fn into_transaction_group(self) -> TransactionGroupStep {
        TransactionGroupStep {
            steps: self.steps.unwrap_or_default().into_iter().map(|s| s.into_step()).collect(),
        }
    }

    fn into_terminal_step(self) -> TerminalStep {
        TerminalStep {
            status: self.status.map(|s| match s.to_lowercase().as_str() {
                "success" => TerminalStatus::Success,
                "failed" => TerminalStatus::Failed,
                "cancelled" => TerminalStatus::Cancelled,
                "timed_out" => TerminalStatus::TimedOut,
                _ => TerminalStatus::Success,
            }).unwrap_or_default(),
            reason: self.reason.map(Expression::Raw),
            emit: self.emit.map(|e| EmitConfig {
                event: e.event,
                data: e.data.map(|d| d.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()).unwrap_or_default(),
            }),
            actions: self.actions.unwrap_or_default().into_iter().map(|s| s.into_action_step()).collect(),
            compensate: self.compensate.unwrap_or(false),
        }
    }
}

impl YamlStepOutcome {
    /// Convert to AST StepOutcome
    pub fn into_outcome(self) -> StepOutcome {
        StepOutcome {
            set: self.set.map(|s| s.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()).unwrap_or_default(),
            next: self.next,
            log: self.log.map(|l| LogConfig {
                level: l.level.map(|lv| match lv.to_lowercase().as_str() {
                    "debug" => LogLevel::Debug,
                    "warn" | "warning" => LogLevel::Warn,
                    "error" => LogLevel::Error,
                    _ => LogLevel::Info,
                }).unwrap_or_default(),
                message: Expression::Raw(l.message),
            }),
        }
    }
}

impl YamlStepFailure {
    /// Convert to AST StepFailure
    pub fn into_failure(self) -> StepFailure {
        StepFailure {
            retry: self.retry,
            backoff: self.backoff.map(|b| match b.to_lowercase().as_str() {
                "linear" => BackoffStrategy::Linear { initial: "1s".to_string() },
                "exponential" => BackoffStrategy::Exponential { initial: "1s".to_string(), max: "1m".to_string() },
                _ => BackoffStrategy::Fixed(b),
            }),
            on_exhausted: self.on_exhausted.map(|o| o.into_outcome()),
            next: self.next,
        }
    }
}

impl YamlConditionBranch {
    /// Convert to AST ConditionBranch
    pub fn into_branch(self) -> ConditionBranch {
        ConditionBranch {
            condition: self.condition.map(Expression::Raw),
            next: self.next.unwrap_or_default(),
            set: self.set.map(|s| s.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()).unwrap_or_default(),
        }
    }
}

impl YamlParallelBranch {
    /// Convert to AST ParallelBranch
    pub fn into_branch(self) -> ParallelBranch {
        ParallelBranch {
            name: self.name,
            condition: self.condition.map(Expression::Raw),
            steps: self.steps.into_iter().map(|s| s.into_step()).collect(),
        }
    }
}

impl YamlTaskForm {
    /// Convert to AST TaskForm
    pub fn into_form(self) -> TaskForm {
        TaskForm {
            fields: self.fields.into_iter().map(|f| TaskFormField {
                name: f.name,
                field_type: f.field_type,
                required: f.required.unwrap_or(false),
                default: f.default.map(yaml_value_to_expr),
                label: f.label,
                validation: f.validation.unwrap_or_default(),
            }).collect(),
        }
    }
}

impl YamlCompensation {
    /// Convert to AST CompensationStep
    pub fn into_compensation(self) -> CompensationStep {
        let comp_type = if self.transition.is_some() {
            CompensationType::Transition {
                entity: self.entity.unwrap_or_default(),
                id: Expression::Raw(self.id.unwrap_or_default()),
                transition: self.transition.unwrap_or_default(),
                params: self.params.map(|p| p.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()).unwrap_or_default(),
            }
        } else if self.foreach.is_some() {
            CompensationType::Loop {
                foreach: Expression::Raw(self.foreach.unwrap_or_default()),
                as_var: self.as_var.unwrap_or_else(|| "item".to_string()),
                steps: self.steps.unwrap_or_default().into_iter().map(|s| s.into_compensation()).collect(),
            }
        } else {
            CompensationType::Action {
                action: self.action.unwrap_or_default(),
                entity: self.entity,
                id: self.id.map(Expression::Raw),
                params: self.params.map(|p| p.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()).unwrap_or_default(),
                where_clause: self.where_clause.map(Expression::Raw),
            }
        };

        CompensationStep {
            name: self.name,
            condition: self.condition.map(Expression::Raw),
            compensation_type: comp_type,
        }
    }
}

impl YamlWorkflowHandler {
    /// Convert to AST WorkflowHandler
    pub fn into_handler(self) -> WorkflowHandler {
        if let Some(emit) = self.emit {
            WorkflowHandler::Emit {
                emit,
                data: self.data.map(|d| d.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()),
            }
        } else if let Some(notify) = self.notify {
            WorkflowHandler::Notify {
                notify,
                message: self.message.map(Expression::Raw),
            }
        } else {
            WorkflowHandler::Action {
                action: self.action.unwrap_or_default(),
                params: self.params.map(|p| p.into_iter().map(|(k, v)| (k, Expression::Raw(v))).collect()).unwrap_or_default(),
            }
        }
    }
}

// ============================================================================
// DDD & AUTHORIZATION CONVERSION IMPLEMENTATIONS
// ============================================================================

impl YamlEntity {
    /// Convert to AST Entity
    pub fn into_entity(self, name: String) -> Entity {
        Entity {
            name,
            model_ref: self.model.unwrap_or_default(),
            description: self.description,
            implements: self.implements,
            value_objects: self.value_objects,
            methods: self.methods.into_iter().map(|m| m.into_method()).collect(),
            invariants: self.invariants,
            ..Default::default()
        }
    }
}

impl YamlEntityMethod {
    /// Convert to AST EntityMethod
    pub fn into_method(self) -> EntityMethod {
        EntityMethod {
            name: self.name,
            description: self.description,
            mutates: self.mutates.unwrap_or(false),
            is_async: self.is_async.unwrap_or(false),
            params: self.params.into_iter().map(|(k, v)| (k, parse_type_ref(&v))).collect(),
            returns: self.returns.map(|r| parse_type_ref(&r)),
            ..Default::default()
        }
    }
}

impl YamlValueObject {
    /// Convert to AST ValueObject
    pub fn into_value_object(self, name: String) -> ValueObject {
        ValueObject {
            name,
            description: self.description,
            inner_type: self.inner_type.map(|t| parse_type_ref(&t)),
            validation: self.validation,
            methods: self.methods.into_iter().map(|m| m.into_method()).collect(),
            fields: self.fields.into_iter().map(|(name, f)| f.into_field(name)).collect(),
            derives: self.derives,
            messages: self.messages,
            ..Default::default()
        }
    }
}

impl YamlValueObjectMethod {
    /// Convert to AST ValueObjectMethod
    pub fn into_method(self) -> ValueObjectMethod {
        ValueObjectMethod {
            name: self.name,
            description: self.description,
            returns: self.returns.map(|r| parse_type_ref(&r)).unwrap_or_else(|| TypeRef::Primitive(PrimitiveType::String)),
            params: self.params.into_iter().map(|(k, v)| (k, parse_type_ref(&v))).collect(),
            is_const: self.is_const.unwrap_or(false),
            ..Default::default()
        }
    }
}

impl YamlDomainService {
    /// Convert to AST DomainService
    pub fn into_domain_service(self, name: String) -> DomainService {
        DomainService {
            name,
            description: self.description,
            stateless: self.stateless.unwrap_or(true),
            dependencies: self.dependencies.into_iter().map(|d| d.into_dependency()).collect(),
            methods: self.methods.into_iter().map(|m| m.into_method()).collect(),
            ..Default::default()
        }
    }
}

impl YamlServiceDependency {
    /// Convert to AST ServiceDependency
    pub fn into_dependency(self) -> ServiceDependency {
        match self {
            YamlServiceDependency::Simple(name) => {
                // Infer type from name
                if name.ends_with("Repository") {
                    ServiceDependency::Repository(name)
                } else if name.ends_with("Service") {
                    ServiceDependency::Service(name)
                } else if name.ends_with("Client") {
                    ServiceDependency::Client(name)
                } else {
                    ServiceDependency::Service(name)
                }
            }
            YamlServiceDependency::Full { name, dep_type } => {
                match dep_type.to_lowercase().as_str() {
                    "repository" => ServiceDependency::Repository(name),
                    "client" => ServiceDependency::Client(name),
                    _ => ServiceDependency::Service(name),
                }
            }
        }
    }
}

impl YamlServiceMethod {
    /// Convert to AST ServiceMethod
    pub fn into_method(self) -> ServiceMethod {
        ServiceMethod {
            name: self.name,
            description: self.description,
            is_async: self.is_async.unwrap_or(true),
            params: self.params.into_iter().map(|(k, v)| (k, parse_type_ref(&v))).collect(),
            returns: self.returns.map(|r| parse_type_ref(&r)),
            error_type: self.error,
            ..Default::default()
        }
    }
}

impl YamlEventSourced {
    /// Convert to AST EventSourcedConfig
    pub fn into_event_sourced(self, entity_name: String) -> EventSourcedConfig {
        EventSourcedConfig {
            entity_name,
            description: self.description,
            events: self.events,
            snapshot: self.snapshot.map(|s| s.into_config()),
            handlers: self.handlers,
            ..Default::default()
        }
    }
}

impl YamlSnapshotConfig {
    /// Convert to AST SnapshotConfig
    pub fn into_config(self) -> SnapshotConfig {
        SnapshotConfig {
            enabled: self.enabled.unwrap_or(true),
            every_n_events: self.every_n_events.unwrap_or(100),
            max_age_seconds: self.max_age_seconds,
            storage: self.storage,
        }
    }
}

impl YamlUseCase {
    /// Convert to AST UseCase
    pub fn into_usecase(self, name: String) -> UseCase {
        UseCase {
            name,
            description: self.description,
            actor: self.actor,
            input: self.input.into_iter().map(|(k, v)| (k, parse_type_ref(&v))).collect(),
            output: self.output.map(|o| parse_type_ref(&o)),
            steps: self.steps,
            is_async: self.is_async.unwrap_or(true),
            ..Default::default()
        }
    }
}

impl YamlDomainEvent {
    /// Convert to AST DomainEvent
    pub fn into_domain_event(self, name: String) -> DomainEvent {
        DomainEvent {
            name,
            description: self.description,
            aggregate: self.aggregate,
            version: self.version.unwrap_or(1),
            storage: self.storage.map(|s| s.into_event_storage()),
            fields: self.fields.into_iter().map(|f| f.into_event_field()).collect(),
            migrations: self.migrations,
            ..Default::default()
        }
    }
}

impl YamlEventStorage {
    /// Convert to AST EventStorage
    pub fn into_event_storage(self) -> EventStorage {
        EventStorage {
            store: self.store,
            retention: self.retention,
            pii_fields: self.pii_fields,
            index_fields: self.index_fields,
        }
    }
}

impl YamlEventField {
    /// Convert to AST EventField
    pub fn into_event_field(self) -> EventField {
        EventField {
            name: self.name,
            field_type: parse_type_ref(&self.field_type),
            description: self.description,
        }
    }
}

impl YamlAuthorization {
    /// Convert to AST AuthorizationConfig
    pub fn into_authorization(self) -> AuthorizationConfig {
        AuthorizationConfig {
            permissions: self.permissions,
            roles: self.roles.into_iter().map(|(name, r)| r.into_role(name)).collect(),
            policies: self.policies.into_iter().map(|(name, p)| p.into_policy(name)).collect(),
            resource_policies: self.resource_policies.into_iter().map(|(name, rp)| (name, rp.into_resource_policy())).collect(),
            attributes: self.attributes.map(|a| a.into_attributes()),
            abac_policies: self.abac_policies.into_iter().map(|(name, ap)| ap.into_abac_policy(name)).collect(),
            ..Default::default()
        }
    }
}

impl YamlRoleDefinition {
    /// Convert to AST RoleDefinition
    pub fn into_role(self, name: String) -> RoleDefinition {
        RoleDefinition {
            name,
            description: self.description,
            permissions: self.permissions,
            level: self.level,
            inherits: self.inherits,
            own_resources: self.own_resources,
            ..Default::default()
        }
    }
}

impl YamlPolicy {
    /// Convert to AST PolicyDefinition
    pub fn into_policy(self, name: String) -> PolicyDefinition {
        PolicyDefinition {
            name,
            description: self.description,
            policy_type: self.policy_type.map(|t| PolicyType::from_str(&t)).unwrap_or_default(),
            rules: self.rules.into_iter().map(|r| r.into_rule()).collect(),
            ..Default::default()
        }
    }
}

impl YamlPolicyRule {
    /// Convert to AST PolicyRule
    pub fn into_rule(self) -> PolicyRule {
        match self {
            YamlPolicyRule::Simple(s) => PolicyRule::Permission(s),
            YamlPolicyRule::Full { permission, role, owner, condition, message, not, policy } => {
                if let Some(perm) = permission {
                    PolicyRule::Permission(perm)
                } else if let Some(r) = role {
                    PolicyRule::Role(r)
                } else if let Some(o) = owner {
                    PolicyRule::Owner {
                        resource: o.resource.unwrap_or_default(),
                        field: o.field,
                        actor_field: o.actor_field,
                    }
                } else if let Some(cond) = condition {
                    PolicyRule::Condition {
                        expression: cond,
                        message,
                    }
                } else if let Some(n) = not {
                    PolicyRule::Not(Box::new(n.into_rule()))
                } else if let Some(p) = policy {
                    PolicyRule::PolicyRef(p)
                } else {
                    PolicyRule::Permission(String::new())
                }
            }
        }
    }
}

impl YamlResourcePolicy {
    /// Convert to AST ResourcePolicy
    pub fn into_resource_policy(self) -> ResourcePolicy {
        ResourcePolicy {
            read: self.read.into_iter().map(|r| r.into_rule()).collect(),
            create: self.create.into_iter().map(|r| r.into_rule()).collect(),
            update: self.update.into_iter().map(|r| r.into_rule()).collect(),
            delete: self.delete.into_iter().map(|r| r.into_rule()).collect(),
            custom: self.custom.into_iter().map(|(k, v)| (k, v.into_iter().map(|r| r.into_rule()).collect())).collect(),
            ..Default::default()
        }
    }
}

impl YamlResourcePolicyRule {
    /// Convert to AST ResourcePolicyRule
    pub fn into_rule(self) -> ResourcePolicyRule {
        match self {
            YamlResourcePolicyRule::Simple(s) => {
                // Try to determine if it's a policy reference or permission
                if s.contains('.') || s.contains(':') {
                    ResourcePolicyRule::Permission(s)
                } else {
                    ResourcePolicyRule::Policy(s)
                }
            }
            YamlResourcePolicyRule::Full { policy, permission, owner, condition, message } => {
                if let Some(p) = policy {
                    ResourcePolicyRule::Policy(p)
                } else if let Some(perm) = permission {
                    ResourcePolicyRule::Permission(perm)
                } else if let Some(o) = owner {
                    ResourcePolicyRule::Owner(o)
                } else if let Some(cond) = condition {
                    ResourcePolicyRule::Condition {
                        expression: cond,
                        message,
                    }
                } else {
                    ResourcePolicyRule::Permission(String::new())
                }
            }
        }
    }
}

impl YamlAbacAttributes {
    /// Convert to AST AbacAttributes
    pub fn into_attributes(self) -> AbacAttributes {
        AbacAttributes {
            subject: self.subject,
            resource: self.resource,
            environment: self.environment,
        }
    }
}

impl YamlAbacPolicy {
    /// Convert to AST AbacPolicy
    pub fn into_abac_policy(self, name: String) -> AbacPolicy {
        AbacPolicy {
            name,
            description: self.description,
            condition: self.condition.unwrap_or_default(),
            ..Default::default()
        }
    }
}

// =============================================================================
// PROJECTION CONVERSIONS
// =============================================================================

impl YamlProjection {
    /// Convert to AST Projection
    pub fn into_projection(self, name: String) -> Projection {
        Projection {
            name,
            description: self.description,
            aggregation: self.aggregation.unwrap_or(false),
            partition_by: self.partition_by,
            storage: self.storage.map(|s| s.into_storage()),
            source_events: self.source_events.into_iter().map(|e| e.into_source_event()).collect(),
            external_events: self.external_events.into_iter().map(|e| e.into_source_event()).collect(),
            fields: self.fields.into_iter().map(|f| f.into_field()).collect(),
            indexes: self.indexes.into_iter().map(|i| i.into_index()).collect(),
            ..Default::default()
        }
    }
}

impl YamlProjectionStorage {
    /// Convert to AST ProjectionStorage
    pub fn into_storage(self) -> ProjectionStorage {
        ProjectionStorage {
            storage_type: self.storage_type,
            table: self.table,
        }
    }
}

impl YamlSourceEvent {
    /// Convert to AST SourceEvent
    pub fn into_source_event(self) -> SourceEvent {
        match self {
            YamlSourceEvent::Simple(name) => SourceEvent {
                name,
                action: None,
                fields: Vec::new(),
            },
            YamlSourceEvent::WithAction(map) => {
                if let Some((name, action)) = map.into_iter().next() {
                    SourceEvent {
                        name,
                        action: action.action,
                        fields: action.fields,
                    }
                } else {
                    SourceEvent {
                        name: String::new(),
                        action: None,
                        fields: Vec::new(),
                    }
                }
            }
        }
    }
}

impl YamlProjectionField {
    /// Convert to AST ProjectionField
    pub fn into_field(self) -> ProjectionField {
        ProjectionField {
            name: self.name,
            field_type: parse_type_ref(&self.field_type),
            primary: self.primary.unwrap_or(false),
            from: self.from,
            default: self.default,
            nullable: self.nullable.unwrap_or(false),
        }
    }
}

impl YamlProjectionIndex {
    /// Convert to AST ProjectionIndex
    pub fn into_index(self) -> ProjectionIndex {
        ProjectionIndex {
            fields: self.fields,
            unique: self.unique.unwrap_or(false),
        }
    }
}

// =============================================================================
// APPLICATION SERVICE CONVERSIONS
// =============================================================================

impl YamlAppService {
    /// Convert to AST AppService
    pub fn into_app_service(self, name: String) -> AppService {
        AppService {
            name,
            description: self.description,
            is_async: self.is_async.unwrap_or(true),
            dependencies: self.dependencies.into_iter().map(|d| d.into_tuple()).collect(),
            methods: self.methods.into_iter().map(|m| m.into_method()).collect(),
            ..Default::default()
        }
    }
}

impl YamlServiceDep {
    /// Convert to tuple (name, type)
    pub fn into_tuple(self) -> (String, String) {
        match self {
            YamlServiceDep::Simple(s) => {
                // Parse "name: Type" format
                if let Some((name, type_str)) = s.split_once(':') {
                    (name.trim().to_string(), type_str.trim().to_string())
                } else {
                    (s.clone(), s)
                }
            }
            YamlServiceDep::Map(map) => {
                map.into_iter().next().unwrap_or((String::new(), String::new()))
            }
        }
    }
}

impl YamlAppServiceMethod {
    /// Convert to AST AppServiceMethod
    pub fn into_method(self) -> AppServiceMethod {
        let params: IndexMap<String, TypeRef> = self.params
            .into_iter()
            .flat_map(|m| m.into_iter())
            .map(|(k, v)| (k, parse_type_ref(&v)))
            .collect();

        AppServiceMethod {
            name: self.name,
            description: self.description,
            params,
            returns: self.returns.map(|r| parse_type_ref(&r)),
        }
    }
}

// =============================================================================
// EVENT HANDLER CONVERSIONS
// =============================================================================

impl YamlHandler {
    /// Convert to AST Handler
    pub fn into_handler(self, name: String) -> Handler {
        Handler {
            name,
            description: self.description,
            event: self.event,
            dependencies: self.dependencies.into_iter().map(|d| d.into_tuple()).collect(),
            retry: self.retry.map(|r| r.into_retry()),
            async_dispatch: self.async_dispatch.unwrap_or(true),
            transaction: self.transaction,
            ..Default::default()
        }
    }
}

impl YamlHandlerRetryPolicy {
    /// Convert to AST HandlerRetryPolicy
    pub fn into_retry(self) -> HandlerRetryPolicy {
        HandlerRetryPolicy {
            max_attempts: self.max_attempts.unwrap_or(3),
            backoff: self.backoff,
            initial_delay: self.initial_delay,
            max_delay: self.max_delay,
        }
    }
}

impl YamlSubscription {
    /// Convert to AST Subscription
    pub fn into_subscription(self, module: String, event: String) -> Subscription {
        Subscription {
            module,
            event,
            handler: self.handler,
            description: self.description,
            condition: self.condition,
        }
    }
}

// =============================================================================
// INTEGRATION/ACL CONVERSIONS
// =============================================================================

impl YamlIntegration {
    /// Convert to AST Integration
    pub fn into_integration(self, name: String) -> Integration {
        Integration {
            name,
            description: self.description,
            is_async: self.is_async.unwrap_or(true),
            methods: self.methods.into_iter().map(|m| m.into_method()).collect(),
            ..Default::default()
        }
    }
}

impl YamlIntegrationMethod {
    /// Convert to AST IntegrationMethod
    pub fn into_method(self) -> IntegrationMethod {
        let params: IndexMap<String, TypeRef> = self.params
            .into_iter()
            .flat_map(|m| m.into_iter())
            .map(|(k, v)| (k, parse_type_ref(&v)))
            .collect();

        IntegrationMethod {
            name: self.name,
            params,
            returns: self.returns.map(|r| parse_type_ref(&r)),
        }
    }
}

// =============================================================================
// PRESENTATION LAYER CONVERSIONS
// =============================================================================

impl YamlPresentation {
    /// Convert to AST Presentation
    pub fn into_presentation(self) -> Presentation {
        Presentation {
            http: self.http.map(|h| h.into_http_config()),
            grpc: self.grpc.map(|g| g.into_grpc_config()),
        }
    }
}

impl YamlHttpConfig {
    /// Convert to AST HttpConfig
    pub fn into_http_config(self) -> HttpConfig {
        HttpConfig {
            prefix: self.prefix,
            routes: self.routes.into_iter().map(|(k, v)| (k, v.into_route_group())).collect(),
        }
    }
}

impl YamlRouteGroup {
    /// Convert to AST RouteGroup
    pub fn into_route_group(self) -> RouteGroup {
        RouteGroup {
            prefix: self.prefix,
            middleware: self.middleware,
            endpoints: self.endpoints.into_iter().map(|e| e.into_endpoint()).collect(),
        }
    }
}

impl YamlEndpoint {
    /// Convert to AST Endpoint
    pub fn into_endpoint(self) -> Endpoint {
        Endpoint {
            name: self.name,
            method: self.method,
            path: self.path,
            usecase: self.usecase,
            response: self.response,
            status: self.status,
            public: self.public.unwrap_or(false),
        }
    }
}

impl YamlGrpcConfig {
    /// Convert to AST GrpcConfig
    pub fn into_grpc_config(self) -> GrpcConfig {
        GrpcConfig {
            package: self.package,
            services: self.services.into_iter().map(|(k, v)| (k, v.into_grpc_service())).collect(),
        }
    }
}

impl YamlGrpcService {
    /// Convert to AST GrpcService
    pub fn into_grpc_service(self) -> GrpcService {
        GrpcService {
            description: self.description,
            methods: self.methods.into_iter().map(|m| m.into_grpc_method()).collect(),
        }
    }
}

impl YamlGrpcMethod {
    /// Convert to AST GrpcMethod
    pub fn into_grpc_method(self) -> GrpcMethod {
        GrpcMethod {
            name: self.name,
            input: self.input,
            output: self.output,
            public: self.public.unwrap_or(false),
        }
    }
}

// =============================================================================
// DTO CONVERSIONS
// =============================================================================

impl YamlDto {
    /// Convert to AST Dto
    pub fn into_dto(self, name: String) -> Dto {
        Dto {
            name,
            description: self.description,
            generic: self.generic.unwrap_or(false),
            from_entity: self.from_entity,
            fields: self.fields.into_iter().map(|f| f.into_dto_field()).collect(),
            exclude: self.exclude,
            computed: self.computed.into_iter().map(|c| c.into_computed()).collect(),
            ..Default::default()
        }
    }
}

impl YamlDtoField {
    /// Convert to AST DtoField
    pub fn into_dto_field(self) -> DtoField {
        match self {
            YamlDtoField::Simple(name) => DtoField::Simple(name),
            YamlDtoField::Full { name, field_type, optional, .. } => DtoField::Full {
                name,
                field_type: parse_type_ref(&field_type),
                optional: optional.unwrap_or(false),
            },
        }
    }
}

impl YamlComputedDtoField {
    /// Convert to AST ComputedDtoField
    pub fn into_computed(self) -> ComputedDtoField {
        ComputedDtoField {
            name: self.name,
            field_type: parse_type_ref(&self.field_type),
            expression: self.expression,
        }
    }
}

// =============================================================================
// VERSIONING CONVERSIONS
// =============================================================================

impl YamlVersioning {
    /// Convert to AST Versioning
    pub fn into_versioning(self) -> Versioning {
        Versioning {
            strategy: self.strategy,
            current: self.current,
            supported: self.supported,
            deprecated: self.deprecated,
        }
    }
}

// =============================================================================
// REPOSITORY TRAIT CONVERSIONS
// =============================================================================

impl YamlRepositoryTrait {
    /// Convert to AST RepositoryTrait
    pub fn into_repository_trait(self, name: String) -> RepositoryTrait {
        RepositoryTrait {
            name,
            description: self.description,
            extends: self.extends,
            entity: self.entity,
            is_async: self.is_async.unwrap_or(true),
            error_type: self.error_type,
            auto_methods: self.auto_methods,
            methods: self.methods.into_iter().map(|m| m.into_method()).collect(),
            ..Default::default()
        }
    }
}

impl YamlTraitMethod {
    /// Convert to AST TraitMethod
    pub fn into_method(self) -> TraitMethod {
        let params: IndexMap<String, TypeRef> = self.params
            .into_iter()
            .flat_map(|m| m.into_iter())
            .map(|(k, v)| (k, parse_type_ref(&v)))
            .collect();

        TraitMethod {
            name: self.name,
            params,
            returns: self.returns.map(|r| parse_type_ref(&r)),
        }
    }
}

