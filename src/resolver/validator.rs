//! Schema validator
//!
//! Validates schema completeness and correctness.

use super::ResolveError;
use crate::ast::{CompanyFence, Enforcement, ModuleSchema};
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

        // Validate workflows (business processes)
        for workflow in &self.schema.workflows {
            errors.extend(self.validate_workflow(workflow));
        }

        // Validate the module-level company fence (ADR-0014)
        errors.extend(self.validate_company_fence());

        // Validate field-level lifecycle declarations (ADR-0016)
        errors.extend(self.validate_lifecycles());

        // Validate scheduled-job postures (ADR-0020)
        errors.extend(self.validate_scheduled_jobs());

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
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
        let machines: Vec<(String, String)> = self
            .schema
            .hooks
            .iter()
            .filter_map(|h| {
                h.state_machine
                    .as_ref()
                    .map(|m| (h.name.clone(), m.field.clone()))
            })
            .collect();

        for model in &self.schema.models {
            for field in &model.fields {
                let Some(lifecycle) = &field.lifecycle else { continue };
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
                        match machines.iter().find(|(hook, _)| hook == machine) {
                            None => errors.push(ResolveError::validation(format!(
                                "{declared}: state_machine '{machine}' does not exist — \
                                 no hook in this module declares it"
                            ))),
                            Some((_, guarded_field)) if guarded_field != &field.name => {
                                errors.push(ResolveError::validation(format!(
                                    "{declared}: state_machine '{machine}' guards field \
                                     '{guarded_field}', not '{}' — a hand_set field must name \
                                     the machine that guards it",
                                    field.name
                                )));
                            }
                            Some(_) => {}
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

        // Check for primary key
        let has_id = model.fields.iter().any(|f| f.has_attribute("id"));
        if !has_id {
            errors.push(ResolveError::validation(format!(
                "Model '{}' has no primary key field (missing @id attribute)",
                model.name
            )));
        }

        // Collect known model names for relation validation
        let known_models: HashSet<_> = self.schema.models.iter()
            .map(|m| m.name.as_str())
            .collect();

        let known_types: HashSet<_> = self.schema.enums.iter()
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
            if field.name.ends_with("_id") && !field.has_attribute("foreign_key") && !skip_fk_check {
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
                if let Some(target) = fk.args.first().and_then(|(_, v)| crate::resolver::cross_module_fk::fk_target(v)) {
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
    fn validate_indexes(&self, model: &crate::ast::Model, field_names: &HashSet<&String>) -> Vec<ResolveError> {
        let mut errors = Vec::new();

        // Known sub-keys of audit_metadata JSONB fields
        const AUDIT_METADATA_KEYS: &[&str] = &[
            "created_at", "updated_at", "deleted_at",
            "created_by", "updated_by", "deleted_by",
        ];

        // Collect JSONB field info for sub-key resolution
        let has_audit_metadata = model.fields.iter().any(|f| f.has_attribute("audit_metadata"));

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
                    model.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
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
        let has_default = enum_def.variants.iter()
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
            if rule.message.is_empty() {
                errors.push(ResolveError::validation(format!(
                    "Rule '{}' in hook '{}' has no message",
                    rule.name, hook.name
                )));
            }
            // ADR-0015: a service-only invariant is reachable from raw-SQL write
            // paths with no DB backstop — the justification is what makes that
            // risk a reviewed decision instead of an accident.
            if rule.enforcement == Enforcement::Service
                && rule.justification.as_deref().unwrap_or("").trim().is_empty()
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

        // Check for at least one final state
        let final_count = sm.states.iter().filter(|s| s.final_state).count();
        if final_count == 0 {
            errors.push(ResolveError::StateMachineError {
                message: format!(
                    "State machine in workflow '{}' has no final state (use @final)",
                    workflow_name
                ),
            });
        }

        // Check transition states exist
        let state_names: HashSet<_> = sm.states.iter().map(|s| s.name.as_str()).collect();

        for transition in &sm.transitions {
            // Check source states
            for from in &transition.from {
                if from != "*" && !state_names.contains(from.as_str()) {
                    errors.push(ResolveError::StateMachineError {
                        message: format!(
                            "Transition '{}' in workflow '{}' references unknown source state '{}'",
                            transition.name, workflow_name, from
                        ),
                    });
                }
            }

            // Check target state
            if !state_names.contains(transition.to.as_str()) {
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
    warnings
}

/// Fence warnings for [`declaration_warnings`] — see ADR-0014.
fn fence_warnings(schema: &ModuleSchema) -> Vec<String> {
    let Some(fence) = schema.company_fence else {
        // ADR-0014 sweep: every module declares an explicit posture. Undeclared is the
        // warning generate shows (never a gate here — legacy modules must still regen);
        // `validate` / `validate-workspace` make the same condition a hard failure.
        return vec![
            "no 'company_fence:' declaration in index.model.yaml — ADR-0014 requires an \
             explicit posture per module (strict | shared_blank | shared_tree | none); \
             'metaphor schema validate-workspace' lists every undeclared module"
                .to_string(),
        ];
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
        f.attributes.push(
            Attribute::new("foreign_key").with_arg(AttributeValue::String(target.into())),
        );
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
            errs.iter().any(|e| e.contains("no model 'Organization' exists")),
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
        child.fields = vec![id_field(), fk_field("organization_id", "corpus.Organization.id")];
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
        for fence in [None, Some(CompanyFence::Strict), Some(CompanyFence::SharedBlank), Some(CompanyFence::SharedTree)] {
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
        required.fields[1].type_ref = TypeRef::optional(
            TypeRef::Primitive(PrimitiveType::Uuid),
        );
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
            !declaration_warnings(&s).iter().any(|w| w.contains("NOT NULL")),
            "nullable company_id must not warn"
        );
    }

    #[test]
    fn undeclared_module_warns_on_generate() {
        // ADR-0014: `generate` stays warning-only (unswept legacy modules must still
        // regen), but the missing declaration is no longer SILENT — the warning points
        // at `validate-workspace`, where the same gap is a hard failure.
        let s = schema_with_fence(vec![company_model(false)], None);
        let warnings = declaration_warnings(&s);
        assert!(
            warnings.len() == 1 && warnings[0].contains("company_fence"),
            "expected exactly the missing-declaration warning, got: {warnings:?}"
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
        let s = schema_with_hook(rule_with(Enforcement::Service, Some("cross-model check; needs both rows in memory")));
        assert!(errors_of(&s).is_empty(), "justified service rule must validate");
        let s = schema_with_hook(rule_with(Enforcement::Db, None));
        assert!(errors_of(&s).is_empty(), "db (default) needs no justification");
        let s = schema_with_hook(rule_with(Enforcement::Both, None));
        assert!(errors_of(&s).is_empty(), "both needs no justification — the DB backstop exists");
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
            Lifecycle { shape: LifecycleShape::Split, ..Default::default() },
        ));
        let errs = errors_of(&schema_with(m));
        assert!(
            errs.iter().any(|e| e.contains("split requires a 'driver:'")),
            "split must demand a driver, got: {errs:?}"
        );
    }

    #[test]
    fn split_with_unknown_driver_is_fatal_but_known_driver_passes() {
        let mut m = stage_model();
        m.fields.push(Field::new("stage", TypeRef::Primitive(PrimitiveType::String)));
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
            errs.iter().any(|e| e.contains("driver 'ghost' is not a field")),
            "driver must resolve to a same-model field, got: {errs:?}"
        );

        let mut m = stage_model();
        m.fields.push(Field::new("stage", TypeRef::Primitive(PrimitiveType::String)));
        m.fields.push(lifecycle_field(
            "sub_state",
            Lifecycle {
                shape: LifecycleShape::Split,
                driver: Some("stage".to_string()),
                ..Default::default()
            },
        ));
        assert!(errors_of(&schema_with(m)).is_empty(), "split with a real driver must validate");
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
            errs.iter().any(|e| e.contains("cannot drive its own lifecycle")),
            "self-driving driver must be rejected, got: {errs:?}"
        );
    }

    #[test]
    fn stage_ref_needs_a_relation() {
        let mut m = stage_model();
        m.fields.push(lifecycle_field(
            "stage_id",
            Lifecycle { shape: LifecycleShape::StageRef, ..Default::default() },
        ));
        let errs = errors_of(&schema_with(m));
        assert!(
            errs.iter().any(|e| e.contains("stage_ref requires a relation")),
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
            Lifecycle { shape: LifecycleShape::StageRef, ..Default::default() },
        );
        f.attributes.push(Attribute::new("exclude_from_foreign_key_check"));
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
        let mut sm = StateMachine { field: field.to_string(), ..Default::default() };
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
            errors_of(&s).iter().any(|e| e.contains("'ghost_machine' does not exist")),
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
        assert_eq!(LifecycleShape::from_name("hand_set"), Some(LifecycleShape::HandSet));
        assert_eq!(LifecycleShape::from_name("stage_ref"), Some(LifecycleShape::StageRef));
        assert_eq!(LifecycleShape::from_name("nope"), None);
        for name in [
            "projection", "hand_set", "hybrid", "split", "stage_ref", "window", "virtual",
            "label", "inert", "none",
        ] {
            let shape = LifecycleShape::from_name(name)
                .unwrap_or_else(|| panic!("'{name}' must parse"));
            assert_eq!(shape.shape_name(), name, "shape_name must round-trip");
        }
    }
}

#[cfg(test)]
mod scheduled_job_tests {
    use super::*;
    use crate::ast::{CommitPolicy, JobPosture, ModuleSchema, ScheduledJob};

    fn job_with(posture: Option<JobPosture>, triggers: Vec<String>, pickup_lock: bool) -> ScheduledJob {
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
            warns.iter().any(|w| w.contains("'nightly_gc' declares no posture")),
            "absent posture must warn, got: {warns:?}"
        );
        // Legacy jobs (all 49 backbone indexes today) never fail validation.
        assert!(errors_of(vec![job]).is_empty(), "absent posture must not error");
    }

    #[test]
    fn queue_draining_postures_demand_pickup_lock() {
        for posture in [
            JobPosture::Pull,
            JobPosture::SelfArming,
            JobPosture::HostRiding,
            JobPosture::AutovacuumRide,
        ] {
            let errs = errors_of(vec![job_with(Some(posture), vec!["mail.created".to_string()], false)]);
            assert!(
                errs.iter()
                    .any(|e| e.contains("without pickup_lock: true") && e.contains("MMB-4")),
                "{posture:?} without pickup_lock must be a hard error, got: {errs:?}"
            );

            let ok = errors_of(vec![job_with(Some(posture), vec!["mail.created".to_string()], true)]);
            assert!(ok.is_empty(), "{posture:?} with pickup_lock must validate, got: {ok:?}");
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
            errs.iter().any(|e| e.contains("self_arming with no triggers")),
            "self_arming must name its re-arming events, got: {errs:?}"
        );

        let ok = errors_of(vec![job_with(
            Some(JobPosture::SelfArming),
            vec!["registration.confirmed".to_string()],
            true,
        )]);
        assert!(ok.is_empty(), "self_arming with triggers and a lock must validate, got: {ok:?}");
    }

    #[test]
    fn posture_names_match_the_declared_vocabulary() {
        assert_eq!(job_posture_name(JobPosture::SelfArming), "self_arming");
        assert_eq!(job_posture_name(JobPosture::InactiveThenIcp), "inactive_then_icp");
    }
}
