//! Permissions declared in the schema: the vocabulary, what each route requires, and the seed.
//!
//! The schema declares the permission *vocabulary* and what each route, transition and verb
//! requires. Who holds which permission is data in the identity module's tables, resolved at run
//! time by the framework (`backbone-auth`'s `GrantSource`); nothing here knows a person.
//!
//! - **Canonical name.** `<resource>:<action>`, the resource an entity in snake_case. A route's
//!   permission is `<entity>:<action>` by default (`read`, `create`, `update`, `delete`, `restore`,
//!   `purge`), a transition or hand verb's `<entity>:<verb>`; `requires:` on a model or a
//!   transition says otherwise.
//! - **Declared vocabulary.** The module-level `authorization:` block's `permissions:` lists the
//!   names that exist beyond what the routes already imply (custom verbs, coarser grants) and its
//!   `roles:` the system roles with their baseline grants. A plural collection name
//!   (`buckets`) maps to its entity (`bucket`). Every permission the generic routes and the
//!   mounted verbs require is part of the vocabulary implicitly, so a module that declares nothing
//!   is complete.
//! - **Legacy spellings.** The older `resource.action` form is accepted and normalised; each use is
//!   reported by `schema doctor` so the schema moves to the canonical form.
//!
//! Everything that needs the answer (the handler generator, the seed migration, `schema rules`,
//! `schema doctor`, the web client's constants) asks [`Rbac`], so they cannot disagree.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Model, ModuleSchema};
use crate::utils::{pluralize, to_snake_case};

/// The permission that matches everything.
pub const GOD_PERMISSION: &str = "*:*";

/// How a generic action of an entity is named in a permission, and in `requires:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Gate {
    Read,
    Create,
    Update,
    Delete,
    Restore,
    Purge,
}

impl Gate {
    /// Every gate, in the order a report lists them.
    pub const ALL: [Gate; 6] = [
        Gate::Read,
        Gate::Create,
        Gate::Update,
        Gate::Delete,
        Gate::Restore,
        Gate::Purge,
    ];

    /// The action half of the default permission, and the key in `requires:`.
    pub fn action(&self) -> &'static str {
        match self {
            Gate::Read => "read",
            Gate::Create => "create",
            Gate::Update => "update",
            Gate::Delete => "delete",
            Gate::Restore => "restore",
            Gate::Purge => "purge",
        }
    }

    /// The gate a `requires:` key names.
    pub fn from_key(key: &str) -> Option<Gate> {
        Gate::ALL.into_iter().find(|g| g.action() == key)
    }

    /// The routes of an entity this gate guards, as a report names them.
    pub fn routes(&self) -> &'static [&'static str] {
        match self {
            Gate::Read => &[
                "GET /",
                "GET /:id",
                "GET /count",
                "GET /aggregate",
                "GET /:id/history",
                "GET /trash",
                "GET /trash/count",
                "GET /:id/deleted",
            ],
            Gate::Create => &["POST /", "POST /bulk", "POST /upsert (with update)"],
            Gate::Update => &[
                "PUT /:id",
                "PATCH /:id",
                "PUT /bulk",
                "PATCH /bulk",
                "POST /bulk/preview",
                "POST /upsert (with create)",
            ],
            Gate::Delete => &["DELETE /:id", "POST /delete/bulk"],
            Gate::Restore => &[
                "POST /:id/restore",
                "POST /restore/bulk",
                "POST /restore/all",
            ],
            Gate::Purge => &["DELETE /trash/:id", "DELETE /trash/bulk", "DELETE /empty"],
        }
    }

    /// Whether the gate guards a write.
    pub fn is_write(&self) -> bool {
        !matches!(self, Gate::Read)
    }
}

/// How a permission was spelled in the schema, when it is not the canonical `<resource>:<action>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spelling {
    Canonical,
    /// `resource.action`
    Dotted,
    /// `<entity>:transition:<verb>`
    Transition,
}

/// A permission read from the schema: canonical, and how it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canon {
    pub permission: String,
    pub spelling: Spelling,
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_half(s: &str) -> bool {
    s == "*" || is_name(s)
}

/// Split a permission as written in a schema into `(resource, action, spelling)`, unmapped.
///
/// `resource:action` is canonical; `resource.action` is the legacy dotted form;
/// `resource:transition:verb` the legacy transition form; a bare `*` is `*:*`.
pub fn split_permission(raw: &str) -> Option<(String, String, Spelling)> {
    let raw = raw.trim();
    if raw == "*" {
        return Some(("*".into(), "*".into(), Spelling::Canonical));
    }
    if !raw.contains(':') {
        let (resource, action) = raw.split_once('.')?;
        return (is_half(resource) && is_half(action))
            .then(|| (resource.into(), action.into(), Spelling::Dotted));
    }
    let parts: Vec<&str> = raw.split(':').collect();
    match parts.as_slice() {
        [resource, "transition", verb] if is_half(resource) && is_half(verb) => {
            Some(((*resource).into(), (*verb).into(), Spelling::Transition))
        }
        [resource, action] if is_half(resource) && is_half(action) => {
            Some(((*resource).into(), (*action).into(), Spelling::Canonical))
        }
        _ => None,
    }
}

/// Where a permission is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    /// A generic CRUD route of an entity.
    Generic(Gate),
    /// A generated state-machine transition route.
    Transition,
    /// A hand-written verb of a machine that declares `verbs: hand`.
    HandVerb,
}

/// One permission a module's routes require.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Required {
    /// The model (`Ticket`).
    pub model: String,
    /// The resource half (`ticket`).
    pub entity: String,
    pub kind: RouteKind,
    /// What it guards, for a report (`resolve`, or a gate's routes).
    pub what: String,
    /// The canonical permission.
    pub permission: String,
    /// Whether a `requires:` renamed it from the default.
    pub renamed: bool,
}

/// A system role the schema declares, with its baseline grants (canonical, inheritance folded in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemRole {
    pub name: String,
    pub description: Option<String>,
    pub permissions: Vec<String>,
}

/// A permission the seed writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedPermission {
    pub name: String,
    pub resource: String,
    pub action: String,
    pub description: String,
}

/// What the seed migration writes: permissions, system roles and their baseline grants.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Seed {
    pub permissions: Vec<SeedPermission>,
    pub roles: Vec<SystemRole>,
}

/// A problem in a module's permission declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub message: String,
    /// Always a gate: the module cannot be used as written.
    pub fatal: bool,
}

/// The permission declarations of one module.
pub struct Rbac<'a> {
    schema: &'a ModuleSchema,
}

impl<'a> Rbac<'a> {
    pub fn new(schema: &'a ModuleSchema) -> Self {
        Self { schema }
    }

    /// The module name in snake_case, the legacy module-wide spelling's resource.
    pub fn module(&self) -> String {
        to_snake_case(&self.schema.name)
    }

    /// The resource half of a model's permissions.
    pub fn entity(model: &Model) -> String {
        to_snake_case(&model.name)
    }

    /// Map a resource name as written (`buckets`, `Bucket`, `stored_files`) to the entity's.
    /// A name that is no entity's collection or plural is kept, singularised when it is a plain
    /// plural, so a resource declared for hand verbs reads as an entity would.
    pub fn resource(&self, raw: &str) -> String {
        let snake = to_snake_case(&raw.replace('-', "_"));
        if snake == "*" {
            return snake;
        }
        for model in &self.schema.models {
            let entity = Self::entity(model);
            if snake == entity || snake == model.collection_name() || snake == pluralize(&entity) {
                return entity;
            }
        }
        if let Some(stem) = snake.strip_suffix("ies") {
            return format!("{stem}y");
        }
        for tail in ["sses", "xes", "ches", "shes"] {
            if let Some(stem) = snake.strip_suffix(tail) {
                return format!("{stem}{}", &tail[..tail.len() - 2]);
            }
        }
        if snake.ends_with('s') && !snake.ends_with("ss") && snake.len() > 1 {
            return snake[..snake.len() - 1].to_string();
        }
        snake
    }

    /// A permission as written anywhere in the schema, canonical (`None` when malformed).
    pub fn canonical(&self, raw: &str) -> Option<Canon> {
        let (resource, action, spelling) = split_permission(raw)?;
        Some(Canon {
            permission: format!("{}:{}", self.resource(&resource), action),
            spelling,
        })
    }

    fn handler_models(&self) -> impl Iterator<Item = &'a Model> {
        self.schema.models.iter().filter(|m| {
            !crate::generators::model_skips_target(m, crate::generators::GenerationTarget::Handler)
        })
    }

    /// The permission `model`'s `gate` requires, and whether a `requires:` renamed it.
    pub fn gate_permission(&self, model: &Model, gate: Gate) -> (String, bool) {
        match model
            .requires
            .get(gate.action())
            .and_then(|raw| self.canonical(raw))
        {
            Some(c) => (c.permission, true),
            None => (format!("{}:{}", Self::entity(model), gate.action()), false),
        }
    }

    /// The permission a transition or hand verb of `model` requires.
    pub fn verb_permission(
        &self,
        model: &Model,
        verb: &str,
        requires: Option<&str>,
    ) -> (String, bool) {
        match requires.and_then(|raw| self.canonical(raw)) {
            Some(c) => (c.permission, true),
            None => (
                format!("{}:{}", Self::entity(model), to_snake_case(verb)),
                false,
            ),
        }
    }

    /// Every permission the module's mounted routes require: the generic routes of each model with
    /// a generated handler (reads always, writes when the model mounts them), the generated
    /// transition routes and the verbs of hand machines.
    pub fn required(&self) -> Vec<Required> {
        let mut out = Vec::new();
        for model in self.handler_models() {
            let entity = Self::entity(model);
            for gate in Gate::ALL {
                if gate.is_write() && !model.mounts_writes() {
                    continue;
                }
                let (permission, renamed) = self.gate_permission(model, gate);
                out.push(Required {
                    model: model.name.clone(),
                    entity: entity.clone(),
                    kind: RouteKind::Generic(gate),
                    what: gate.routes().join(", "),
                    permission,
                    renamed,
                });
            }
            for machine in self
                .schema
                .rule_register
                .machines
                .iter()
                .filter(|m| m.bound_model.as_deref() == Some(model.name.as_str()))
            {
                let kind = match machine.verbs.as_deref() {
                    Some("generated")
                        if crate::rules::compile::mounts_generated_verbs(self.schema, model) =>
                    {
                        RouteKind::Transition
                    }
                    Some("hand") => RouteKind::HandVerb,
                    _ => continue,
                };
                for t in &machine.transitions {
                    let Some(verb) = &t.verb else { continue };
                    let (permission, renamed) =
                        self.verb_permission(model, verb, t.requires.as_deref());
                    out.push(Required {
                        model: model.name.clone(),
                        entity: entity.clone(),
                        kind,
                        what: format!("transition {verb}"),
                        permission,
                        renamed,
                    });
                }
            }
        }
        out
    }

    /// The names the `authorization.permissions` block declares, canonical.
    pub fn declared(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        if let Some(auth) = &self.schema.authorization {
            for (resource, actions) in &auth.permissions {
                for action in actions {
                    out.insert(format!("{}:{}", self.resource(resource), action));
                }
            }
        }
        out
    }

    /// The whole vocabulary: what is declared, what the routes require, and what the system roles
    /// are granted.
    pub fn vocabulary(&self) -> BTreeSet<String> {
        let mut out = self.declared();
        out.extend(self.required().into_iter().map(|r| r.permission));
        for role in self.system_roles() {
            out.extend(role.permissions);
        }
        out
    }

    /// The system roles the `authorization.roles` block declares, grants canonical and inherited
    /// grants folded in. A grant that is malformed is left out (see [`Self::problems`]).
    pub fn system_roles(&self) -> Vec<SystemRole> {
        let Some(auth) = &self.schema.authorization else {
            return Vec::new();
        };
        let own = |name: &str| -> Vec<String> {
            auth.find_role(name)
                .map(|r| {
                    r.permissions
                        .iter()
                        .filter_map(|p| self.canonical(p).map(|c| c.permission))
                        .collect()
                })
                .unwrap_or_default()
        };
        auth.roles
            .iter()
            .map(|role| {
                let mut perms: Vec<String> = own(&role.name);
                let mut seen = BTreeSet::from([role.name.clone()]);
                let mut parent = role.inherits.clone();
                while let Some(p) = parent {
                    if !seen.insert(p.clone()) {
                        break;
                    }
                    perms.extend(own(&p));
                    parent = auth.find_role(&p).and_then(|r| r.inherits.clone());
                }
                perms.sort();
                perms.dedup();
                SystemRole {
                    name: role.name.clone(),
                    description: role.description.clone(),
                    permissions: perms,
                }
            })
            .collect()
    }

    /// What the seed migration writes.
    pub fn seed(&self) -> Seed {
        let roles = self.system_roles();
        let mut names: BTreeSet<String> = self.vocabulary();
        names.extend(roles.iter().flat_map(|r| r.permissions.iter().cloned()));
        let permissions = names
            .into_iter()
            .filter_map(|name| {
                let (resource, action) = name.split_once(':')?;
                let description = match (resource, action) {
                    ("*", "*") => "Everything".to_string(),
                    ("*", a) => format!("{} anything", capitalise(a)),
                    (r, "*") => format!("Everything on {}", r.replace('_', " ")),
                    (r, a) => format!(
                        "{} {}",
                        capitalise(&a.replace('_', " ")),
                        r.replace('_', " ")
                    ),
                };
                Some(SeedPermission {
                    name: name.clone(),
                    resource: resource.to_string(),
                    action: action.to_string(),
                    description,
                })
            })
            .collect();
        Seed { permissions, roles }
    }

    /// Every use of a legacy spelling in the schema, as `(where, written, canonical)`.
    pub fn legacy_spellings(&self) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        if let Some(auth) = &self.schema.authorization {
            for role in &auth.roles {
                for p in &role.permissions {
                    if let Some(c) = self.canonical(p) {
                        if c.spelling != Spelling::Canonical {
                            out.push((format!("role `{}`", role.name), p.clone(), c.permission));
                        }
                    }
                }
            }
        }
        for model in &self.schema.models {
            for (key, raw) in &model.requires {
                if let Some(c) = self.canonical(raw) {
                    if c.spelling != Spelling::Canonical {
                        out.push((
                            format!("`{}` requires.{key}", model.name),
                            raw.clone(),
                            c.permission,
                        ));
                    }
                }
            }
        }
        for machine in &self.schema.rule_register.machines {
            for t in &machine.transitions {
                if let Some(raw) = &t.requires {
                    if let Some(c) = self.canonical(raw) {
                        if c.spelling != Spelling::Canonical {
                            out.push((
                                format!(
                                    "transition `{}` of `{}`",
                                    t.verb.as_deref().unwrap_or("?"),
                                    machine.target
                                ),
                                raw.clone(),
                                c.permission,
                            ));
                        }
                    }
                }
            }
        }
        out
    }

    /// What is wrong with the module's declarations. A `fatal` problem is a gate.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let fatal = |message: String| Problem {
            message,
            fatal: true,
        };
        if let Some(auth) = &self.schema.authorization {
            for role in &auth.roles {
                for p in &role.permissions {
                    if self.canonical(p).is_none() {
                        out.push(fatal(format!("role `{}` lists `{p}`, which is no permission (write `<resource>:<action>`)", role.name)));
                    }
                }
                if let Some(parent) = &role.inherits {
                    if !auth.has_role(parent) {
                        out.push(fatal(format!(
                            "role `{}` inherits `{parent}`, which is not declared",
                            role.name
                        )));
                    }
                }
            }
            for (resource, actions) in &auth.permissions {
                if !is_name(&to_snake_case(&resource.replace('-', "_"))) {
                    out.push(fatal(format!(
                        "permissions: `{resource}` is not a resource name"
                    )));
                }
                for action in actions {
                    if !is_name(action) {
                        out.push(fatal(format!("permissions: `{resource}` lists the action `{action}`, which is not a name")));
                    }
                }
            }
        }
        let declared = self.declared();
        let implied: BTreeSet<String> = self
            .required()
            .into_iter()
            .filter(|r| !r.renamed)
            .map(|r| r.permission)
            .collect();
        for model in &self.schema.models {
            for (key, raw) in &model.requires {
                if Gate::from_key(key).is_none() {
                    out.push(fatal(format!(
                        "`{}` requires: `{key}` is not a generic action (read, create, update, delete, restore, purge)",
                        model.name
                    )));
                    continue;
                }
                match self.canonical(raw) {
                    None => out.push(fatal(format!("`{}` requires.{key}: `{raw}` is no permission (write `<resource>:<action>`)", model.name))),
                    Some(c) if !declared.contains(&c.permission) && !implied.contains(&c.permission) => out.push(fatal(format!(
                        "`{}` requires.{key} names `{}`, which the authorization permissions do not declare",
                        model.name, c.permission
                    ))),
                    _ => {}
                }
            }
        }
        for machine in &self.schema.rule_register.machines {
            for t in &machine.transitions {
                let Some(raw) = &t.requires else { continue };
                let verb = t.verb.as_deref().unwrap_or("?");
                match self.canonical(raw) {
                    None => out.push(fatal(format!("transition `{verb}` of `{}`: requires `{raw}` is no permission", machine.target))),
                    Some(c) if !declared.contains(&c.permission) && !implied.contains(&c.permission) => out.push(fatal(format!(
                        "transition `{verb}` of `{}` requires `{}`, which the authorization permissions do not declare",
                        machine.target, c.permission
                    ))),
                    _ => {}
                }
            }
        }
        out
    }

    /// Declared names no generic route, transition or system role uses: a hand verb must guard with
    /// them, or they are dead vocabulary. `schema doctor` drops the ones hand code names.
    pub fn unused_declared(&self) -> Vec<String> {
        let required: BTreeSet<String> =
            self.required().into_iter().map(|r| r.permission).collect();
        let granted: BTreeSet<String> = self
            .system_roles()
            .into_iter()
            .flat_map(|r| r.permissions)
            .collect();
        self.declared()
            .into_iter()
            .filter(|d| !required.contains(d) && !granted.contains(d))
            .collect()
    }

    /// The permissions grouped by what they are for, as `(model, [Required])`, for a report.
    pub fn by_model(&self) -> BTreeMap<String, Vec<Required>> {
        let mut out: BTreeMap<String, Vec<Required>> = BTreeMap::new();
        for r in self.required() {
            out.entry(r.model.clone()).or_default().push(r);
        }
        out
    }
}

fn sql_text(s: &str) -> String {
    format!("'{}'::text", s.replace('\'', "''"))
}

fn sql_text_or_null(s: Option<&str>) -> String {
    s.map(sql_text).unwrap_or_else(|| "NULL::text".to_string())
}

/// The seed migration's SQL for `seed`: `(up, down)`.
///
/// It writes into the identity module's tables (`public.permissions`, `public.roles`,
/// `public.role_permissions`): permissions are inserted or have their description refreshed, system
/// roles and their baseline grants are inserted where missing, and nothing is ever deleted or
/// revoked, so what a tenant edited (a role's grants, a role renamed or removed) survives a re-run.
/// Every statement is idempotent without leaning on a unique index. A database without the identity
/// tables (the identity module not composed, or its migrations not yet run) skips with a notice.
pub fn seed_sql(module: &str, seed: &Seed) -> (String, String) {
    let mut up = String::new();
    up.push_str(&format!(
        "-- Permission vocabulary and system roles of the {module} module\n"
    ));
    up.push_str("-- Generated by metaphor-schema\n");
    up.push_str("--\n");
    up.push_str("-- Writes into the identity module's tables (public.permissions, public.roles,\n");
    up.push_str(
        "-- public.role_permissions), so those migrations must have run first. Inserts what is\n",
    );
    up.push_str(
        "-- missing and never deletes or revokes: what a tenant changed survives a re-run.\n\n",
    );
    up.push_str("DO $$\nBEGIN\n");
    up.push_str("    IF to_regclass('public.permissions') IS NULL\n       OR to_regclass('public.roles') IS NULL\n       OR to_regclass('public.role_permissions') IS NULL THEN\n");
    up.push_str(&format!(
        "        RAISE NOTICE 'seed of the {module} permissions skipped: the identity module has not created public.permissions, public.roles and public.role_permissions';\n        RETURN;\n    END IF;\n\n"
    ));
    if !seed.permissions.is_empty() {
        let rows: Vec<String> = seed
            .permissions
            .iter()
            .map(|p| {
                format!(
                    "({}, {}, {}, {})",
                    sql_text(&p.name),
                    sql_text(&p.resource),
                    sql_text(&p.action),
                    sql_text(&p.description)
                )
            })
            .collect();
        let values = rows.join(",\n            ");
        up.push_str(&format!(
            "    UPDATE public.permissions p SET description = v.description\n      FROM (VALUES\n            {values}\n      ) AS v(name, resource, action, description)\n     WHERE p.name = v.name AND (p.metadata->>'deleted_at') IS NULL\n       AND p.description IS DISTINCT FROM v.description;\n\n"
        ));
        up.push_str(&format!(
            "    INSERT INTO public.permissions (name, resource, action, description)\n    SELECT v.name, v.resource, v.action, v.description\n      FROM (VALUES\n            {values}\n      ) AS v(name, resource, action, description)\n     WHERE NOT EXISTS (SELECT 1 FROM public.permissions p WHERE p.name = v.name AND (p.metadata->>'deleted_at') IS NULL)\n    ON CONFLICT DO NOTHING;\n\n"
        ));
    }
    if !seed.roles.is_empty() {
        let rows: Vec<String> = seed
            .roles
            .iter()
            .map(|r| {
                format!(
                    "({}, {})",
                    sql_text(&r.name),
                    sql_text_or_null(r.description.as_deref())
                )
            })
            .collect();
        up.push_str(&format!(
            "    INSERT INTO public.roles (name, description)\n    SELECT v.name, v.description\n      FROM (VALUES\n            {}\n      ) AS v(name, description)\n     WHERE NOT EXISTS (SELECT 1 FROM public.roles r WHERE r.name = v.name AND (r.metadata->>'deleted_at') IS NULL)\n    ON CONFLICT DO NOTHING;\n\n",
            rows.join(",\n            ")
        ));
        let grants: Vec<String> = seed
            .roles
            .iter()
            .flat_map(|r| {
                r.permissions
                    .iter()
                    .map(move |p| format!("({}, {})", sql_text(&r.name), sql_text(p)))
            })
            .collect();
        if !grants.is_empty() {
            up.push_str(&format!(
                "    INSERT INTO public.role_permissions (role_id, permission_id)\n    SELECT r.id, p.id\n      FROM (VALUES\n            {}\n      ) AS g(role, permission)\n      JOIN public.roles r ON r.name = g.role AND (r.metadata->>'deleted_at') IS NULL\n      JOIN public.permissions p ON p.name = g.permission AND (p.metadata->>'deleted_at') IS NULL\n     WHERE NOT EXISTS (SELECT 1 FROM public.role_permissions rp WHERE rp.role_id = r.id AND rp.permission_id = p.id)\n    ON CONFLICT DO NOTHING;\n\n",
                grants.join(",\n            ")
            ));
        }
    }
    up.push_str("END $$;\n");
    let down = format!("-- Down: the {module} permission seed is never reversed; permissions and grants may be in use\nDO $$ BEGIN END $$;\n");
    (up, down)
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A digest of text, stable across runs, for naming a content-addressed file.
pub fn digest(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::authorization::{AuthorizationConfig, RoleDefinition};

    fn schema() -> ModuleSchema {
        let mut s = ModuleSchema::new("Shop");
        let mut ticket = Model::new("Ticket");
        ticket.collection = Some("tickets".into());
        let mut stored = Model::new("StoredFile");
        stored.collection = Some("stored_files".into());
        s.models.push(ticket);
        s.models.push(stored);
        s
    }

    #[test]
    fn a_permission_reads_in_either_spelling_and_a_plural_maps_to_its_entity() {
        let s = schema();
        let r = Rbac::new(&s);
        assert_eq!(
            r.canonical("ticket:read").unwrap(),
            Canon {
                permission: "ticket:read".into(),
                spelling: Spelling::Canonical
            }
        );
        assert_eq!(
            r.canonical("tickets.read").unwrap(),
            Canon {
                permission: "ticket:read".into(),
                spelling: Spelling::Dotted
            }
        );
        assert_eq!(
            r.canonical("stored_files.*").unwrap().permission,
            "stored_file:*"
        );
        assert_eq!(r.canonical("*").unwrap().permission, "*:*");
        assert_eq!(
            r.canonical("ticket:transition:resolve").unwrap().spelling,
            Spelling::Transition
        );
        assert_eq!(
            r.canonical("categories.read").unwrap().permission,
            "category:read",
            "a resource no entity names reads as its singular"
        );
        assert!(r.canonical("a:b:c").is_none());
        assert!(r.canonical("Ticket Read").is_none());
    }

    #[test]
    fn an_entity_requires_its_generic_permissions_by_default_and_honours_a_rename() {
        let mut s = schema();
        s.models[0]
            .requires
            .insert("read".into(), "support:view".into());
        s.authorization = Some({
            let mut a = AuthorizationConfig::new();
            a.permissions.insert("support".into(), vec!["view".into()]);
            a
        });
        let r = Rbac::new(&s);
        let req = r.required();
        let read = req
            .iter()
            .find(|x| x.model == "Ticket" && x.kind == RouteKind::Generic(Gate::Read))
            .unwrap();
        assert_eq!(
            (read.permission.as_str(), read.renamed),
            ("support:view", true)
        );
        let purge = req
            .iter()
            .find(|x| x.model == "Ticket" && x.kind == RouteKind::Generic(Gate::Purge))
            .unwrap();
        assert_eq!(purge.permission, "ticket:purge");
        assert!(r.problems().iter().all(|p| !p.fatal), "{:?}", r.problems());
    }

    #[test]
    fn a_rename_to_an_undeclared_permission_is_a_problem() {
        let mut s = schema();
        s.models[0]
            .requires
            .insert("read".into(), "support:view".into());
        let p = Rbac::new(&s).problems();
        assert!(
            p.iter()
                .any(|p| p.fatal && p.message.contains("support:view")),
            "{p:?}"
        );
        s.models[0].requires.clear();
        s.models[0]
            .requires
            .insert("peek".into(), "ticket:read".into());
        let p = Rbac::new(&s).problems();
        assert!(
            p.iter().any(|p| p.fatal && p.message.contains("`peek`")),
            "{p:?}"
        );
    }

    #[test]
    fn the_vocabulary_is_the_declared_names_the_routes_and_the_roles_grants() {
        let mut s = schema();
        let mut a = AuthorizationConfig::new();
        a.permissions
            .insert("tickets".into(), vec!["escalate".into()]);
        let mut admin = RoleDefinition::new("admin");
        admin.permissions = vec!["tickets.*".into(), "stored_files.read".into()];
        let mut lead = RoleDefinition::new("lead");
        lead.inherits = Some("admin".into());
        lead.permissions = vec!["ticket:escalate".into()];
        a.roles = vec![admin, lead];
        s.authorization = Some(a);
        let r = Rbac::new(&s);
        let v = r.vocabulary();
        assert!(
            v.contains("ticket:escalate")
                && v.contains("ticket:read")
                && v.contains("ticket:*")
                && v.contains("stored_file:read")
        );
        let roles = r.system_roles();
        let lead = roles.iter().find(|x| x.name == "lead").unwrap();
        assert_eq!(
            lead.permissions,
            vec!["stored_file:read", "ticket:*", "ticket:escalate"],
            "inherited grants are folded in"
        );
        assert_eq!(r.legacy_spellings().len(), 2, "{:?}", r.legacy_spellings());
        let seed = r.seed();
        assert!(seed
            .permissions
            .iter()
            .any(|p| p.name == "ticket:*" && p.resource == "ticket" && p.action == "*"));
    }

    #[test]
    fn a_module_that_declares_nothing_is_complete() {
        let s = schema();
        let r = Rbac::new(&s);
        assert!(r.problems().is_empty());
        let v = r.vocabulary();
        for p in [
            "ticket:read",
            "ticket:create",
            "ticket:update",
            "ticket:delete",
            "ticket:restore",
            "ticket:purge",
            "stored_file:read",
        ] {
            assert!(v.contains(p), "{p}");
        }
        assert!(r.system_roles().is_empty());
    }

    #[test]
    fn a_read_only_model_requires_only_a_read() {
        let mut s = schema();
        s.models[0].read_only = true;
        let r = Rbac::new(&s);
        let ticket: Vec<_> = r
            .required()
            .into_iter()
            .filter(|x| x.model == "Ticket")
            .collect();
        assert_eq!(ticket.len(), 1);
        assert_eq!(ticket[0].permission, "ticket:read");
    }

    #[test]
    fn a_role_naming_a_missing_parent_or_a_malformed_grant_is_a_problem() {
        let mut s = schema();
        let mut a = AuthorizationConfig::new();
        let mut role = RoleDefinition::new("lead");
        role.inherits = Some("ghost".into());
        role.permissions = vec!["a:b:c".into()];
        a.roles = vec![role];
        s.authorization = Some(a);
        let p = Rbac::new(&s).problems();
        assert!(p.iter().any(|p| p.fatal && p.message.contains("ghost")));
        assert!(p.iter().any(|p| p.fatal && p.message.contains("a:b:c")));
    }

    #[test]
    fn the_digest_is_stable() {
        assert_eq!(digest("x"), digest("x"));
        assert_ne!(digest("x"), digest("y"));
    }
}
