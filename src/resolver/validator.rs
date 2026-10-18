//! Schema validator
//!
//! Validates schema completeness and correctness.

use super::ResolveError;
use crate::ast::{CompanyFence, Enforcement, ModuleSchema, OrgFence};
use std::collections::HashSet;

/// Validates a schema for correctness
pub struct SchemaValidator<'a> {
    schema: &'a ModuleSchema,
}

impl<'a> SchemaValidator<'a> {
    pub fn new(schema: &'a ModuleSchema) -> Self {
        Self { schema }
    }

    /// Validate the schema
    pub fn validate(&self) -> Result<(), Vec<ResolveError>> {
        let mut errors = Vec::new();

        // PHASE 2: Check for duplicate model names
        let mut model_names = HashSet::new();
        for model in &self.schema.models {
            if !model_names.insert(&model.name) {
                errors.push(ResolveError::validation(format!(
                    "Schema has duplicate model name '{}'",
                    model.name
                )));
            }
        }

        // Validate models
        for model in &self.schema.models {
            errors.extend(self.validate_model(model));
        }

        // Validate enums
        for enum_def in &self.schema.enums {
            errors.extend(self.validate_enum(enum_def));
        }

        // Validate hooks (entity lifecycle)
        for hook in &self.schema.hooks {
            errors.extend(self.validate_hook(hook));
        }

        // A declared event the write hooks stage must name real fields, verbs and states
        errors.extend(event_problems(self.schema).into_iter().map(ResolveError::validation));

        // The permission declarations: roles, the vocabulary and every `requires:`
        errors.extend(crate::rbac::Rbac::new(self.schema).problems().into_iter().map(|p| ResolveError::validation(p.message)));

        // Hook triggers the generator cannot honour (a module on `legacy_triggers` has none reported)
        errors.extend(trigger_problems(self.schema).into_iter().map(ResolveError::validation));

        // Workflows the engine runs: every name, expression and hand surface must hold
        errors.extend(workflow_problems(self.schema).into_iter().map(ResolveError::validation));

        // Validate workflows (business processes)
        for workflow in &self.schema.workflows {
            errors.extend(self.validate_workflow(workflow));
        }

        // Validate the module-level company fence (ADR-0014)
        errors.extend(self.validate_company_fence());

        // Validate the org fence declaration and org_unit_id columns (ADR-0028)
        errors.extend(self.validate_org_fence());

        // Validate field-level lifecycle declarations (ADR-0016)
        errors.extend(self.validate_lifecycles());

        // Validate scheduled-job postures (ADR-0020)
        errors.extend(self.validate_scheduled_jobs());

        // Two models must not emit the same HTTP route function
        errors.extend(self.validate_route_function_names());

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Every model with generated HTTP handlers emits four route functions named
    /// after it: `create_<m>_routes`, `create_<m>_read_routes`,
    /// `create_<m>_write_routes` and `create_<m>_history_route`. A model named
    /// `<X>Read` or `<X>Write` beside `<X>` emits a name `<X>` already emits, and
    /// the generated re-exports then fail to compile. Reject it here, naming both
    /// models, so the schema says how to resolve it (rename one, or disable the
    /// `handler` generator on the model that needs no generic routes).
    fn validate_route_function_names(&self) -> Vec<ResolveError> {
        if self
            .schema
            .generators_config
            .as_ref()
            .and_then(|cfg| cfg.disabled.as_ref())
            .is_some_and(|list| list.iter().any(|d| d.eq_ignore_ascii_case("handler")))
        {
            return Vec::new();
        }
        let mut owner: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        let mut errors = Vec::new();
        for model in &self.schema.models {
            if crate::generators::model_skips_target(model, crate::generators::GenerationTarget::Handler) {
                continue;
            }
            let snake = crate::utils::to_snake_case(&model.name);
            for name in [
                format!("create_{snake}_routes"),
                format!("create_{snake}_read_routes"),
                format!("create_{snake}_write_routes"),
                format!("create_{snake}_history_route"),
            ] {
                match owner.get(&name) {
                    Some(first) if *first != model.name.as_str() => errors.push(ResolveError::validation(format!(
                        "Models '{first}' and '{}' both generate the route function '{name}'; \
                         rename one, or disable the `handler` generator on the model that needs no generic routes",
                        model.name
                    ))),
                    Some(_) => {}
                    None => {
                        owner.insert(name, model.name.as_str());
                    }
                }
            }
        }
        errors
    }

    /// Validate the module-level `company_fence:` declaration (ADR-0014).
    ///
    /// One fatal combination: `company_fence: none` alongside a model with a
    /// non-`@global` `company_id` column. That module silently unfences every
    /// row it stores — the declaration says "no company dimension" while the
    /// schema says otherwise, so the module is lying about its shape and the
    /// correct fix (mark the column `@global`, drop it, or pick a real fence)
    /// is a per-model decision the validator must not make silently.
    fn validate_company_fence(&self) -> Vec<ResolveError> {
        if self.schema.company_fence != Some(CompanyFence::None) {
            return Vec::new();
        }
        self.schema
            .models
            .iter()
            .filter(|m| {
                m.fields
                    .iter()
                    .any(|f| f.name == "company_id" && !f.has_attribute("global"))
            })
            .map(|m| {
                ResolveError::validation(format!(
                    "module declares company_fence: none but model '{}' has a non-@global \
                     'company_id' column — this would silently unfence its rows (ADR-0014); \
                     mark the column @global, remove it, or declare a real fence",
                    m.name
                ))
            })
            .collect()
    }

    /// Validate the module-level `org_fence:` declaration and `org_unit_id` columns
    /// (ADR-0028).
    ///
    /// Unlike the company fence there is no legacy posture to infer: the org fence
    /// must be declared **before** the first `org_unit_id` column lands, so every
    /// combination where an org-scoped column exists without a `strict` declaration
    /// is a hard failure, not a warning. Fatal combinations:
    ///
    /// - an `org_unit_id` column with the declaration absent, or declared `none` —
    ///   both would emit no fence for a scoping key that exists (an unfenced tenant
    ///   column is worse than none);
    /// - a model carrying both a non-`@global` `company_id` and an `org_unit_id` —
    ///   the sanctioned transition is a diff-generated re-key migration that swaps
    ///   the columns in one step; both on the same model is a state no migration
    ///   will produce and no runtime fence covers;
    /// - `@global` on an `org_unit_id` column — the column IS the tenant scoping
    ///   key, a "global org-scoped row" is a contradiction;
    /// - `@org_root_shared` on any field other than `org_unit_id` — the attribute
    ///   widens the scoping key's kind guard; elsewhere it does nothing and only
    ///   mimics the vocabulary.
    fn validate_org_fence(&self) -> Vec<ResolveError> {
        let mut errors = Vec::new();
        for model in &self.schema.models {
            // @org_root_shared belongs on the scoping key itself, wherever it landed.
            if let Some(field) = model
                .fields
                .iter()
                .find(|f| f.has_attribute("org_root_shared") && f.name != "org_unit_id")
            {
                errors.push(ResolveError::validation(format!(
                    "model '{}' puts @org_root_shared on field '{}' — the attribute \
                     widens the org_unit_id kind guard to the root node and belongs \
                     on the scoping key itself (ADR-0028)",
                    model.name, field.name
                )));
            }

            if model
                .fields
                .iter()
                .any(|f| f.name == "org_unit_id" && f.has_attribute("global"))
            {
                errors.push(ResolveError::validation(format!(
                    "model '{}' marks 'org_unit_id' @global — the column is the tenant \
                     scoping key (ADR-0028), a global org-scoped row is a contradiction; \
                     drop either the column or the @global",
                    model.name
                )));
            }

            let Some(org_field) = model
                .fields
                .iter()
                .find(|f| f.name == "org_unit_id" && !f.has_attribute("global"))
            else {
                continue;
            };
            if self.schema.org_fence != Some(OrgFence::Strict) {
                errors.push(ResolveError::validation(format!(
                    "model '{}' carries an 'org_unit_id' column but the module declares \
                     no org_fence: strict in index.model.yaml — unlike the legacy company \
                     fence there is no posture to infer (ADR-0028); declare \
                     'org_fence: strict' before the first org-scoped column lands",
                    model.name
                )));
            }
            if org_field.type_ref.is_optional() {
                errors.push(ResolveError::validation(format!(
                    "model '{}' declares 'org_unit_id' optional — the org scoping key is \
                     NOT NULL by law (ADR-0028); shared rows anchor on the root node, \
                     never on a NULL key",
                    model.name
                )));
            }
            if model
                .fields
                .iter()
                .any(|f| f.name == "company_id" && !f.has_attribute("global"))
            {
                errors.push(ResolveError::validation(format!(
                    "model '{}' carries both 'company_id' and 'org_unit_id' — the column \
                     swap is a generated re-key migration, not a schema state (ADR-0028); \
                     keep company_id and generate the re-key, or drop it if already re-keyed",
                    model.name
                )));
            }
        }
        errors
    }

    /// Validate field-level `lifecycle:` declarations (ADR-0016).
    ///
    /// The declaration is advisory for generators but its *references* must be
    /// real, or it degrades into a comment no one can trust:
    /// - `split` needs a `driver:` naming another field on the same model
    ///   (the stage/sub-state pair is the whole point of the shape);
    /// - `stage_ref` needs a relation on this field (it hangs another field's
    ///   lifecycle off this reference — a plain column can't do that);
    /// - `hand_set` + `state_machine:` needs that machine to exist in a hook
    ///   AND to guard *this* field (`machine.field == field.name`), otherwise
    ///   the "guarded transition graph" claim is false.
    fn validate_lifecycles(&self) -> Vec<ResolveError> {
        use crate::ast::LifecycleShape;

        let mut errors = Vec::new();

        // Every declared state machine in the module, by hook name.
        let machines: Vec<(String, String, &crate::ast::StateMachine)> = self
            .schema
            .hooks
            .iter()
            .filter_map(|h| {
                h.state_machine
                    .as_ref()
                    .map(|m| (h.name.clone(), m.field.clone(), m))
            })
            .collect();

        for model in &self.schema.models {
            for field in &model.fields {
                let Some(lifecycle) = &field.lifecycle else {
                    continue;
                };
                let declared = format!(
                    "model '{}' field '{}' (lifecycle shape '{}')",
                    model.name,
                    field.name,
                    lifecycle.shape.shape_name()
                );

                // `driver:` — whenever present, must name a *different*
                // same-model field; `split` additionally requires it.
                if let Some(driver) = &lifecycle.driver {
                    if driver == &field.name {
                        errors.push(ResolveError::validation(format!(
                            "{declared}: driver '{driver}' is the field itself — \
                             a field cannot drive its own lifecycle"
                        )));
                    } else if !model.fields.iter().any(|f| &f.name == driver) {
                        errors.push(ResolveError::validation(format!(
                            "{declared}: driver '{driver}' is not a field on model '{}' \
                             — split/projection drivers must be same-model fields",
                            model.name
                        )));
                    }
                }
                if lifecycle.shape == LifecycleShape::Split && lifecycle.driver.is_none() {
                    errors.push(ResolveError::validation(format!(
                        "{declared}: split requires a 'driver:' naming the stage field \
                         this sub-state splits against (ADR-0016)"
                    )));
                }

                // `stage_ref` — the field must carry a relation.
                if lifecycle.shape == LifecycleShape::StageRef
                    && !model.relations.iter().any(|r| {
                        r.name == field.name
                            || field.name.strip_suffix("_id") == Some(r.name.as_str())
                    })
                {
                    errors.push(ResolveError::validation(format!(
                        "{declared}: stage_ref requires a relation on this field \
                         (a stage another field's lifecycle hangs off must be a real reference)"
                    )));
                }

                // `hand_set` + `state_machine:` — the machine must exist and
                // guard exactly this field.
                if lifecycle.shape == LifecycleShape::HandSet {
                    if let Some(machine) = &lifecycle.state_machine {
                        match machines.iter().find(|(hook, _, _)| hook == machine) {
                            None => errors.push(ResolveError::validation(format!(
                                "{declared}: state_machine '{machine}' does not exist — \
                                 no hook in this module declares it"
                            ))),
                            Some((_, guarded_field, _)) if guarded_field != &field.name => {
                                errors.push(ResolveError::validation(format!(
                                    "{declared}: state_machine '{machine}' guards field \
                                     '{guarded_field}', not '{}' — a hand_set field must name \
                                     the machine that guards it",
                                    field.name
                                )));
                            }
                            Some((_, _, sm)) => {
                                // Null-boundary coherence: a machine that arms
                                // from or ends to the null boundary can only
                                // guard a nullable field — the boundary is the
                                // column's NULL.
                                let field_nullable = matches!(
                                    &field.type_ref,
                                    crate::ast::TypeRef::Optional(_)
                                );
                                if sm.is_nullable() && !field_nullable {
                                    errors.push(ResolveError::validation(format!(
                                        "{declared}: state_machine '{machine}' uses the null \
                                         state boundary (from/to 'null'), but the field is not \
                                         nullable — declare the field type as optional or drop \
                                         the null-boundary edges"
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }

        errors
    }

    /// Validate scheduled-job declarations (ADR-0020).
    ///
    /// Two hard rules:
    /// - `self_arming` with no `triggers:` — the interval is a floor, the
    ///   triggers are the contract; without them the job is just a pull loop
    ///   wearing a misleading name;
    /// - a queue-draining posture (`pull` / `self_arming` / `host_riding` /
    ///   `autovacuum_ride`) without `pickup_lock: true` — two concurrent
    ///   workers will double-process the same rows (MMB-4 class). The exempt
    ///   postures either don't run on a schedule at all (`read_time_lazy`) or
    ///   aren't the ones doing the claiming (`inactive_then_icp`).
    fn validate_scheduled_jobs(&self) -> Vec<ResolveError> {
        use crate::ast::JobPosture;

        self.schema
            .scheduled_jobs
            .iter()
            .flat_map(|job| {
                let mut errors = Vec::new();
                let Some(posture) = job.posture else {
                    return errors;
                };

                if posture == JobPosture::SelfArming && job.triggers.is_empty() {
                    errors.push(ResolveError::validation(format!(
                        "scheduled job '{}' declares posture: self_arming with no triggers: \
                         — the schedule interval is only a floor; name the events that \
                         re-arm the job (ADR-0020)",
                        job.name
                    )));
                }

                let queue_draining = matches!(
                    posture,
                    JobPosture::Pull
                        | JobPosture::SelfArming
                        | JobPosture::HostRiding
                        | JobPosture::AutovacuumRide
                );
                if queue_draining && !job.pickup_lock {
                    errors.push(ResolveError::validation(format!(
                        "scheduled job '{}' declares posture: {} without pickup_lock: true \
                         — concurrent workers will claim the same rows and double-process \
                         them (MMB-4 class); claim intake with FOR UPDATE SKIP LOCKED or \
                         declare why this job cannot race (ADR-0020)",
                        job.name,
                        job_posture_name(posture)
                    )));
                }

                errors
            })
            .collect()
    }

    fn validate_model(&self, model: &crate::ast::Model) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // The data-change audit captures whole row images and the history endpoint
        // serves them, so a secret on an audited model would be stored and served.
        if model.has_attribute("audited") {
            let secrets: Vec<&str> = model
                .fields
                .iter()
                .filter(|f| f.has_attribute("sensitive") || f.has_attribute("hashed"))
                .map(|f| f.name.as_str())
                .collect();
            if !secrets.is_empty() {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' is @audited and declares the secret field(s) {}: the audit trail stores whole rows and the history endpoint serves them; move the secret to a model that is not audited",
                    model.name,
                    secrets.join(", ")
                )));
            }
        }

        // Check for primary key
        let has_id = model.fields.iter().any(|f| f.has_attribute("id"));
        if !has_id {
            errors.push(ResolveError::validation(format!(
                "Model '{}' has no primary key field (missing @id attribute)",
                model.name
            )));
        }

        // Collect known model names for relation validation
        let known_models: HashSet<_> = self.schema.models.iter().map(|m| m.name.as_str()).collect();

        let known_types: HashSet<_> = self
            .schema
            .enums
            .iter()
            .map(|e| e.name.as_str())
            .chain(self.schema.type_defs.iter().map(|t| t.name.as_str()))
            .collect();

        // Check for duplicate field names and validate each field
        let mut field_names = HashSet::new();
        for field in &model.fields {
            if !field_names.insert(&field.name) {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' has duplicate field '{}'",
                    model.name, field.name
                )));
            }

            // PHASE 2: Check that fields ending with _id have @foreign_key attribute
            // Skip check if @exclude_from_foreign_key_check attribute is present
            let skip_fk_check = field.has_attribute("exclude_from_foreign_key_check");
            if field.name.ends_with("_id") && !field.has_attribute("foreign_key") && !skip_fk_check
            {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' field '{}' ends with '_id' but missing @foreign_key(Model.field) attribute (use @exclude_from_foreign_key_check for non-reference IDs)",
                    model.name, field.name
                )));
            }

            // Check that an intra-module @foreign_key names a model that actually exists.
            //
            // Declaring a @foreign_key is not the same as its target being real: a whole
            // `Organization` subsystem pointed five FKs at `corpus.Organization`, an entity that
            // never existed in any module, and every validation passed — because nothing checked
            // the target. This closes the same-module case (the one this single-module validator
            // CAN see): `@foreign_key(Ghost.id)` where `Ghost` is not a model here is now an error.
            //
            // Cross-module targets (`module.Entity.id`) are intentionally NOT flagged here: this
            // validator sees only the current module, so it cannot know another module's entities.
            // Verifying those needs a workspace-level pass over the full model registry (the
            // `corpus.Organization` phantom was cross-module and lives beyond this guard's reach).
            if let Some(fk) = field.attributes.iter().find(|a| a.name == "foreign_key") {
                // `@foreign_key(Entity.id)` is written unquoted, so the parser yields `Ident`, not
                // `String`. Matching only `String` here made this check a silent no-op on every real
                // schema; `fk_target` accepts both.
                if let Some(target) = fk
                    .args
                    .first()
                    .and_then(|(_, v)| crate::resolver::cross_module_fk::fk_target(v))
                {
                    let parts: Vec<&str> = target.split('.').collect();
                    // `Entity.column` = intra-module (2 parts). `module.Entity.column` = cross (3).
                    if parts.len() == 2 {
                        let entity = parts[0];
                        if !known_models.contains(entity) && !entity.is_empty() {
                            errors.push(ResolveError::validation(format!(
                                "Model '{}' field '{}' has @foreign_key({}) but no model '{}' exists in this module \
                                 (declare it, fix the name, or use `module.{}` if it lives in another module)",
                                model.name, field.name, target, entity, target
                            )));
                        }
                    }
                }
            }

            // PHASE 2: Check that metadata fields use 'json' type, not custom types
            if field.name == "metadata" {
                if let crate::ast::TypeRef::Custom(ref type_name) = field.type_ref {
                    if type_name != "json" && type_name != "Json" {
                        errors.push(ResolveError::validation(format!(
                            "Model '{}' metadata field must use 'type: json' not 'type: {}'",
                            model.name, type_name
                        )));
                    }
                }
            }
        }

        // Validate index fields reference actual columns or valid JSONB expressions
        errors.extend(self.validate_indexes(model, &field_names));

        // Check for duplicate relation names and validate relations
        for relation in &model.relations {
            if !field_names.insert(&relation.name) {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' has duplicate relation/field name '{}'",
                    model.name, relation.name
                )));
            }

            // PHASE 2: Check that relation targets reference existing models
            let target_name = match &relation.target {
                crate::ast::TypeRef::Custom(name) => name.as_str(),
                crate::ast::TypeRef::Optional(inner) => {
                    if let crate::ast::TypeRef::Custom(name) = inner.as_ref() {
                        name.as_str()
                    } else {
                        continue;
                    }
                }
                crate::ast::TypeRef::Array(inner) => {
                    if let crate::ast::TypeRef::Custom(name) = inner.as_ref() {
                        name.as_str()
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Skip if it's a known enum or type def (not a model relation)
            if known_types.contains(target_name) {
                continue;
            }

            // Check if it's a known model
            if !known_models.contains(target_name) {
                errors.push(ResolveError::validation(format!(
                    "Model '{}' relation '{}' references unknown model '{}'",
                    model.name, relation.name, target_name
                )));
            }
        }

        errors
    }

    /// Validate that index fields reference actual columns or valid JSONB expressions
    fn validate_indexes(
        &self,
        model: &crate::ast::Model,
        field_names: &HashSet<&String>,
    ) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Known sub-keys of audit_metadata JSONB fields
        const AUDIT_METADATA_KEYS: &[&str] = &[
            "created_at",
            "updated_at",
            "deleted_at",
            "created_by",
            "updated_by",
            "deleted_by",
        ];

        // Collect JSONB field info for sub-key resolution
        let has_audit_metadata = model
            .fields
            .iter()
            .any(|f| f.has_attribute("audit_metadata"));

        // Collect JSONB default keys for data fields
        let jsonb_data_keys: HashSet<String> = model.fields.iter()
            .filter(|f| {
                matches!(f.type_ref, crate::ast::TypeRef::Primitive(crate::ast::PrimitiveType::Json))
                    || matches!(&f.type_ref, crate::ast::TypeRef::Optional(inner)
                        if matches!(inner.as_ref(), crate::ast::TypeRef::Primitive(crate::ast::PrimitiveType::Json)))
            })
            .filter_map(|f| f.default_value())
            .flat_map(|v| {
                // Extract the default string from the AttributeValue
                let default_str = match v {
                    crate::ast::AttributeValue::String(s) => s.clone(),
                    other => format!("{:?}", other),
                };
                // Extract JSON keys: find "key": patterns
                let mut keys = Vec::new();
                let mut chars = default_str.chars().peekable();
                while let Some(ch) = chars.next() {
                    if ch == '"' {
                        let mut key = String::new();
                        while let Some(&next) = chars.peek() {
                            if next == '"' {
                                chars.next();
                                break;
                            }
                            key.push(next);
                            chars.next();
                        }
                        // Check if followed by ':' (it's a JSON key)
                        // Skip whitespace
                        while let Some(&next) = chars.peek() {
                            if next == ' ' || next == '\t' {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Some(&':') = chars.peek() {
                            keys.push(key);
                        }
                    }
                }
                keys
            })
            .collect();

        for index in &model.indexes {
            for field_name in &index.fields {
                // Skip JSONB expressions (e.g., "(data->>'field')")
                if field_name.contains("->>") {
                    continue;
                }

                // Check if it's a real column
                if field_names.contains(field_name) {
                    continue;
                }

                // Check if it's a known audit_metadata sub-key
                if has_audit_metadata && AUDIT_METADATA_KEYS.contains(&field_name.as_str()) {
                    continue; // Valid — generator will resolve to ((metadata->>'field'))
                }

                // Check if it's a known JSONB data sub-key
                if jsonb_data_keys.contains(field_name.as_str()) {
                    continue; // Valid — generator will resolve to ((data->>'field'))
                }

                // Unknown field in index
                errors.push(ResolveError::validation(format!(
                    "Model '{}' index references unknown field '{}'. \
                     Available columns: {}",
                    model.name,
                    field_name,
                    model
                        .fields
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }

        errors
    }

    fn validate_enum(&self, enum_def: &crate::ast::EnumDef) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Check for at least one variant
        if enum_def.variants.is_empty() {
            errors.push(ResolveError::validation(format!(
                "Enum '{}' has no variants",
                enum_def.name
            )));
        }

        // PHASE 2: Check for at least one default variant
        let has_default = enum_def
            .variants
            .iter()
            .any(|v| v.attributes.iter().any(|a| a.name == "default"));
        if !has_default && !enum_def.variants.is_empty() {
            errors.push(ResolveError::validation(format!(
                "Enum '{}' has no default variant (add 'default: true' to one variant)",
                enum_def.name
            )));
        }

        // Check for duplicate variant names
        let mut variant_names = HashSet::new();
        for variant in &enum_def.variants {
            if !variant_names.insert(&variant.name) {
                errors.push(ResolveError::validation(format!(
                    "Enum '{}' has duplicate variant '{}'",
                    enum_def.name, variant.name
                )));
            }
        }

        errors
    }

    fn validate_hook(&self, hook: &crate::ast::Hook) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Validate state machine if present
        if let Some(ref sm) = hook.state_machine {
            errors.extend(self.validate_state_machine(sm, &hook.name));
        }

        // Validate rules have conditions and messages
        for rule in &hook.rules {
            // A message is what a person reads when the rule refuses them, so
            // an assertion without one is a real defect. But not every rule
            // asserts: some declare behaviour instead, such as an idempotency
            // key (`unique_on` + `on_conflict`), and those have nothing to say
            // because they never refuse anything.
            //
            // A rule with no assertion parses to the trivial condition `true`,
            // which is how they are told apart. Demanding a message from the
            // declarative ones failed ten modules outright once their hook
            // files started being read rather than silently discarded.
            let asserts_something = {
                use crate::ast::expressions::{Expression, Literal};
                match &rule.condition {
                    // No `condition:` in the source parses to an empty raw
                    // expression; the default when one is absent entirely is
                    // the trivial `true`. Neither asserts anything.
                    Expression::Raw(text) => !text.trim().is_empty(),
                    Expression::Literal(Literal::Bool(true)) => false,
                    _ => true,
                }
            };
            if rule.message.is_empty() && asserts_something {
                errors.push(ResolveError::validation(format!(
                    "Rule '{}' in hook '{}' asserts a condition but has no message, \
                     so a caller it refuses is told nothing",
                    rule.name, hook.name
                )));
            }
            // ADR-0015: a service-only invariant is reachable from raw-SQL write
            // paths with no DB backstop — the justification is what makes that
            // risk a reviewed decision instead of an accident.
            if rule.enforcement == Enforcement::Service
                && rule
                    .justification
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                errors.push(ResolveError::validation(format!(
                    "Rule '{}' in hook '{}' declares enforcement: service without a \
                     justification — state why this invariant lives in the service \
                     layer instead of the database (ADR-0015)",
                    rule.name, hook.name
                )));
            }
        }

        errors
    }

    fn validate_workflow(&self, workflow: &crate::ast::Workflow) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Validate workflow has at least one step
        if workflow.steps.is_empty() {
            errors.push(ResolveError::validation(format!(
                "Workflow '{}' has no steps",
                workflow.name
            )));
        }

        errors
    }

    fn validate_state_machine(
        &self,
        sm: &crate::ast::StateMachine,
        workflow_name: &str,
    ) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Check for at least one state
        if sm.states.is_empty() {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has no states",
                    workflow_name
                ),
            });
            return errors;
        }

        // Check for exactly one initial state
        let initial_count = sm.states.iter().filter(|s| s.initial).count();
        if initial_count == 0 {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has no initial state (use @initial)",
                    workflow_name
                ),
            });
        } else if initial_count > 1 {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has {} initial states (should be 1)",
                    workflow_name, initial_count
                ),
            });
        }

        // Check for at least one final state — a transition exiting to the null
        // boundary counts: clearing the field ends the lifecycle.
        let final_count = sm.states.iter().filter(|s| s.final_state).count();
        let exits_to_null = sm
            .transitions
            .iter()
            .any(|t| t.to == crate::ast::NULL_STATE);
        if final_count == 0 && !exits_to_null {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has no final state (use @final)",
                    workflow_name
                ),
            });
        }

        // Check transition states exist
        let state_names: HashSet<_> = sm.states.iter().map(|s| s.name.as_str()).collect();

        // A declared state must not claim the reserved null-boundary name.
        if state_names.contains(crate::ast::NULL_STATE) {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' declares a state named '{}' — \
                     that name is reserved for the null boundary of a nullable field",
                    workflow_name,
                    crate::ast::NULL_STATE
                ),
            });
        }

        for transition in &sm.transitions {
            // Check source states
            for from in &transition.from {
                if from != "*"
                    && from != crate::ast::NULL_STATE
                    && !state_names.contains(from.as_str())
                {
                    errors.push(ResolveError::StateMachineError {
                        message: format!(
                            "Transition '{}' in workflow '{}' references unknown source state '{}'",
                            transition.name, workflow_name, from
                        ),
                    });
                }
            }

            // Check target state
            if transition.to != crate::ast::NULL_STATE && !state_names.contains(transition.to.as_str())
            {
                errors.push(ResolveError::StateMachineError {
                    message: format!(
                        "Transition '{}' in workflow '{}' references unknown target state '{}'",
                        transition.name, workflow_name, transition.to
                    ),
                });
            }
        }

        // Check for unreachable states (states that can't be reached from initial)
        // TODO: Implement reachability analysis

        errors
    }
}

/// Non-fatal declarations audit (ADR-0014 and friends) — advisory messages that
/// must NEVER enter the `Vec<ResolveError>` flow (that would break generate on
/// legacy modules). Callers print them; they change nothing.
///
/// Current checks (company fence):
/// - a declared fence (`strict`/`shared_blank`/`shared_tree`) with no fenced
///   model at all — the declaration has no effect;
/// - `shared_blank` where every `company_id` column is NOT NULL — the shared
///   NULL arm is dead, and `strict` is probably what was meant.
pub fn declaration_warnings(schema: &ModuleSchema) -> Vec<String> {
    let mut warnings = Vec::new();
    warnings.extend(fence_warnings(schema));
    warnings.extend(scheduled_job_warnings(schema));
    warnings.extend(declaration_only_warnings(schema));
    warnings
}

/// Fence warnings for [`declaration_warnings`] — see ADR-0014 and ADR-0029.
///
/// Under ADR-0029 modules are tenant-agnostic: an absent `company_fence:` declaration
/// is the intended end state and warns nothing (the strip sweep is tracked by
/// `validate-workspace`'s progress note, which counts modules still declaring). The
/// warnings below fire only while a declaration survives and says something odd.
fn fence_warnings(schema: &ModuleSchema) -> Vec<String> {
    let Some(fence) = schema.company_fence else {
        return Vec::new();
    };

    let fenced: Vec<&crate::ast::Model> = schema
        .models
        .iter()
        .filter(|m| {
            m.fields
                .iter()
                .any(|f| f.name == "company_id" && !f.has_attribute("global"))
        })
        .collect();

    let mut warnings = Vec::new();

    if fenced.is_empty() && fence != CompanyFence::None {
        warnings.push(format!(
            "company_fence: {fence:?} is declared but no model carries a non-@global \
             'company_id' column — the declaration has no effect"
        ));
        return warnings;
    }

    if fence == CompanyFence::SharedBlank {
        let all_required = fenced.iter().all(|m| {
            m.fields
                .iter()
                .filter(|f| f.name == "company_id" && !f.has_attribute("global"))
                .all(|f| !f.type_ref.is_optional())
        });
        if all_required {
            warnings.push(
                "company_fence: shared_blank but every company_id column is NOT NULL — the \
                 shared-NULL arm can never match; strict is probably what was meant"
                    .to_string(),
            );
        }
    }

    // Org fence (ADR-0028): a strict declaration with no org-scoped model is dead
    // vocabulary — it emits nothing. Warning-grade (the company twin treats the
    // analogous case the same): a module may legitimately declare ahead of its
    // re-key sweep.
    if schema.org_fence == Some(OrgFence::Strict)
        && !schema.models.iter().any(|m| {
            m.fields
                .iter()
                .any(|f| f.name == "org_unit_id" && !f.has_attribute("global"))
        })
    {
        warnings.push(
            "org_fence: strict is declared but no model carries an 'org_unit_id' column — \
             the declaration has no effect"
                .to_string(),
        );
    }

    warnings
}

/// The declared name of a job posture (`SelfArming` → `"self_arming"`) —
/// validator messages quote the declaration back at its author.
fn job_posture_name(posture: crate::ast::JobPosture) -> &'static str {
    use crate::ast::JobPosture;
    match posture {
        JobPosture::Pull => "pull",
        JobPosture::SelfArming => "self_arming",
        JobPosture::HostRiding => "host_riding",
        JobPosture::ReadTimeLazy => "read_time_lazy",
        JobPosture::InactiveThenIcp => "inactive_then_icp",
        JobPosture::AutovacuumRide => "autovacuum_ride",
    }
}

/// What the schema declares that nothing generated runs, for [`declaration_warnings`] and
/// `schema doctor`: hook `events:` (no publishing code is generated; the module publishes them
/// by hand) and `schema/workflows` (parsed and validated; no generated code runs them). Said on
/// every generation so nobody takes a declaration for behaviour.
pub fn declaration_only_warnings(schema: &ModuleSchema) -> Vec<String> {
    use crate::rules::events::EventStatus;
    let mut out = Vec::new();
    let mut by_file: indexmap::IndexMap<&str, Vec<&str>> = indexmap::IndexMap::new();
    let by_triggers = crate::generators::triggers::emitted_events(schema);
    for event in schema
        .rule_register
        .events
        .iter()
        .filter(|e| e.status() == EventStatus::HandStaged && !by_triggers.contains_key(&e.name))
    {
        by_file.entry(event.source.file.as_str()).or_default().push(event.name.as_str());
    }
    for (file, events) in by_file {
        out.push(format!(
            "{file} declares event(s) {} under `events:` with no `on:`: a typed payload is generated, but no generated code stages them; hand code does (add `on:` to have the write hooks stage one)",
            events.join(", ")
        ));
    }
    for workflow in schema.workflows.iter().filter(|w| !w.runs_on_engine() && w.implemented_by.is_none()) {
        out.push(format!(
            "workflow '{}' (schema/workflows) is declaration-only: it is parsed and validated, and no generated code runs it; set `config.executor: engine` to have the engine run it, or name the hand function that does with `implemented_by`",
            workflow.name
        ));
    }
    out
}

/// What is wrong with the workflows a module declares: an `executor` that is neither `engine` nor
/// `hand`, an `implemented_by` that is not `<file>::<fn>` (or sits on a workflow the engine runs),
/// and, for a workflow on the engine, everything its compiler refuses (an expression outside the
/// set, a wait that is not an equality key, an unknown step, entity, verb or event, a bespoke verb
/// in a module without the extension layout, which has nowhere to put it). Each is an error: a
/// workflow that cannot run must not read as one that does.
pub fn workflow_problems(schema: &ModuleSchema) -> Vec<String> {
    let mut out = Vec::new();
    let extensions = schema.generators_config.as_ref().and_then(|c| c.extensions) == Some(true);
    let mut seen: HashSet<String> = HashSet::new();
    for w in &schema.workflows {
        if !seen.insert(crate::utils::to_snake_case(&w.name)) {
            out.push(format!("workflow '{}' is declared twice", w.name));
        }
        if w.executor_kind().is_none() {
            out.push(format!(
                "workflow '{}': `config.executor: {}` is neither `engine` nor `hand`",
                w.name,
                w.executor.as_deref().unwrap_or_default()
            ));
            continue;
        }
        if let Some(by) = &w.implemented_by {
            match by.split_once("::") {
                Some((file, function)) if !file.trim().is_empty() && !function.trim().is_empty() => {}
                _ => out.push(format!("workflow '{}': `implemented_by: \"{by}\"` is not `<file>::<function>`", w.name)),
            }
            if w.runs_on_engine() {
                out.push(format!(
                    "workflow '{}': `implemented_by` names the hand function that runs a documented workflow, and `executor: engine` runs it generated: say one",
                    w.name
                ));
            }
        }
        if w.runs_on_engine() {
            let compiled = crate::rules::workflows::compile(schema, w);
            if compiled.problems.is_empty() && compiled.required_methods() > 0 && !extensions {
                out.push(format!(
                    "workflow '{}' needs {} hand method(s) ({}) and the module does not use the extension layout (`generators: extensions: true`), which is where they are written",
                    w.name,
                    compiled.required_methods(),
                    compiled
                        .hand
                        .iter()
                        .map(|h| h.verb.clone())
                        .chain(compiled.functions.iter().cloned())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            out.extend(compiled.problems);
        }
    }
    out
}

/// The problems of the declared events the write hooks stage (`on:`): each is an error, since
/// a declaration that cannot be generated would promise an event nothing stages.
pub fn event_problems(schema: &ModuleSchema) -> Vec<String> {
    use crate::rules::events::EventStatus;
    let mut out = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for event in &schema.rule_register.events {
        if !seen.insert(event.name.as_str()) && event.status() == EventStatus::Staged {
            out.push(format!(
                "{}: event `{}` is declared more than once; the staged declaration must be the only one",
                event.source, event.name
            ));
        }
        out.extend(event.problems.iter().map(|p| format!("{}: {p}", event.source)));
    }
    out
}

/// Scheduled-job warnings for [`declaration_warnings`] — see ADR-0020.
fn scheduled_job_warnings(schema: &ModuleSchema) -> Vec<String> {
    schema
        .scheduled_jobs
        .iter()
        .filter(|job| job.posture.is_none())
        .map(|job| {
            format!(
                "scheduled job '{}' declares no posture — pick one of pull, self_arming, \
                 host_riding, read_time_lazy, inactive_then_icp, autovacuum_ride \
                 (ADR-0020); the schedule alone does not say what re-arms the job",
                job.name
            )
        })
        .collect()
}

#[cfg(test)]
mod fk_target_tests {
    use super::*;
    use crate::ast::{Attribute, AttributeValue, Field, Model, PrimitiveType, TypeRef};

    fn id_field() -> Field {
        let mut f = Field::new("id", TypeRef::Primitive(PrimitiveType::Uuid));
        f.attributes.push(Attribute::new("id"));
        f
    }

    fn fk_field(name: &str, target: &str) -> Field {
        let mut f = Field::new(name, TypeRef::Primitive(PrimitiveType::Uuid));
        f.attributes
            .push(Attribute::new("foreign_key").with_arg(AttributeValue::String(target.into())));
        f
    }

    fn schema_with(models: Vec<Model>) -> ModuleSchema {
        let mut s = ModuleSchema::new("test");
        s.models = models;
        s
    }

    fn errors_of(s: &ModuleSchema) -> Vec<String> {
        match SchemaValidator::new(s).validate() {
            Ok(()) => vec![],
            Err(es) => es.into_iter().map(|e| e.to_string()).collect(),
        }
    }

    #[test]
    fn intra_module_fk_to_missing_model_is_rejected() {
        // The phantom, in miniature: an FK naming a model that does not exist in this module.
        let mut child = Model::new("OrgUser");
        child.fields = vec![id_field(), fk_field("organization_id", "Organization.id")];
        let errs = errors_of(&schema_with(vec![child]));
        assert!(
            errs.iter()
                .any(|e| e.contains("no model 'Organization' exists")),
            "a same-module FK to a nonexistent model must be rejected, got: {errs:?}"
        );
    }

    #[test]
    fn intra_module_fk_to_present_model_is_accepted() {
        let mut parent = Model::new("Organization");
        parent.fields = vec![id_field()];
        let mut child = Model::new("OrgUser");
        child.fields = vec![id_field(), fk_field("organization_id", "Organization.id")];
        let errs = errors_of(&schema_with(vec![parent, child]));
        assert!(
            !errs.iter().any(|e| e.contains("@foreign_key")),
            "a valid same-module FK must pass, got: {errs:?}"
        );
    }

    #[test]
    fn cross_module_fk_is_not_flagged_here() {
        // `corpus.Organization.id` can't be checked single-module — this validator must NOT
        // error on it (that would break every legitimate cross-module logical FK). It is the
        // workspace-level pass's job, noted in the validator.
        let mut child = Model::new("OrgUser");
        child.fields = vec![
            id_field(),
            fk_field("organization_id", "corpus.Organization.id"),
        ];
        let errs = errors_of(&schema_with(vec![child]));
        assert!(
            !errs.iter().any(|e| e.contains("@foreign_key")),
            "a cross-module FK must not be flagged by the single-module validator, got: {errs:?}"
        );
    }
}

#[cfg(test)]
mod company_fence_tests {
    use super::*;
    use crate::ast::{Attribute, Field, Model, PrimitiveType, TypeRef};

    fn id_field() -> Field {
        let mut f = Field::new("id", TypeRef::Primitive(PrimitiveType::Uuid));
        f.attributes.push(Attribute::new("id"));
        f
    }

    fn schema_with(models: Vec<Model>) -> ModuleSchema {
        let mut s = ModuleSchema::new("test");
        s.models = models;
        s
    }

    fn errors_of(s: &ModuleSchema) -> Vec<String> {
        match SchemaValidator::new(s).validate() {
            Ok(()) => vec![],
            Err(es) => es.into_iter().map(|e| e.to_string()).collect(),
        }
    }

    fn company_model(global: bool) -> Model {
        let mut f = Field::new("company_id", TypeRef::Primitive(PrimitiveType::Uuid));
        // Not a cross-module FK test — silence the _id FK check.
        f.attributes
            .push(Attribute::new("exclude_from_foreign_key_check"));
        if global {
            f.attributes.push(Attribute::new("global"));
        }
        let mut m = Model::new("Scoped");
        m.fields = vec![id_field(), f];
        m
    }

    fn schema_with_fence(models: Vec<Model>, fence: Option<CompanyFence>) -> ModuleSchema {
        let mut s = schema_with(models);
        s.company_fence = fence;
        s
    }

    #[test]
    fn none_fence_with_stray_company_column_is_fatal() {
        let s = schema_with_fence(vec![company_model(false)], Some(CompanyFence::None));
        let errs = errors_of(&s);
        assert!(
            errs.iter()
                .any(|e| e.contains("company_fence: none") && e.contains("'Scoped'")),
            "none + non-@global company_id must be a hard error, got: {errs:?}"
        );
    }

    #[test]
    fn none_fence_with_global_or_no_column_is_fine() {
        let global_only = schema_with_fence(vec![company_model(true)], Some(CompanyFence::None));
        assert!(
            errors_of(&global_only).is_empty(),
            "@global column under none is a deliberate unfence"
        );
        let mut bare = Model::new("Bare");
        bare.fields = vec![id_field()];
        let no_column = schema_with_fence(vec![bare], Some(CompanyFence::None));
        assert!(errors_of(&no_column).is_empty());
    }

    #[test]
    fn undeclared_and_fenced_declarations_are_fine() {
        for fence in [
            None,
            Some(CompanyFence::Strict),
            Some(CompanyFence::SharedBlank),
            Some(CompanyFence::SharedTree),
        ] {
            let s = schema_with_fence(vec![company_model(false)], fence);
            assert!(
                errors_of(&s).is_empty(),
                "{fence:?} with a fenced model must validate"
            );
        }
    }

    #[test]
    fn warns_when_declared_fence_has_no_fenced_model() {
        let mut bare = Model::new("Bare");
        bare.fields = vec![id_field()];
        let s = schema_with_fence(vec![bare], Some(CompanyFence::Strict));
        let warnings = declaration_warnings(&s);
        assert!(
            warnings.iter().any(|w| w.contains("no effect")),
            "expected a no-effect warning, got: {warnings:?}"
        );
    }

    #[test]
    fn warns_when_shared_blank_null_arm_is_dead() {
        let mut required = company_model(false);
        required.fields[1].type_ref = TypeRef::optional(TypeRef::Primitive(PrimitiveType::Uuid));
        // make it non-optional again — the dead-arm case
        let mut not_null = company_model(false);
        not_null.fields[1].type_ref = TypeRef::Primitive(PrimitiveType::Uuid);
        let s = schema_with_fence(vec![not_null], Some(CompanyFence::SharedBlank));
        let warnings = declaration_warnings(&s);
        assert!(
            warnings.iter().any(|w| w.contains("NOT NULL")),
            "expected a dead-NULL-arm warning, got: {warnings:?}"
        );
        // and a nullable company_id under shared_blank is the healthy shape
        let s = schema_with_fence(vec![required], Some(CompanyFence::SharedBlank));
        assert!(
            !declaration_warnings(&s)
                .iter()
                .any(|w| w.contains("NOT NULL")),
            "nullable company_id must not warn"
        );
    }

    #[test]
    fn undeclared_module_is_silent_under_composition_tenancy() {
        // ADR-0029: modules are tenant-agnostic; an absent `company_fence:`
        // declaration is the intended end state and must not warn (or gate)
        // anywhere. Sweep progress lives in validate-workspace's note, which
        // counts modules still DECLARING.
        let s = schema_with_fence(vec![company_model(false)], None);
        let warnings = declaration_warnings(&s);
        assert!(
            warnings.is_empty(),
            "undeclared tenancy must be warning-free, got: {warnings:?}"
        );
    }
}

#[cfg(test)]
mod org_fence_tests {
    use super::*;
    use crate::ast::{Attribute, Field, Model, PrimitiveType, TypeRef};

    fn id_field() -> Field {
        let mut f = Field::new("id", TypeRef::Primitive(PrimitiveType::Uuid));
        f.attributes.push(Attribute::new("id"));
        f
    }

    fn org_model() -> Model {
        // The scoping key is NOT NULL by law; @exclude_from_foreign_key_check because
        // organization.org_units lives in another module (the pilot's convention).
        let mut f = Field::new("org_unit_id", TypeRef::Primitive(PrimitiveType::Uuid));
        f.attributes.push(Attribute::new("required"));
        f.attributes
            .push(Attribute::new("exclude_from_foreign_key_check"));
        let mut m = Model::new("Warehouse");
        m.fields = vec![id_field(), f];
        m
    }

    fn schema_with_org_fence(models: Vec<Model>, org_fence: Option<OrgFence>) -> ModuleSchema {
        let mut s = ModuleSchema::new("test");
        s.models = models;
        s.org_fence = org_fence;
        s
    }

    fn errors_of(s: &ModuleSchema) -> Vec<String> {
        match SchemaValidator::new(s).validate() {
            Ok(()) => vec![],
            Err(es) => es.into_iter().map(|e| e.to_string()).collect(),
        }
    }

    #[test]
    fn org_column_without_a_strict_declaration_is_fatal() {
        for fence in [None, Some(OrgFence::None)] {
            let s = schema_with_org_fence(vec![org_model()], fence);
            let errs = errors_of(&s);
            assert!(
                errs.iter().any(|e| e.contains("org_fence")),
                "{fence:?} + org_unit_id must be a hard error naming the declaration, got: {errs:?}"
            );
        }
    }

    #[test]
    fn declared_strict_with_an_org_column_validates() {
        let s = schema_with_org_fence(vec![org_model()], Some(OrgFence::Strict));
        assert!(
            errors_of(&s).is_empty(),
            "the sanctioned post-re-key shape must validate"
        );
    }

    #[test]
    fn both_scoping_columns_on_one_model_is_fatal() {
        let mut m = org_model();
        let mut c = Field::new("company_id", TypeRef::Primitive(PrimitiveType::Uuid));
        c.attributes
            .push(Attribute::new("exclude_from_foreign_key_check"));
        m.fields.push(c);
        let s = schema_with_org_fence(vec![m], Some(OrgFence::Strict));
        let errs = errors_of(&s);
        assert!(
            errs.iter()
                .any(|e| e.contains("both 'company_id' and 'org_unit_id'")),
            "the column swap is a migration, not a schema state, got: {errs:?}"
        );
    }

    #[test]
    fn global_or_optional_org_column_is_fatal() {
        let mut global = org_model();
        global.fields[1].attributes.push(Attribute::new("global"));
        let errs = errors_of(&schema_with_org_fence(vec![global], Some(OrgFence::Strict)));
        assert!(
            errs.iter().any(|e| e.contains("@global")),
            "a global org-scoped row is a contradiction, got: {errs:?}"
        );

        let mut optional = org_model();
        optional.fields[1].type_ref = TypeRef::optional(TypeRef::Primitive(PrimitiveType::Uuid));
        let errs = errors_of(&schema_with_org_fence(
            vec![optional],
            Some(OrgFence::Strict),
        ));
        assert!(
            errs.iter().any(|e| e.contains("NOT NULL")),
            "the org scoping key is NOT NULL by law, got: {errs:?}"
        );
    }

    #[test]
    fn org_root_shared_off_the_scoping_key_is_fatal() {
        let mut m = org_model();
        let mut code = Field::new("code", TypeRef::Primitive(PrimitiveType::String));
        code.attributes.push(Attribute::new("org_root_shared"));
        m.fields.push(code);
        let errs = errors_of(&schema_with_org_fence(vec![m], Some(OrgFence::Strict)));
        assert!(
            errs.iter()
                .any(|e| e.contains("@org_root_shared") && e.contains("'code'")),
            "the attribute belongs on the scoping key, got: {errs:?}"
        );
        // On the key itself it is the sanctioned shared-row shape.
        let mut sane = org_model();
        sane.fields[1]
            .attributes
            .push(Attribute::new("org_root_shared"));
        assert!(errors_of(&schema_with_org_fence(vec![sane], Some(OrgFence::Strict))).is_empty());
    }

    #[test]
    fn strict_declaration_with_no_org_model_warns() {
        let mut bare = Model::new("Bare");
        bare.fields = vec![id_field()];
        let mut s = schema_with_org_fence(vec![bare], Some(OrgFence::Strict));
        // The honest no-company shape — otherwise the missing company declaration
        // short-circuits the warning list before the org block is reached.
        s.company_fence = Some(CompanyFence::None);
        let warnings = declaration_warnings(&s);
        assert!(
            warnings.iter().any(|w| w.contains("org_fence")),
            "expected a no-effect org warning, got: {warnings:?}"
        );
    }
}

#[cfg(test)]
mod enforcement_tests {
    use super::*;
    use crate::ast::{Enforcement, Hook, Rule};

    fn schema_with_hook(rule: Rule) -> ModuleSchema {
        let mut hook = Hook::new("Order", "Order");
        hook.rules = vec![rule];
        let mut s = ModuleSchema::new("test");
        s.hooks.push(hook);
        s
    }

    fn rule_with(enforcement: Enforcement, justification: Option<&str>) -> Rule {
        Rule {
            name: "positive_total".into(),
            message: "total must be positive".into(),
            condition: crate::ast::expressions::Expression::Raw("total > 0".into()),
            enforcement,
            justification: justification.map(String::from),
            ..Default::default()
        }
    }

    fn errors_of(s: &ModuleSchema) -> Vec<String> {
        match SchemaValidator::new(s).validate() {
            Ok(()) => vec![],
            Err(es) => es.into_iter().map(|e| e.to_string()).collect(),
        }
    }

    #[test]
    fn service_without_justification_is_fatal() {
        let s = schema_with_hook(rule_with(Enforcement::Service, None));
        let errs = errors_of(&s);
        assert!(
            errs.iter()
                .any(|e| e.contains("enforcement: service without a justification")),
            "service-enforced rule must demand a justification, got: {errs:?}"
        );
        // Blank justification is just as missing.
        let s = schema_with_hook(rule_with(Enforcement::Service, Some("   ")));
        assert!(
            errors_of(&s)
                .iter()
                .any(|e| e.contains("without a justification")),
            "whitespace-only justification must not count"
        );
    }

    #[test]
    fn service_with_justification_and_other_modes_are_fine() {
        let s = schema_with_hook(rule_with(
            Enforcement::Service,
            Some("cross-model check; needs both rows in memory"),
        ));
        assert!(
            errors_of(&s).is_empty(),
            "justified service rule must validate"
        );
        let s = schema_with_hook(rule_with(Enforcement::Db, None));
        assert!(
            errors_of(&s).is_empty(),
            "db (default) needs no justification"
        );
        let s = schema_with_hook(rule_with(Enforcement::Both, None));
        assert!(
            errors_of(&s).is_empty(),
            "both needs no justification — the DB backstop exists"
        );
    }

    #[test]
    fn default_enforcement_is_db() {
        let rule = rule_with(Enforcement::Db, None);
        let legacy = Rule {
            name: "x".into(),
            message: "m".into(),
            ..Default::default()
        };
        assert_eq!(legacy.enforcement, Enforcement::Db);
        assert_eq!(rule.enforcement, Enforcement::Db);
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::ast::{
        Attribute, Field, Hook, Lifecycle, LifecycleShape, Model, PrimitiveType, Relation,
        StateMachine, TypeRef,
    };

    fn id_field() -> Field {
        let mut f = Field::new("id", TypeRef::Primitive(PrimitiveType::Uuid));
        f.attributes.push(Attribute::new("id"));
        f
    }

    fn lifecycle_field(name: &str, lifecycle: Lifecycle) -> Field {
        let mut f = Field::new(name, TypeRef::Primitive(PrimitiveType::String));
        f.lifecycle = Some(lifecycle);
        f
    }

    fn schema_with(model: Model) -> ModuleSchema {
        let mut s = ModuleSchema::new("test");
        s.models.push(model);
        s
    }

    fn errors_of(s: &ModuleSchema) -> Vec<String> {
        match SchemaValidator::new(s).validate() {
            Ok(()) => vec![],
            Err(es) => es.into_iter().map(|e| e.to_string()).collect(),
        }
    }

    fn stage_model() -> Model {
        let mut m = Model::new("Festival");
        m.fields = vec![id_field()];
        m
    }

    #[test]
    fn split_without_driver_is_fatal() {
        let mut m = stage_model();
        m.fields.push(lifecycle_field(
            "sub_state",
            Lifecycle {
                shape: LifecycleShape::Split,
                ..Default::default()
            },
        ));
        let errs = errors_of(&schema_with(m));
        assert!(
            errs.iter()
                .any(|e| e.contains("split requires a 'driver:'")),
            "split must demand a driver, got: {errs:?}"
        );
    }

    #[test]
    fn split_with_unknown_driver_is_fatal_but_known_driver_passes() {
        let mut m = stage_model();
        m.fields.push(Field::new(
            "stage",
            TypeRef::Primitive(PrimitiveType::String),
        ));
        m.fields.push(lifecycle_field(
            "sub_state",
            Lifecycle {
                shape: LifecycleShape::Split,
                driver: Some("ghost".to_string()),
                ..Default::default()
            },
        ));
        let errs = errors_of(&schema_with(m));
        assert!(
            errs.iter()
                .any(|e| e.contains("driver 'ghost' is not a field")),
            "driver must resolve to a same-model field, got: {errs:?}"
        );

        let mut m = stage_model();
        m.fields.push(Field::new(
            "stage",
            TypeRef::Primitive(PrimitiveType::String),
        ));
        m.fields.push(lifecycle_field(
            "sub_state",
            Lifecycle {
                shape: LifecycleShape::Split,
                driver: Some("stage".to_string()),
                ..Default::default()
            },
        ));
        assert!(
            errors_of(&schema_with(m)).is_empty(),
            "split with a real driver must validate"
        );
    }

    #[test]
    fn driver_cannot_be_the_field_itself() {
        let mut m = stage_model();
        m.fields.push(lifecycle_field(
            "stage",
            Lifecycle {
                shape: LifecycleShape::Split,
                driver: Some("stage".to_string()),
                ..Default::default()
            },
        ));
        let errs = errors_of(&schema_with(m));
        assert!(
            errs.iter()
                .any(|e| e.contains("cannot drive its own lifecycle")),
            "self-driving driver must be rejected, got: {errs:?}"
        );
    }

    #[test]
    fn stage_ref_needs_a_relation() {
        let mut m = stage_model();
        m.fields.push(lifecycle_field(
            "stage_id",
            Lifecycle {
                shape: LifecycleShape::StageRef,
                ..Default::default()
            },
        ));
        let errs = errors_of(&schema_with(m));
        assert!(
            errs.iter()
                .any(|e| e.contains("stage_ref requires a relation")),
            "stage_ref on a plain column must be rejected, got: {errs:?}"
        );

        // With a relation named after the (suffix-stripped) field it passes
        // (the relation target must be a real model in the schema; the column
        // carries the relation, so the _id FK check is satisfied by exemption).
        let mut stage = Model::new("Stage");
        stage.fields = vec![id_field()];
        let mut m = stage_model();
        let mut f = lifecycle_field(
            "stage_id",
            Lifecycle {
                shape: LifecycleShape::StageRef,
                ..Default::default()
            },
        );
        f.attributes
            .push(Attribute::new("exclude_from_foreign_key_check"));
        m.fields.push(f);
        m.relations.push(Relation {
            name: "stage".to_string(),
            target: TypeRef::Custom("Stage".to_string()),
            ..Default::default()
        });
        let mut s = schema_with(m);
        s.models.push(stage);
        assert!(
            errors_of(&s).is_empty(),
            "stage_ref with a matching relation must validate, got: {:?}",
            errors_of(&s)
        );
    }

    fn hand_set_model(machine: Option<&str>) -> Model {
        let mut m = stage_model();
        m.fields.push(lifecycle_field(
            "state",
            Lifecycle {
                shape: LifecycleShape::HandSet,
                state_machine: machine.map(|s| s.to_string()),
                ..Default::default()
            },
        ));
        m
    }

    fn hook_guarding(field: &str) -> Hook {
        let mut h = Hook::new("FestivalHook", "Festival");
        let mut sm = StateMachine {
            field: field.to_string(),
            ..Default::default()
        };
        sm.states = vec![
            crate::ast::hook::State::new("draft").initial(),
            crate::ast::hook::State::new("done").final_state(),
        ];
        h.state_machine = Some(sm);
        h
    }

    #[test]
    fn hand_set_with_missing_machine_is_fatal() {
        let mut s = schema_with(hand_set_model(Some("ghost_machine")));
        assert!(
            errors_of(&s)
                .iter()
                .any(|e| e.contains("'ghost_machine' does not exist")),
            "hand_set must name a real machine, got: {:?}",
            errors_of(&s)
        );
    }

    #[test]
    fn hand_set_with_machine_guarding_another_field_is_fatal() {
        let mut s = schema_with(hand_set_model(Some("FestivalHook")));
        s.hooks.push(hook_guarding("other_field"));
        assert!(
            errors_of(&s)
                .iter()
                .any(|e| e.contains("guards field 'other_field', not 'state'")),
            "the machine must guard exactly the declared field, got: {:?}",
            errors_of(&s)
        );
    }

    #[test]
    fn hand_set_with_matching_machine_or_no_machine_is_fine() {
        let mut s = schema_with(hand_set_model(Some("FestivalHook")));
        s.hooks.push(hook_guarding("state"));
        assert!(
            errors_of(&s).is_empty(),
            "matching machine must validate, got: {:?}",
            errors_of(&s)
        );

        let s = schema_with(hand_set_model(None));
        assert!(
            errors_of(&s).is_empty(),
            "hand_set without state_machine is advisory-only and must validate"
        );
    }

    #[test]
    fn shape_names_round_trip_the_declared_vocabulary() {
        assert_eq!(
            LifecycleShape::from_name("hand_set"),
            Some(LifecycleShape::HandSet)
        );
        assert_eq!(
            LifecycleShape::from_name("stage_ref"),
            Some(LifecycleShape::StageRef)
        );
        assert_eq!(LifecycleShape::from_name("nope"), None);
        for name in [
            "projection",
            "hand_set",
            "hybrid",
            "split",
            "stage_ref",
            "window",
            "virtual",
            "label",
            "inert",
            "none",
        ] {
            let shape =
                LifecycleShape::from_name(name).unwrap_or_else(|| panic!("'{name}' must parse"));
            assert_eq!(shape.shape_name(), name, "shape_name must round-trip");
        }
    }
}

#[cfg(test)]
mod scheduled_job_tests {
    use super::*;
    use crate::ast::{CommitPolicy, JobPosture, ModuleSchema, ScheduledJob};

    fn job_with(
        posture: Option<JobPosture>,
        triggers: Vec<String>,
        pickup_lock: bool,
    ) -> ScheduledJob {
        ScheduledJob {
            name: "nightly_gc".to_string(),
            schedule: "0 3 * * *".to_string(),
            handler: "gc::run".to_string(),
            posture,
            triggers,
            commit_policy: Some(CommitPolicy::CommitPerBatch),
            pickup_lock,
        }
    }

    fn errors_of(jobs: Vec<ScheduledJob>) -> Vec<String> {
        let mut s = ModuleSchema::new("test");
        s.scheduled_jobs = jobs;
        match SchemaValidator::new(&s).validate() {
            Ok(()) => vec![],
            Err(es) => es.into_iter().map(|e| e.to_string()).collect(),
        }
    }

    fn warnings_of(jobs: Vec<ScheduledJob>) -> Vec<String> {
        let mut s = ModuleSchema::new("test");
        s.scheduled_jobs = jobs;
        declaration_warnings(&s)
    }

    #[test]
    fn undeclared_posture_warns_but_never_errors() {
        let job = job_with(None, vec![], false);
        let warns = warnings_of(vec![job.clone()]);
        assert!(
            warns
                .iter()
                .any(|w| w.contains("'nightly_gc' declares no posture")),
            "absent posture must warn, got: {warns:?}"
        );
        // Legacy jobs (all 49 backbone indexes today) never fail validation.
        assert!(
            errors_of(vec![job]).is_empty(),
            "absent posture must not error"
        );
    }

    #[test]
    fn queue_draining_postures_demand_pickup_lock() {
        for posture in [
            JobPosture::Pull,
            JobPosture::SelfArming,
            JobPosture::HostRiding,
            JobPosture::AutovacuumRide,
        ] {
            let errs = errors_of(vec![job_with(
                Some(posture),
                vec!["mail.created".to_string()],
                false,
            )]);
            assert!(
                errs.iter()
                    .any(|e| e.contains("without pickup_lock: true") && e.contains("MMB-4")),
                "{posture:?} without pickup_lock must be a hard error, got: {errs:?}"
            );

            let ok = errors_of(vec![job_with(
                Some(posture),
                vec!["mail.created".to_string()],
                true,
            )]);
            assert!(
                ok.is_empty(),
                "{posture:?} with pickup_lock must validate, got: {ok:?}"
            );
        }
    }

    #[test]
    fn read_time_lazy_and_inactive_then_icp_are_exempt_from_the_lock() {
        for posture in [JobPosture::ReadTimeLazy, JobPosture::InactiveThenIcp] {
            let errs = errors_of(vec![job_with(Some(posture), vec![], false)]);
            assert!(
                errs.is_empty(),
                "{posture:?} does not claim rows on a schedule and must be exempt, got: {errs:?}"
            );
        }
    }

    #[test]
    fn self_arming_without_triggers_is_fatal() {
        let errs = errors_of(vec![job_with(Some(JobPosture::SelfArming), vec![], true)]);
        assert!(
            errs.iter()
                .any(|e| e.contains("self_arming with no triggers")),
            "self_arming must name its re-arming events, got: {errs:?}"
        );

        let ok = errors_of(vec![job_with(
            Some(JobPosture::SelfArming),
            vec!["registration.confirmed".to_string()],
            true,
        )]);
        assert!(
            ok.is_empty(),
            "self_arming with triggers and a lock must validate, got: {ok:?}"
        );
    }

    #[test]
    fn posture_names_match_the_declared_vocabulary() {
        assert_eq!(job_posture_name(JobPosture::SelfArming), "self_arming");
        assert_eq!(
            job_posture_name(JobPosture::InactiveThenIcp),
            "inactive_then_icp"
        );
    }
}

#[cfg(test)]
mod route_name_collision_tests {
    use super::*;
    use crate::ast::Model;

    fn errors_of(models: Vec<Model>) -> Vec<String> {
        let mut s = ModuleSchema::new("test");
        s.models = models;
        match SchemaValidator::new(&s).validate() {
            Ok(()) => vec![],
            Err(es) => es.into_iter().map(|e| e.to_string()).collect(),
        }
    }

    #[test]
    fn two_models_emitting_the_same_route_function_are_rejected() {
        // `Announcement`'s read-only routes and `AnnouncementRead`'s full routes
        // are both `create_announcement_read_routes`.
        let errs = errors_of(vec![Model::new("Announcement"), Model::new("AnnouncementRead")]);
        assert!(
            errs.iter().any(|e| e.contains("create_announcement_read_routes")
                && e.contains("Announcement")
                && e.contains("AnnouncementRead")),
            "a route function emitted twice must be rejected, got: {errs:?}"
        );
    }

    #[test]
    fn a_model_without_generated_routes_cannot_collide() {
        let mut ledger = Model::new("AnnouncementRead");
        ledger.disabled_generators = vec!["handler".into()];
        let errs = errors_of(vec![Model::new("Announcement"), ledger]);
        assert!(
            !errs.iter().any(|e| e.contains("route function")),
            "a model whose handler is disabled emits no routes, got: {errs:?}"
        );
    }
}

/// Workflows the schema declares. No executor is generated for them: a workflow YAML
/// documents a saga a hand service runs, so it is read, never started. A module that
/// still sets `generators: workflows: true` is told the flag does nothing.
pub fn workflow_warnings(schema: &ModuleSchema) -> Vec<String> {
    // A workflow on the engine runs; one naming `implemented_by` documents the function that does.
    let mut out: Vec<String> = schema
        .workflows
        .iter()
        .filter(|w| !w.runs_on_engine() && w.implemented_by.is_none())
        .map(|w| format!("Workflow '{}' is documentation: no executor is generated, so the service that runs it is hand-written", w.name))
        .collect();
    for w in crate::rules::workflows::engine_workflows(schema) {
        out.extend(crate::rules::workflows::compile(schema, w).warnings);
    }
    if schema.generators_config.as_ref().and_then(|c| c.workflows).is_some() {
        out.push("`generators: workflows:` generates nothing: workflow executors are retired, remove the flag".into());
    }
    out
}

/// Generator names in `generators: opt_in:` or `enabled:` that turn on a retired target.
/// A retired target generates nothing, so the entry is dead configuration.
pub fn retired_generator_warnings(schema: &ModuleSchema) -> Vec<String> {
    use crate::generators::GenerationTarget::*;
    let Some(config) = schema.generators_config.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    for (key, names) in [("opt_in", &config.opt_in), ("enabled", &config.enabled)] {
        for name in names.iter().flatten() {
            let target = crate::generators::GenerationTarget::from_str(name);
            if matches!(target, Some(Trigger | Flow | Specification | Validator | UseCase)) {
                out.push(format!("`generators: {key}:` names '{name}', a retired target that generates nothing: remove it"));
            }
        }
    }
    out
}

/// Whether the module is exempt from the hook trigger rule (`generators: legacy_triggers: true`).
fn legacy_triggers(schema: &ModuleSchema) -> bool {
    schema.generators_config.as_ref().and_then(|c| c.legacy_triggers) == Some(true)
}

/// What is wrong with the hook triggers and state enter/exit lists a module declares, when it
/// generates them (it has not opted out with `legacy_triggers`): a key that names no field, verb
/// or state, an `if:` that cannot compile, an `emit:` of an event no file declares, a `set:` that
/// cannot be said, and hand methods in a module without the extension layout, which has nowhere to
/// put them. Each is an error, since a declaration that cannot be honoured must not read as one that is.
pub fn trigger_problems(schema: &ModuleSchema) -> Vec<String> {
    if legacy_triggers(schema) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for t in &schema.rule_register.triggers {
        for problem in &t.problems {
            out.push(format!("{}: {problem}", t.source));
        }
    }
    if schema.generators_config.as_ref().and_then(|c| c.extensions) != Some(true) {
        for model in &schema.models {
            if let Some(first) = crate::generators::triggers::hand_methods(schema, model).first() {
                out.push(format!(
                    "{}: {} has triggers that need hand code (`{}` and {} more), but the module does not use the extension layout (`generators: extensions: true`), which is where the implementation of `{}Triggers` lives",
                    first.source,
                    model.name,
                    first.name,
                    crate::generators::triggers::hand_methods(schema, model).len() - 1,
                    model.name
                ));
            }
        }
    }
    out
}

/// What a module that generates its triggers is told without failing: what became a hand method
/// or was left out of a log, and the triggers whose moment depends on hand code.
pub fn trigger_warnings(schema: &ModuleSchema) -> Vec<String> {
    if legacy_triggers(schema) {
        return legacy_trigger_findings(schema);
    }
    let mut out = Vec::new();
    for t in &schema.rule_register.triggers {
        for note in &t.notes {
            out.push(format!("{}: {note}", t.source));
        }
        let Some(model) = t.bound_model.as_ref() else { continue };
        let machines = || {
            schema
                .rule_register
                .machines
                .iter()
                .filter(move |m| m.bound_model.as_deref() == Some(model.as_str()))
        };
        use crate::rules::triggers::{StateList, TriggerWhen};
        match (&t.when, &t.state_list) {
            (_, Some(StateList::Enter(state))) if machines().any(|m| m.initial.as_deref() == Some(state.as_str())) => out.push(format!(
                "{}: `on_enter {state}` of {model} fires when a transition enters the state; a row created in `{state}` (its initial state) enters it with no transition, so this list never runs on create",
                t.source
            )),
            (Some(TriggerWhen::Transition { verb }), _)
                if machines().any(|m| m.verbs.as_deref() == Some("hand") && m.transitions.iter().any(|tr| tr.verb.as_deref() == Some(verb.as_str()))) =>
            {
                out.push(format!(
                    "{}: `{}` of {model} fires only when the hand-written `{verb}` verb writes through `backbone_core::write_guard::as_transition`: its machine declares `verbs: hand`",
                    t.source, t.key
                ))
            }
            (Some(TriggerWhen::HandFired), _) => out.push(format!(
                "{}: `{}` of {model} names no write, field or transition: it runs only when hand code calls `fire_{}` on the model's write hooks",
                t.source,
                t.key,
                crate::rules::trigger_compile::ident(&t.key)
            )),
            _ => {}
        }
    }
    out
}

/// The old finding for each hook trigger and state enter/exit action a module that kept
/// `legacy_triggers: true` still declares: none is generated, so each promises behaviour nothing
/// performs. It resolves and generates as it always did, and `schema validate` shows these.
pub fn legacy_trigger_findings(schema: &ModuleSchema) -> Vec<String> {
    const FIX: &str = "hook triggers are not generated, so nothing runs them: move the behaviour into the service and declare the events it publishes under `events:` (or drop `legacy_triggers` and the generator runs them)";
    let mut out = Vec::new();
    for hook in &schema.hooks {
        for trigger in &hook.triggers {
            let key = trigger.declared_key.clone().unwrap_or_else(|| trigger_key(&trigger.event));
            out.push(format!(
                "Hook '{}' declares trigger '{key}' with {} action(s); {FIX}",
                hook.name,
                trigger.actions.len()
            ));
        }
        if let Some(sm) = &hook.state_machine {
            for state in &sm.states {
                for (at, actions) in [("on_enter", &state.on_enter), ("on_exit", &state.on_exit)] {
                    if !actions.is_empty() {
                        out.push(format!(
                            "Hook '{}' declares state '{}' {at} with {} action(s); {FIX}",
                            hook.name,
                            state.name,
                            actions.len()
                        ));
                    }
                }
            }
        }
    }
    out
}

fn trigger_key(event: &crate::ast::hook::TriggerEvent) -> String {
    use crate::ast::hook::TriggerEvent::*;
    match event {
        BeforeCreate => "before_create".into(),
        AfterCreate => "after_create".into(),
        BeforeUpdate => "before_update".into(),
        AfterUpdate => "after_update".into(),
        BeforeDelete => "before_delete".into(),
        AfterDelete => "after_delete".into(),
        OnTransition(t) => format!("on_transition {t}"),
        OnEnterState(s) => format!("on_enter {s}"),
        OnExitState(s) => format!("on_exit {s}"),
    }
}

/// Fields named like a secret (a password, token, PIN or key, or a digest of one) that
/// carry none of `@sensitive`, `@hashed` or `@not_secret`, so every reader of the model
/// is served them. Advisory: `@not_secret` acknowledges a field that is meant to be read,
/// such as a public URL key.
pub fn secret_warnings(schema: &ModuleSchema) -> Vec<String> {
    const SECRET: &[&str] = &[
        "password", "passwd", "secret", "token", "api_key", "apikey", "private_key", "otp",
        "pin_hash", "pin_code", "salt", "credential", "_hash", "recovery_code", "backup_code",
        "signing_key", "access_key",
    ];
    // Facts about a secret, not the secret: when it expires, how it is used, its policy.
    const ABOUT: &[&str] = &[
        "_id", "_ids", "_at", "_count", "_length", "_type", "_expires", "expires_", "_enabled",
        "_required", "is_", "has_", "_policy", "_hint", "_url", "_format", "_version",
        "_algorithm", "_method", "_status", "_prefix", "_last4", "_ttl", "_days", "_limit",
        "_scope", "_name", "_ref", "_generated", "_strength", "_metadata", "_requirements",
        "_history", "require_",
    ];
    let mut out = Vec::new();
    for model in &schema.models {
        for f in &model.fields {
            let name = f.name.to_lowercase();
            let looks_secret = SECRET.iter().any(|w| name.contains(w)) || name == "pin";
            let about = ABOUT.iter().any(|w| {
                if let Some(rest) = w.strip_prefix('_') {
                    name.ends_with(&format!("_{rest}")) || (w.ends_with('_') && name.contains(*w))
                } else {
                    name.starts_with(*w)
                }
            });
            let marked = ["sensitive", "hashed", "not_secret"].iter().any(|a| f.has_attribute(a));
            if looks_secret && !about && !marked {
                out.push(format!(
                    "Field '{}.{}' looks like a secret and is served to every reader of the model: mark it @sensitive (never served), or @not_secret if it is meant to be read",
                    model.name, f.name
                ));
            }
        }
    }
    out
}

/// Names the generator has to change because two models would otherwise emit the
/// same type: a model called `<Model>Filter` beside `<Model>` makes the `<Model>`
/// repository filter come out as `<Model>RepositoryFilter`. Advisory: printed by
/// `schema validate`, never a gate.
pub fn naming_warnings(schema: &ModuleSchema) -> Vec<String> {
    let mut out = Vec::new();
    for model in &schema.models {
        let filter = format!("{}Filter", model.name);
        if schema.models.iter().any(|m| m.name == filter) {
            out.push(format!(
                "Model '{filter}' is named like the repository filter of model '{}'; that filter is generated as '{}RepositoryFilter'",
                model.name, model.name
            ));
        }
    }
    out
}

#[cfg(test)]
mod naming_warning_tests {
    use super::*;
    use crate::ast::Model;

    #[test]
    fn a_model_named_like_another_models_filter_is_reported() {
        let mut s = ModuleSchema::new("test");
        s.models = vec![Model::new("Mailing"), Model::new("MailingFilter")];
        let w = naming_warnings(&s);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("MailingRepositoryFilter"));
        s.models = vec![Model::new("Mailing"), Model::new("MailingTrace")];
        assert!(naming_warnings(&s).is_empty());
    }
}

#[cfg(test)]
mod secret_rule_tests {
    use super::*;
    use crate::ast::{Attribute, Field, Model};

    fn model(attrs: &[&str], fields: Vec<Field>) -> ModuleSchema {
        let mut m = Model::new("Session");
        m.attributes = attrs.iter().map(|a| Attribute::new(*a)).collect();
        m.fields = fields;
        let mut s = ModuleSchema::new("test");
        s.models = vec![m];
        s
    }
    fn field(name: &str, attrs: &[&str]) -> Field {
        let mut f = Field::new(name, crate::ast::TypeRef::Primitive(crate::ast::PrimitiveType::String));
        f.attributes = attrs.iter().map(|a| Attribute::new(*a)).collect();
        f
    }

    #[test]
    fn an_unmarked_secret_is_reported_and_a_marked_or_acknowledged_one_is_not() {
        let s = model(&[], vec![
            field("token_hash", &[]),
            field("password_hash", &["hashed"]),
            field("session_token", &["sensitive"]),
            field("access_token", &["not_secret"]),
            field("token_expires_at", &[]),
            field("password_policy", &[]),
            field("name", &[]),
        ]);
        let w = secret_warnings(&s);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("'Session.token_hash'"), "{w:?}");
    }

    #[test]
    fn an_audited_model_with_a_secret_fails_validation() {
        let s = model(&["audited"], vec![field("token_hash", &["sensitive"])]);
        let errors: Vec<String> = SchemaValidator::new(&s)
            .validate()
            .err()
            .unwrap_or_default()
            .iter()
            .map(|e| e.to_string())
            .filter(|e| e.contains("@audited"))
            .collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("token_hash"));
        let plain = model(&[], vec![field("token_hash", &["sensitive"])]);
        assert!(SchemaValidator::new(&plain).validate().err().unwrap_or_default().iter().all(|e| !e.to_string().contains("@audited")));
    }
}

#[cfg(test)]
mod trigger_rule_tests {
    use super::*;
    use crate::ast::hook::{Action, Hook, State, StateMachine, Trigger, TriggerEvent};
    use crate::ast::{Field, Model, PrimitiveType, TypeRef};

    fn act(text: &str) -> Action {
        crate::parser::yaml_parser::YamlAction::Simple(text.to_string()).into_action()
    }

    fn schema_with(triggers: Vec<Trigger>, states: Vec<State>) -> ModuleSchema {
        let mut s = ModuleSchema::new("test");
        let mut hook = Hook::new("Order", "Order");
        hook.triggers = triggers;
        if !states.is_empty() {
            hook.state_machine = Some(StateMachine { states, ..Default::default() });
        }
        s.hooks = vec![hook, Hook::new("Quiet", "Quiet")];
        s
    }

    fn legacy(mut s: ModuleSchema) -> ModuleSchema {
        s.generators_config = Some(crate::parser::yaml_parser::GeneratorsConfig { legacy_triggers: Some(true), ..Default::default() });
        s
    }

    #[test]
    fn a_module_on_legacy_triggers_gets_a_warning_per_trigger_and_state_action_and_no_error() {
        let s = legacy(schema_with(
            vec![
                Trigger { event: TriggerEvent::BeforeCreate, actions: vec![act("set: placed_at = now()")], ..Default::default() },
                Trigger { event: TriggerEvent::AfterCreate, actions: vec![act("emit: OrderPlaced(order_id: id)"), act("log(created)")], ..Default::default() },
            ],
            vec![State { name: "paid".into(), on_enter: vec![act("set: paid_at = now()")], on_exit: vec![], ..Default::default() }],
        ));
        let w = trigger_warnings(&s);
        assert_eq!(w.len(), 3, "one per trigger and per state hook, quiet hooks pass: {w:?}");
        assert!(w[0].contains("trigger 'before_create' with 1 action(s)") && w[0].contains("not generated"), "{w:?}");
        assert!(w[1].contains("trigger 'after_create' with 2 action(s)"), "{w:?}");
        assert!(w[2].contains("state 'paid' on_enter"), "{w:?}");
        assert!(trigger_problems(&s).is_empty(), "legacy modules get warnings, never errors");
        assert!(validate_trigger_errors(&s).is_empty(), "and still validate");
    }

    #[test]
    fn a_trigger_names_the_key_the_hook_file_wrote_for_a_legacy_module() {
        let yaml: crate::parser::yaml_parser::YamlTrigger = serde_yaml::from_str("actions: [\"emit: StatusChanged(id)\"]").unwrap();
        let t = yaml.into_trigger("after_update_status");
        let w = trigger_warnings(&legacy(schema_with(vec![t], vec![])));
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("'after_update_status'"), "{w:?}");
    }

    fn generating(hook: &str, extensions: bool) -> ModuleSchema {
        let mut s = ModuleSchema::new("test");
        let mut order = Model::new("Order");
        for name in ["id", "status"] {
            order.fields.push(Field::new(name, TypeRef::Primitive(PrimitiveType::String)));
        }
        s.models.push(order);
        s.rule_register.read_file("order.hook.yaml", hook);
        let models = s.models.clone();
        s.rule_register.bind(&models);
        s.rule_register.check(&models);
        s.rule_register.mark_compiled(&models, &Default::default());
        if extensions {
            s.generators_config = Some(crate::parser::yaml_parser::GeneratorsConfig { extensions: Some(true), ..Default::default() });
        }
        s
    }

    #[test]
    fn a_module_that_generates_triggers_fails_only_on_what_cannot_be_honoured() {
        let ok = "name: Order\nmodel: Order\ntriggers:\n  after_create:\n    actions: [\"log(made)\"]\n";
        assert!(trigger_problems(&generating(ok, false)).is_empty());
        let bad = "name: Order\nmodel: Order\ntriggers:\n  after_update_nope:\n    actions: [\"log(made)\"]\n  after_create:\n    if: \"x IN (1)\"\n    actions: [\"log(made)\"]\n";
        let p = trigger_problems(&generating(bad, true));
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].starts_with("order.hook.yaml") && p[0].contains("no field that `nope` spells"), "{p:?}");
        assert!(p[1].contains("an `if:` the generator cannot compile"), "{p:?}");
        assert_eq!(validate_trigger_errors(&generating(bad, true)).len(), 2, "validation fails on each");
    }

    #[test]
    fn hand_methods_need_the_extension_layout_and_say_where_the_implementation_lives() {
        let hand = "name: Order\nmodel: Order\ntriggers:\n  after_create:\n    actions: [\"trigger: refresh\", \"trigger: again\"]\n";
        let p = trigger_problems(&generating(hand, false));
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("`after_create__refresh` and 1 more") && p[0].contains("generators: extensions: true") && p[0].contains("OrderTriggers"), "{p:?}");
        assert!(trigger_problems(&generating(hand, true)).is_empty());
    }

    #[test]
    fn a_moment_that_depends_on_hand_code_is_a_warning() {
        let hook = "name: Order\nmodel: Order\nstates:\n  field: status\n  verbs: hand\n  values:\n    open:\n      initial: true\n      on_enter: [\"log(opened)\"]\n    done: {}\n  transitions:\n    finish: { from: open, to: done }\ntriggers:\n  after_finish:\n    actions: [\"log(done)\"]\n  after_login:\n    actions: [\"log(in)\"]\n";
        let s = generating(hook, true);
        assert!(trigger_problems(&s).is_empty(), "{:?}", trigger_problems(&s));
        let w = trigger_warnings(&s).join("\n");
        for needle in ["never runs on create", "`verbs: hand`", "fire_after_login"] {
            assert!(w.contains(needle), "{needle} not in:\n{w}");
        }
    }

    /// The validation errors the trigger rule raised; the bare test hook trips
    /// unrelated checks, which are left out.
    fn validate_trigger_errors(s: &ModuleSchema) -> Vec<String> {
        SchemaValidator::new(s)
            .validate()
            .err()
            .unwrap_or_default()
            .iter()
            .map(|e| e.to_string())
            .filter(|m| m.contains("order.hook.yaml:") || (m.contains("Hook 'Order'") && m.contains(" declares ")))
            .collect()
    }
}

#[cfg(test)]
mod workflow_warning_tests {
    use super::*;

    #[test]
    fn a_declared_workflow_reads_as_documentation_and_the_dead_flag_is_named() {
        let mut s = ModuleSchema::new("test");
        s.workflows = vec![crate::ast::Workflow { name: "ProcureToPay".into(), ..Default::default() }];
        let w = workflow_warnings(&s);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("'ProcureToPay'") && w[0].contains("documentation"), "{w:?}");
        s.workflows.clear();
        assert!(workflow_warnings(&s).is_empty());
        s.generators_config = Some(crate::parser::yaml_parser::GeneratorsConfig { workflows: Some(true), ..Default::default() });
        let w = workflow_warnings(&s);
        assert!(w.len() == 1 && w[0].contains("remove the flag"), "{w:?}");
    }

    #[test]
    fn an_opt_in_that_names_a_retired_target_is_dead_configuration() {
        let mut s = ModuleSchema::new("test");
        s.generators_config = Some(crate::parser::yaml_parser::GeneratorsConfig {
            opt_in: Some(vec!["seeder".into(), "trigger".into(), "versioning".into()]),
            enabled: Some(vec!["flows".into()]),
            ..Default::default()
        });
        let w = retired_generator_warnings(&s);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("opt_in") && w[0].contains("'trigger'") && w[1].contains("enabled") && w[1].contains("'flows'"), "{w:?}");
    }
}
