//! Cross-module `@foreign_key` target validation.
//!
//! The single-module [`SchemaValidator`](super::validator::SchemaValidator) can only see one module,
//! so it checks intra-module FK targets (`@foreign_key(Entity.id)`) and deliberately leaves
//! cross-module targets (`@foreign_key(module.Entity.id)`) alone — it has no way to know another
//! module's entities. That gap is how a phantom survived: five models pointed
//! `@foreign_key(corpus.Organization.id)` at an entity that never existed in any module, and every
//! per-module validation passed.
//!
//! This pass closes it. Given every module's entity set (a workspace-wide registry), it resolves each
//! cross-module FK to (a) a module that exists and (b) an entity that exists in it — and reports the
//! ones that dangle.

use std::collections::{HashMap, HashSet};

use crate::ast::{AttributeValue, ModuleSchema};

/// Read a `@foreign_key(...)` target string regardless of how the parser classified it.
///
/// An unquoted dotted target like `sapiens.User.id` parses as [`AttributeValue::Ident`], while a
/// quoted one parses as [`AttributeValue::String`]. Both are the same reference; matching only one
/// silently ignores the other — the defect that made the first FK-target check a no-op on real
/// schemas (every real `@foreign_key` is written unquoted).
pub(crate) fn fk_target(value: &AttributeValue) -> Option<&str> {
    match value {
        AttributeValue::String(s) | AttributeValue::Ident(s) => Some(s.as_str()),
        _ => None,
    }
}

/// One cross-module foreign-key reference, located for a useful error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossModuleFkRef {
    /// The module the reference lives in.
    pub from_module: String,
    /// The model the field belongs to.
    pub from_model: String,
    /// The `_id` field carrying the reference.
    pub from_field: String,
    /// The target module named in `@foreign_key(<module>.Entity.id)`.
    pub target_module: String,
    /// The target entity named in `@foreign_key(module.<Entity>.id)`.
    pub target_entity: String,
}

/// A workspace registry: module name → the set of entity (model) names it declares.
pub type EntityRegistry = HashMap<String, HashSet<String>>;

/// Collect the cross-module (`module.Entity.id`) FK references declared in one module's schema.
///
/// Intra-module refs (`Entity.id`, two parts) are skipped — the single-module validator owns those.
pub fn collect_cross_module_fk_refs(module_name: &str, schema: &ModuleSchema) -> Vec<CrossModuleFkRef> {
    let mut refs = Vec::new();
    for model in &schema.models {
        for field in &model.fields {
            let Some(fk) = field.attributes.iter().find(|a| a.name == "foreign_key") else {
                continue;
            };
            let Some(target) = fk.args.first().and_then(|(_, v)| fk_target(v)) else {
                continue;
            };
            // `module.Entity.column` = cross-module (3 parts). `Entity.column` (2) is intra-module.
            let parts: Vec<&str> = target.split('.').collect();
            if parts.len() == 3 {
                refs.push(CrossModuleFkRef {
                    from_module: module_name.to_string(),
                    from_model: model.name.clone(),
                    from_field: field.name.clone(),
                    target_module: parts[0].to_string(),
                    target_entity: parts[1].to_string(),
                });
            }
        }
    }
    refs
}

/// Validate cross-module FK references against the workspace entity registry.
///
/// Returns one error string per reference whose target module is unknown or whose target entity does
/// not exist in that module. An empty result means every cross-module FK resolves.
pub fn validate_cross_module_fks(registry: &EntityRegistry, refs: &[CrossModuleFkRef]) -> Vec<String> {
    let mut errors = Vec::new();
    for r in refs {
        match registry.get(&r.target_module) {
            None => errors.push(format!(
                "{}.{} field '{}' has @foreign_key({}.{}...) but no module '{}' exists in the workspace",
                r.from_module, r.from_model, r.from_field, r.target_module, r.target_entity, r.target_module
            )),
            Some(entities) if !entities.contains(&r.target_entity) => errors.push(format!(
                "{}.{} field '{}' has @foreign_key({}.{}...) but module '{}' has no entity '{}' \
                 (phantom cross-module reference)",
                r.from_module, r.from_model, r.from_field, r.target_module, r.target_entity,
                r.target_module, r.target_entity
            )),
            Some(_) => {}
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(pairs: &[(&str, &[&str])]) -> EntityRegistry {
        pairs
            .iter()
            .map(|(m, ents)| (m.to_string(), ents.iter().map(|e| e.to_string()).collect()))
            .collect()
    }

    fn fk(from_module: &str, target_module: &str, target_entity: &str) -> CrossModuleFkRef {
        CrossModuleFkRef {
            from_module: from_module.into(),
            from_model: "SomeModel".into(),
            from_field: "some_id".into(),
            target_module: target_module.into(),
            target_entity: target_entity.into(),
        }
    }

    #[test]
    fn the_phantom_is_caught() {
        // corpus exists (as a knowledge base) but has no Organization — the exact real bug.
        let reg = registry(&[
            ("corpus", &["Article", "ArticleCategory", "ArticleFeedback"]),
            ("sapiens", &["User", "Role", "OrganizationUser"]),
        ]);
        let refs = vec![fk("sapiens", "corpus", "Organization")];
        let errs = validate_cross_module_fks(&reg, &refs);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("has no entity 'Organization'"), "got: {}", errs[0]);
        assert!(errs[0].contains("phantom"), "should name it a phantom: {}", errs[0]);
    }

    #[test]
    fn unknown_target_module_is_caught() {
        let reg = registry(&[("sapiens", &["User"])]);
        let refs = vec![fk("sapiens", "ghostmod", "Thing")];
        let errs = validate_cross_module_fks(&reg, &refs);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("no module 'ghostmod'"), "got: {}", errs[0]);
    }

    #[test]
    fn a_valid_cross_module_ref_passes() {
        let reg = registry(&[
            ("organization", &["Company", "Branch"]),
            ("selling", &["SalesInvoice"]),
        ]);
        // selling.SalesInvoice → organization.Company is legitimate.
        let refs = vec![fk("selling", "organization", "Company")];
        assert!(validate_cross_module_fks(&reg, &refs).is_empty());
    }

    #[test]
    fn multiple_dangling_refs_all_reported() {
        let reg = registry(&[("corpus", &["Article"]), ("sapiens", &["User"])]);
        let refs = vec![
            fk("sapiens", "corpus", "Organization"),
            fk("sapiens", "corpus", "Organization"),
            fk("sapiens", "corpus", "Organization"),
        ];
        assert_eq!(validate_cross_module_fks(&reg, &refs).len(), 3);
    }
}
