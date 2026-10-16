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

/// One line per hand-written place that builds a generated service with `with_repository`
/// instead of its generated `build_<entity>_service`, which is what attaches the entity's write
/// guard: a service built by hand serves its writes without the rules unless it remembers to.
pub(super) fn services_built_by_hand(root: &Path) -> Vec<String> {
    let built = Regex::new(r"\b([A-Z][A-Za-z0-9]*)Service::with_repository\(").expect("a valid pattern");
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
            if line.trim_start().starts_with("//") {
                continue;
            }
            for cap in built.captures_iter(line) {
                let entity = &cap[1];
                let rel = path.strip_prefix(root).unwrap_or(path).display();
                out.push(format!(
                    "{rel}:{} builds {entity}Service by hand: use build_{}_service, which attaches its write guard",
                    i + 1,
                    crate::utils::to_snake_case(entity)
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
    fn a_service_built_by_hand_is_named_with_its_constructor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/lib.ext.rs", "let s = Arc::new(OrderService::with_repository(repo.clone()));\n// OrderService::with_repository(old) in a comment\n");
        write(root, "src/application/service/mod.rs", "// Generated by metaphor-schema. Do not edit manually.\npub fn build_order_service(r: R) -> OrderService { OrderService::with_repository(r) }\n");
        let out = services_built_by_hand(root);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].starts_with("src/lib.ext.rs:1 builds OrderService by hand: use build_order_service"), "{out:?}");
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
    fn a_module_that_declares_no_events_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "schema/hooks/index.hook.yaml", "events: {}\n");
        assert!(unpublished_events(dir.path()).is_empty());
    }
}
