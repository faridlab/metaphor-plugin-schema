//! Declared events the module never publishes.
//!
//! A schema file declares the events a module publishes under a top-level `events:`
//! map (a hook file or the model index). Nothing is generated from it: a module stages
//! its events by hand, in the write's transaction. The declaration is the module's
//! event contract, so a name that no hand-written source file mentions promises an
//! event no subscriber will ever receive. A mention is not proof the event is sent,
//! but a name with none is proof it is not.

use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// The marker every generated file carries in its header.
const GENERATED: &str = "by metaphor-schema. Do not edit manually.";

/// The event names the module's schema files declare under a top-level `events:` map.
pub(super) fn declared_events(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for entry in WalkDir::new(root.join("schema")).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("yaml" | "yml")) {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else { continue };
        let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(&text) else { continue };
        if let Some(events) = doc.get("events").and_then(|e| e.as_mapping()) {
            names.extend(events.keys().filter_map(|k| k.as_str().map(str::to_string)));
        }
    }
    names
}

/// One line per declared event that no hand-written `src/**/*.rs` file names as a whole
/// word. When only the name without its `Event` suffix appears, the line says so: the
/// module publishes the event under the shorter name and the declaration should match.
pub(super) fn unpublished_events(root: &Path) -> Vec<String> {
    let declared = declared_events(root);
    if declared.is_empty() {
        return Vec::new();
    }
    let hand: Vec<String> = WalkDir::new(root.join("src"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("rs"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter(|text| !text.lines().take(12).any(|l| l.contains(GENERATED)))
        .collect();
    let named = |name: &str| {
        let re = Regex::new(&format!(r"\b{}\b", regex::escape(name))).expect("an escaped name is a valid pattern");
        hand.iter().any(|text| re.is_match(text))
    };
    let mut out = Vec::new();
    for name in &declared {
        if named(name) {
            continue;
        }
        match name.strip_suffix("Event").filter(|stem| !stem.is_empty() && named(stem)) {
            Some(stem) => out.push(format!(
                "event '{name}' is declared, and the hand code publishes '{stem}': rename the declaration to match"
            )),
            None => out.push(format!(
                "event '{name}' is declared and no hand-written source names it: publish it, or remove the declaration"
            )),
        }
    }
    out
}

/// One entry per hand-written place that builds a generated service with `with_repository` and
/// attaches no guard in the same statement, instead of its generated `build_<entity>_service`,
/// which is what attaches the entity's write guard. Each entry says whether the entity has a
/// generated guard: such a service serves its writes without the entity's rules.
pub(super) fn services_built_by_hand(root: &Path) -> Vec<(String, bool)> {
    let built = Regex::new(r"\b([A-Z][A-Za-z0-9]*)Service::with_repository\(").expect("a valid pattern");
    let sources: Vec<(std::path::PathBuf, String)> = WalkDir::new(root.join("src"))
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("rs"))
        .filter_map(|e| fs::read_to_string(e.path()).ok().map(|t| (e.path().to_path_buf(), t)))
        .collect();
    let has_guard = |entity: &str| {
        let decl = format!("pub struct {entity}RowRules;");
        sources.iter().any(|(_, t)| t.contains(&decl))
    };
    let mut out = Vec::new();
    for (path, text) in &sources {
        if text.lines().take(12).any(|l| l.contains(GENERATED)) {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for cap in built.captures_iter(line) {
                // The statement this construction belongs to, to its `;`.
                let statement: String = lines[i..]
                    .iter()
                    .take(6)
                    .scan(false, |done, l| (!*done).then(|| { *done = l.contains(';'); *l }))
                    .collect::<Vec<_>>()
                    .join(" ");
                if statement.contains(".with_guard(") {
                    continue;
                }
                let entity = &cap[1];
                let rel = path.strip_prefix(root).unwrap_or(path).display();
                let guarded = has_guard(entity);
                out.push((
                    format!(
                        "{rel}:{} builds {entity}Service without its guard: use build_{}_service{}",
                        i + 1,
                        crate::utils::to_snake_case(entity),
                        if guarded { ", or the entity's rules check nothing on these writes" } else { "" }
                    ),
                    guarded,
                ));
            }
        }
    }
    out
}

/// One line per declared field no migration mentions beside its table: a field added to a model
/// whose create migration had already shipped, with no forward migration adding its column, so
/// every generic read or write of the entity names a column the database lacks. A computed field
/// is never stored and is left out. A mention is not proof the column exists, but a field with
/// none is proof it does not.
pub(super) fn fields_without_columns(schema: &crate::ast::ModuleSchema, root: &Path) -> Vec<String> {
    let migrations: Vec<String> = WalkDir::new(root.join("migrations"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().to_string_lossy().ends_with(".up.sql"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .collect();
    if migrations.is_empty() {
        return Vec::new();
    }
    let word = |name: &str| Regex::new(&format!(r"\b{}\b", regex::escape(name))).expect("an escaped name is a valid pattern");
    let mut out = Vec::new();
    for model in &schema.models {
        let table = word(&model.collection_name());
        let touching: Vec<&String> = migrations.iter().filter(|m| table.is_match(m)).collect();
        if touching.is_empty() {
            continue;
        }
        for field in model.fields.iter().filter(|f| !f.has_attribute("computed")) {
            let column = word(&field.name);
            if !touching.iter().any(|m| column.is_match(m)) {
                out.push(format!(
                    "{}.{} is declared and no migration of {} names it: add a forward migration for its column",
                    model.name,
                    field.name,
                    model.collection_name()
                ));
            }
        }
    }
    out
}

/// One line per hand-written place that binds an optional acting unit (`scope.map(|s| s.acting_unit_id())`)
/// into a write: without a task-level scope it binds NULL, which only the table's
/// `<table>_org_unit_fill` trigger turns back into the unit the connection carries. Leaving
/// `org_unit_id` out of the insert lets the column default stamp it with no trigger to rely on.
pub(super) fn optional_unit_binds(root: &Path) -> Vec<String> {
    let bound = Regex::new(r"\.map\(\|\w+\|\s*\w+\.acting_unit_id\(\)\)").expect("a valid pattern");
    let mut out = Vec::new();
    for entry in WalkDir::new(root.join("src")).sort_by_file_name().into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else { continue };
        if text.lines().take(12).any(|l| l.contains(GENERATED)) {
            continue;
        }
        for (i, line) in text.lines().enumerate() {
            if !line.trim_start().starts_with("//") && line.contains(".bind(") && bound.is_match(line) {
                let rel = path.strip_prefix(root).unwrap_or(path).display();
                out.push(format!(
                    "{rel}:{} binds an optional acting unit: with no task scope it writes NULL and leans on the org_unit_fill trigger; leave org_unit_id out of the insert so its default stamps it",
                    i + 1
                ));
            }
        }
    }
    out
}

/// One line per hand-written route whose path parameters use the other axum version's syntax:
/// before axum 0.8 a parameter is `:id` and `{id}` is a literal segment, so the route never
/// matches; from 0.8 a parameter is `{id}` and a `:id` segment panics when the router is built.
/// The version is the module's locked axum; a module without a lock reports nothing.
pub(super) fn route_param_syntax(root: &Path) -> Vec<String> {
    let Ok(lock) = fs::read_to_string(root.join("Cargo.lock")) else { return Vec::new() };
    let version = Regex::new(r#"name = "axum"\nversion = "(\d+)\.(\d+)"#).expect("a valid pattern");
    let Some(cap) = version.captures(&lock) else { return Vec::new() };
    let (major, minor): (u32, u32) = (cap[1].parse().unwrap_or(0), cap[2].parse().unwrap_or(0));
    let braces = major == 0 && minor >= 8 || major >= 1;
    let route = Regex::new(r#"\.route\(\s*"([^"]*)""#).expect("a valid pattern");
    let (wrong, right) = if braces { (Regex::new(r"/:[A-Za-z_]").unwrap(), "{id}") } else { (Regex::new(r"\{[A-Za-z_]+\}").unwrap(), ":id") };
    let mut out = Vec::new();
    for entry in WalkDir::new(root.join("src")).sort_by_file_name().into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else { continue };
        for cap in route.captures_iter(&text) {
            let m = cap.get(1).expect("the path group");
            if wrong.is_match(m.as_str()) {
                let line = text[..m.start()].matches('\n').count() + 1;
                let rel = path.strip_prefix(root).unwrap_or(path).display();
                out.push(format!(
                    "{rel}:{line} route \"{}\" names its parameters the way axum {major}.{minor} does not: write them as {right}",
                    m.as_str()
                ));
            }
        }
    }
    out
}

/// The `rust_decimal` features that change how EVERY decimal of the composed service serializes:
/// Cargo unifies features across a build, so one module turning one on changes money, rates and
/// quantities in every other module's responses.
const GLOBAL_DECIMAL_SWITCHES: &[&str] = &["serde-float", "serde-arbitrary-precision"];

/// One line per `rust_decimal` feature in the module's `Cargo.toml` that changes the default wire
/// shape of every decimal in the build. A field that wants a number asks for one itself, with
/// `#[serde(with = "rust_decimal::serde::float")]` under the `serde-with-float` feature.
pub(super) fn global_decimal_switches(root: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(root.join("Cargo.toml")) else { return Vec::new() };
    let mut section = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            section = t.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let declares = t.starts_with("rust_decimal") && t["rust_decimal".len()..].trim_start().starts_with('=');
        if !declares || t.starts_with('#') {
            continue;
        }
        for f in GLOBAL_DECIMAL_SWITCHES {
            if t.contains(&format!("\"{f}\"")) {
                out.push(format!(
                    "Cargo.toml [{section}] turns on rust_decimal/{f}, which changes how every decimal in the composed service serializes: use serde-with-float and a per-field `#[serde(with = \"rust_decimal::serde::float\")]` instead"
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn a_declared_event_counts_as_published_only_when_hand_code_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "schema/hooks/order.hook.yaml", "events:\n  OrderPlaced:\n    payload: [id]\n  OrderShipped: {}\n  UserCreatedEvent: {}\n  OrderArchived: {}\n");
        write(root, "schema/hooks/index.hook.yaml", "events: {}\n");
        write(root, "schema/models/index.model.yaml", "events:\n  StockMoved: {}\n");
        write(root, "src/service/orders.rs", "let r = OutboxRecord::new(\"OrderPlaced\", \"Order\");\nenum E { StockMoved(Id), UserCreated }\n");
        // A generated file names OrderShipped, which proves nothing about hand publishing.
        write(root, "src/exports/events.rs", "// Generated by metaphor-schema. Do not edit manually.\npub struct OrderShipped; pub struct OrderArchivedSoon;\n");

        assert_eq!(declared_events(root).len(), 5);
        let out = unpublished_events(root);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out[0].contains("'OrderArchived'") && out[0].contains("no hand-written source"), "a longer word is no mention: {out:?}");
        assert!(out[1].contains("'OrderShipped'"), "{out:?}");
        assert!(out[2].contains("'UserCreatedEvent'") && out[2].contains("publishes 'UserCreated'"), "{out:?}");
    }

    #[test]
    fn a_service_built_without_its_guard_is_named_and_a_guarded_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "src/lib.ext.rs",
            "let s = Arc::new(OrderService::with_repository(repo.clone()));\n// OrderService::with_repository(old) in a comment\nlet t = Arc::new(TagService::with_repository(r)\n    .with_guard(Arc::new(TagRowRules)));\nlet n = Arc::new(NoteService::with_repository(r));\n",
        );
        write(root, "src/application/service/mod.rs", "// Generated by metaphor-schema. Do not edit manually.\npub fn build_order_service(r: R) -> OrderService { OrderService::with_repository(r) }\npub struct OrderRowRules;\npub struct TagRowRules;\n");
        let out = services_built_by_hand(root);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].0.starts_with("src/lib.ext.rs:1 builds OrderService without its guard") && out[0].1, "{out:?}");
        assert!(out[1].0.starts_with("src/lib.ext.rs:5 builds NoteService") && !out[1].1, "a service with no guard to attach: {out:?}");
    }

    #[test]
    fn a_crate_wide_decimal_switch_is_reported_and_the_per_field_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", "[package]\nname = \"m\"\n[dependencies]\nrust_decimal = { version = \"1.36\", features = [\"serde\", \"serde-float\"] }\n");
        let out = global_decimal_switches(dir.path());
        assert!(out.len() == 1 && out[0].contains("rust_decimal/serde-float"), "{out:?}");
        write(dir.path(), "Cargo.toml", "[package]\nname = \"m\"\n[dependencies]\nrust_decimal = { version = \"1.36\", features = [\"serde\", \"serde-with-float\"] }\n");
        assert!(global_decimal_switches(dir.path()).is_empty());
    }

    #[test]
    fn a_bound_optional_acting_unit_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/handler.rs", "q.bind(employee_id)\n    .bind(backbone_orm::org_scope::current_org_scope().map(|s| s.acting_unit_id()))\n    .bind(scope.acting_unit_id())\n    .bind(scope.as_ref().map(|s| s.acting_unit_id()))\n");
        let out = optional_unit_binds(dir.path());
        assert!(out.len() == 2 && out[0].starts_with("src/handler.rs:2 binds an optional acting unit") && out[1].starts_with("src/handler.rs:4 "), "{out:?}");
    }

    #[test]
    fn a_route_in_the_other_axum_syntax_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Cargo.lock", "[[package]]\nname = \"axum\"\nversion = \"0.7.9\"\n");
        write(root, "src/routes.rs", "Router::new()\n    .route(\"/a/:id\", get(a))\n    .route(\n        \"/b/{id}/c\", post(b))\n");
        let out = route_param_syntax(root);
        assert!(out.len() == 1 && out[0].starts_with("src/routes.rs:4 route \"/b/{id}/c\"") && out[0].ends_with(":id"), "{out:?}");
        write(root, "Cargo.lock", "[[package]]\nname = \"axum\"\nversion = \"0.8.1\"\n");
        let out = route_param_syntax(root);
        assert!(out.len() == 1 && out[0].starts_with("src/routes.rs:2 route \"/a/:id\""), "{out:?}");
    }

    #[test]
    fn a_module_that_declares_no_events_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "schema/hooks/index.hook.yaml", "events: {}\n");
        assert!(unpublished_events(dir.path()).is_empty());
    }
}
