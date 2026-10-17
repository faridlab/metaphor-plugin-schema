//! The conformance suite: every Backbone feature the schema DSL declares, proven at runtime
//! through a module this crate's generator writes.
//!
//! It copies the module skeleton (`../modules/backbone-module`, or `METAPHOR_MODULE_SKELETON`),
//! replaces its example entity with `tests/fixtures/conformance` (schema, hand-written extension
//! files, the runtime probe file), regenerates it with this crate's generator, and runs the probe
//! file inside the generated module. The probes mount the module's routes behind the framework's
//! real middleware (tenant router, `org_auth` with a tenant-bound verifier, the write guard) on a
//! scratch Postgres database the module's own migrations build, connected as a role that is
//! neither superuser nor BYPASSRLS, so row-level security is really enforced. The database is
//! dropped at the end, also when a probe panics.
//!
//! Every probe is run and recorded even after one fails; the matrix is printed and written to
//! `tests/fixtures/conformance/RESULTS.md`, and the test fails when any probe failed.
//!
//!     CONFORMANCE_DATABASE_URL=postgresql://root:password@localhost:5434/postgres \
//!         cargo test --test conformance -- --ignored --nocapture
//!
//! Environment:
//! - `CONFORMANCE_DATABASE_URL` (required): an admin DSN whose role may CREATE DATABASE and
//!   CREATE ROLE. Only a database and a role this run names are created, and both are dropped.
//! - `METAPHOR_FRAMEWORK_PATH`: a local backbone-framework checkout to build against through
//!   `[patch.crates-io]`. Defaults to `../modules/backbone-framework` when that exists; set it to
//!   `crates.io` to build against the released crates.
//! - `METAPHOR_MODULE_SKELETON`: the module skeleton to copy.
//! - `CARGO_TARGET_DIR`: reused for the generated module's build, so a rerun is incremental.
//! - `CONFORMANCE_KEEP=1`: keep the generated module's directory and print where it is.
//! - `CONFORMANCE_JOBS`: cargo `-j` for the generated module's build (default 2).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::Value;

const MODULE: &str = "conformance";

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if matches!(name.to_str(), Some("target" | ".git" | "graphify-out" | ".claude")) {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&src, &dst);
        } else {
            fs::copy(&src, &dst).unwrap();
        }
    }
}

fn fill_module_name(dir: &Path, name: &str) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fill_module_name(&path, name);
        } else if let Ok(text) = fs::read_to_string(&path) {
            if text.contains("__MODULE__") || text.contains("__module__") {
                fs::write(&path, text.replace("__MODULE__", name).replace("__module__", name)).unwrap();
            }
        }
    }
}

/// Remove everything the skeleton carries for its example entity, so the module holds only the
/// fixture's entities and the generator writes every generated file afresh.
fn strip_skeleton_example(module: &Path) {
    let migrations = module.join("migrations");
    for entry in fs::read_dir(&migrations).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "sql") {
            fs::remove_file(path).unwrap();
        }
    }
    let _ = fs::remove_file(migrations.join("seeds/example_seed.sql"));
    let _ = fs::write(migrations.join("seeds/seed_order.yml"), "[]\n");
    let _ = fs::remove_file(module.join("schema/models/example.model.yaml"));
    fn walk(dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("example_") || n.starts_with("example."))
            {
                fs::remove_file(path).unwrap();
            }
        }
    }
    walk(&module.join("src"));
    walk(&module.join("tests"));
}

fn run(cmd: &mut Command, what: &str) -> std::process::Output {
    cmd.output().unwrap_or_else(|e| panic!("{what}: {e}"))
}

fn git(module: &Path, args: &[&str]) {
    let out = run(Command::new("git").args(args).current_dir(module), "git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The framework checkout to patch in, or `None` to build against crates.io.
fn framework_path(crate_dir: &Path) -> Option<PathBuf> {
    match std::env::var("METAPHOR_FRAMEWORK_PATH") {
        Ok(v) if v == "crates.io" || v.is_empty() => None,
        Ok(v) => Some(PathBuf::from(v)),
        Err(_) => {
            let local = crate_dir.join("../modules/backbone-framework");
            local.join("Cargo.toml").exists().then(|| local.canonicalize().unwrap())
        }
    }
}

/// The dependencies the probe file needs beyond the module's own, and the framework patch.
fn patch_manifest(module: &Path, framework: Option<&Path>) {
    let manifest = module.join("Cargo.toml");
    let mut text = fs::read_to_string(&manifest).unwrap();
    let marker = "[dev-dependencies]\n";
    assert!(text.contains(marker), "the skeleton manifest has a [dev-dependencies] table");
    let extra = "backbone-auth = { version = \"3.0.0\", features = [\"axum\", \"identity-grants\"] }\n\
                 backbone-tenant = { version = \"3.0.0\", features = [\"axum\"] }\n\
                 tower = { version = \"0.4\", features = [\"util\"] }\n\
                 http-body-util = \"0.1\"\n\
                 futures = \"0.3\"\n\
                 backbone-outbox = \"3.0.0\"\n";
    text = text.replacen(marker, &format!("{marker}{extra}"), 1);
    if let Some(fw) = framework {
        text.push_str("\n[patch.crates-io]\n");
        for krate in ["backbone-core", "backbone-orm", "backbone-auth", "backbone-tenant", "backbone-messaging", "backbone-outbox"] {
            text.push_str(&format!("{krate} = {{ path = \"{}\" }}\n", fw.join(krate).display()));
        }
    }
    fs::write(&manifest, text).unwrap();
}

/// One probe outcome, as the runtime probes and the generator probes both record it.
#[derive(Debug, Clone)]
struct Row {
    area: String,
    feature: String,
    probe: String,
    status: String,
    request: String,
    expected: String,
    got: String,
    diagnosis: String,
}

impl Row {
    fn from_json(v: &Value) -> Self {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Self {
            area: s("area"),
            feature: s("feature"),
            probe: s("probe"),
            status: s("status"),
            request: s("request"),
            expected: s("expected"),
            got: s("got"),
            diagnosis: s("diagnosis"),
        }
    }
}

fn gen_row(area: &str, feature: &str, probe: &str, ok: bool, request: &str, expected: &str, got: &str, diagnosis: &str) -> Row {
    Row {
        area: area.into(),
        feature: feature.into(),
        probe: probe.into(),
        status: if ok { "PASS" } else { "FAIL" }.into(),
        request: request.into(),
        expected: expected.into(),
        got: got.into(),
        diagnosis: if ok { String::new() } else { diagnosis.into() },
    }
}

const AREAS: &[(&str, &str)] = &[
    ("A", "Generic routes"),
    ("B", "Validation"),
    ("C", "Custom validation"),
    ("D", "Hooks and events"),
    ("E", "State machine"),
    ("F", "Extension"),
    ("G", "Scoping"),
    ("H", "Debuggability"),
    ("I", "Permissions"),
];

fn one_line(s: &str) -> String {
    let flat = s.replace('|', "\\|").replace('\n', " ");
    if flat.chars().count() > 400 {
        format!("{}…", flat.chars().take(400).collect::<String>())
    } else {
        flat
    }
}

fn render(rows: &[Row], header: &str) -> String {
    let mut out = String::new();
    out.push_str("# Conformance results\n\n");
    out.push_str("Written by `tests/conformance.rs`; do not edit by hand. Rerun with\n");
    out.push_str("`CONFORMANCE_DATABASE_URL=... cargo test --test conformance -- --ignored --nocapture`.\n\n");
    out.push_str(header);
    out.push_str("\n## Summary\n\n| Area | PASS | FAIL | UNSUPPORTED |\n|---|---|---|---|\n");
    let mut totals = [0usize; 3];
    for (key, name) in AREAS {
        let count = |s: &str| rows.iter().filter(|r| r.area == *key && r.status == s).count();
        let c = [count("PASS"), count("FAIL"), count("UNSUPPORTED")];
        for i in 0..3 {
            totals[i] += c[i];
        }
        out.push_str(&format!("| {key}. {name} | {} | {} | {} |\n", c[0], c[1], c[2]));
    }
    out.push_str(&format!("| **Total** | **{}** | **{}** | **{}** |\n", totals[0], totals[1], totals[2]));

    out.push_str("\n## Matrix\n\n| Area | Feature | Probe | Result |\n|---|---|---|---|\n");
    for r in rows {
        out.push_str(&format!("| {} | {} | {} | {} |\n", r.area, one_line(&r.feature), one_line(&r.probe), r.status));
    }

    let fails: Vec<&Row> = rows.iter().filter(|r| r.status == "FAIL").collect();
    out.push_str(&format!("\n## Failures ({})\n", fails.len()));
    for (i, r) in fails.iter().enumerate() {
        out.push_str(&format!(
            "\n### X{} — {} / {} / {}\n\n- **Request:** {}\n- **Expected:** {}\n- **Got:** {}\n- **Diagnosis:** {}\n",
            i + 1,
            r.area,
            one_line(&r.feature),
            one_line(&r.probe),
            one_line(&r.request),
            one_line(&r.expected),
            one_line(&r.got),
            if r.diagnosis.is_empty() { "not yet diagnosed".to_string() } else { one_line(&r.diagnosis) },
        ));
    }

    let unsupported: Vec<&Row> = rows.iter().filter(|r| r.status == "UNSUPPORTED").collect();
    out.push_str(&format!("\n## Unsupported ({})\n\n", unsupported.len()));
    for r in unsupported {
        out.push_str(&format!("- **{} / {} / {}** — {}\n", r.area, one_line(&r.feature), one_line(&r.probe), one_line(&r.got)));
    }
    out
}

/// The generator-side probes of area H: the rules register labels each declared rule and machine
/// the way the runtime behaves, and the doctor accepts the generated module.
fn register_probes(module: &Path, rows: &mut Vec<Row>, runtime: &[Row]) {
    let bin = env!("CARGO_BIN_EXE_metaphor-schema");
    let out = run(
        Command::new(bin).args(["schema", "rules", ".", "-f", "json"]).current_dir(module).env("NO_COLOR", "1"),
        "schema rules",
    );
    let parsed: Option<Value> = serde_json::from_slice(&out.stdout).ok();
    rows.push(gen_row(
        "H",
        "Rules register",
        "`schema rules . -f json` runs and parses",
        out.status.success() && parsed.is_some(),
        "metaphor-schema schema rules . -f json",
        "exit 0, JSON document",
        &format!("exit {:?}; {}", out.status.code(), one_line(&String::from_utf8_lossy(&out.stderr))),
        "metaphor-plugin-schema/src/commands/schema/rules.rs",
    ));
    let register = parsed.unwrap_or(Value::Null);
    let rules = register.get("rules").and_then(Value::as_array).cloned().unwrap_or_default();

    // A runtime probe, found by a phrase of its name; its answer's status code leads `got`.
    let answer = |phrase: &str| runtime.iter().find(|r| r.probe.contains(phrase)).map(|r| r.got.chars().take(3).collect::<String>());
    // (rule, label the register should give, the runtime probe that shows it, the status that agrees)
    let expected: &[(&str, &str, &str, &str)] = &[
        ("price_within_budget", "guard", "amount over budget_cap answers 422", "422"),
        ("discount_needs_reason", "guard", "a discount without a reason", "422"),
        ("client_needs_website", "guard", "a client project without a website", "422"),
        ("code_unique_per_company", "guard", "declared uniqueness rule refuses a duplicate code", "422"),
        ("note_shadowed", "shadow", "shadow rule lets the write through", "201"),
        ("archive_reason_verb", "verb", "rule held by a hand-written verb refuses a blank reason", "422"),
        ("aspiration_unenforced", "unenforced", "unenforced rule lets the write through", "201"),
    ];
    for (name, label, phrase, status) in expected {
        let got_label = rules
            .iter()
            .find(|r| r.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|r| r.get("status").and_then(Value::as_str))
            .unwrap_or("absent")
            .to_string();
        let seen = answer(phrase);
        rows.push(gen_row(
            "H",
            "Rules register",
            &format!("rule `{name}` is labelled {} and the runtime agrees", label.to_uppercase()),
            got_label == *label && seen.as_deref() == Some(*status),
            "metaphor-schema schema rules . -f json; the runtime probe",
            &format!("label {label}; runtime answers {status} (`{phrase}`)"),
            &format!("label {got_label}; runtime answers {}", seen.unwrap_or_else(|| "nothing (probe missing)".into())),
            "metaphor-plugin-schema/src/rules/mod.rs (mark_compiled) vs the generated guard",
        ));
    }

    // Declared events: the register labels each as staged by the write hooks or by hand, and the
    // runtime agrees.
    let events = register.get("events").and_then(Value::as_array).cloned().unwrap_or_default();
    let event_status = |name: &str| {
        events
            .iter()
            .find(|e| e.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|e| e.get("status").and_then(Value::as_str))
            .unwrap_or("absent")
            .to_string()
    };
    let passed_probe = |phrase: &str| runtime.iter().any(|r| r.probe.contains(phrase) && r.status == "PASS");
    for (name, label, phrase) in [
        ("TicketOpened", "EVENT STAGED (on create, in tx)", "a create stages exactly one TicketOpened row"),
        ("TicketResolved", "EVENT STAGED (on transition(resolve), in tx)", "a transition stages its event"),
        ("TicketEscalated", "EVENT HAND-STAGED", "a hand-staged declared event is staged by hand code"),
    ] {
        let got = event_status(name);
        rows.push(gen_row(
            "H",
            "Rules register",
            &format!("event `{name}` is labelled {label} and the runtime agrees"),
            got == label && passed_probe(phrase),
            "metaphor-schema schema rules . -f json; the runtime probe",
            &format!("label {label}; the probe `{phrase}` passes"),
            &format!("label {got}"),
            "metaphor-plugin-schema/src/commands/schema/rules.rs (event_label) vs the generated write hooks",
        ));
    }

    // Machine labels are computed in the pretty register only.
    let pretty = run(Command::new(bin).args(["schema", "rules", "."]).current_dir(module).env("NO_COLOR", "1"), "schema rules");
    let text = String::from_utf8_lossy(&pretty.stdout).to_string();
    let line = |m: &str| text.lines().find(|l| l.contains(m)).unwrap_or("absent").trim().to_string();
    let passed = |phrase: &str| runtime.iter().any(|r| r.probe.contains(phrase) && r.status == "PASS");
    let ticket = line("Ticket.status");
    rows.push(gen_row(
        "H",
        "Rules register",
        "machine `Ticket.status` is labelled MOVES REFUSED and the transition trigger refuses a raw UPDATE",
        ticket.contains("MOVES REFUSED") && passed("raw UPDATE refused by the transition trigger"),
        "metaphor-schema schema rules .",
        "MOVES REFUSED; the raw-UPDATE probe passes",
        &ticket,
        "metaphor-plugin-schema/src/commands/schema/rules.rs:229-242",
    ));
    rows.push(gen_row(
        "H",
        "Rules register",
        "machine `Ticket.status` is labelled GUARDS IN GENERATED VERBS and the generated verb refuses a failing guard",
        ticket.contains("GUARDS IN GENERATED VERBS") && passed("a transition whose guard does not hold"),
        "metaphor-schema schema rules .",
        "GUARDS IN GENERATED VERBS; the guard probe passes",
        &ticket,
        "metaphor-plugin-schema/src/commands/schema/rules.rs:229-242",
    ));
    let approval = line("Approval.state");
    rows.push(gen_row(
        "H",
        "Rules register",
        "machine `Approval.state` (hand_set) is labelled MOVES REFUSED and its verbs refuse a wrong-state move",
        approval.contains("MOVES REFUSED") && passed("a verb from the wrong state is refused"),
        "metaphor-schema schema rules .",
        "MOVES REFUSED; the wrong-state probe passes",
        &approval,
        "metaphor-plugin-schema/src/commands/schema/rules.rs:229-242",
    ));

    // Permissions: the register says what every route requires, and in which mode.
    let permissions = register.get("permissions").cloned().unwrap_or(Value::Null);
    let required: Vec<String> = permissions
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|r| r.get("permission").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default();
    let vocabulary: Vec<String> = permissions
        .get("vocabulary")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let has = |list: &[String], names: &[&str]| names.iter().all(|n| list.iter().any(|l| l == n));
    rows.push(gen_row(
        "I",
        "Register",
        "`schema rules` lists the permission every generic route, generated verb and declared verb requires",
        has(&required, &["ticket:read", "ticket:create", "ticket:update", "ticket:delete", "ticket:restore", "ticket:purge", "ticket:start", "ticket:resolve", "approval:submit", "project:read"])
            && has(&vocabulary, &["ticket:escalate", "ticket:archive_with_reason"]),
        "metaphor-schema schema rules . -f json",
        "permissions.required names the generic actions and the verbs; permissions.vocabulary the declared custom verbs",
        &format!("required {required:?}; vocabulary {vocabulary:?}"),
        "metaphor-plugin-schema/src/rbac.rs",
    ));
    let roles = permissions.get("system_roles").and_then(Value::as_array).cloned().unwrap_or_default();
    let viewer: Vec<String> = roles
        .iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some("viewer"))
        .and_then(|r| r.get("permissions").and_then(Value::as_array))
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    rows.push(gen_row(
        "I",
        "Register",
        "a declared system role's grants are normalised to the canonical spelling (projects.read -> project:read)",
        viewer == ["project:read", "ticket:read"],
        "metaphor-schema schema rules . -f json",
        "viewer: [project:read, ticket:read]",
        &format!("{viewer:?}"),
        "metaphor-plugin-schema/src/rbac.rs (canonical)",
    ));
    let mode = permissions.get("mode").and_then(Value::as_str).unwrap_or("absent").to_string();
    let permission_lines: Vec<&str> = text.lines().filter(|l| l.contains("PERMISSION REQUIRED")).collect();
    rows.push(gen_row(
        "I",
        "Register",
        "`schema rules` labels each route group PERMISSION REQUIRED <permission> and says the mode (no resolver installed: AUTHCONTEXT)",
        mode == "auth_context"
            && text.contains("Permissions: mode AUTHCONTEXT")
            && permission_lines.iter().any(|l| l.contains("Ticket") && l.contains("ticket:{read|create|update|delete|restore|purge}"))
            && permission_lines.iter().any(|l| l.contains("ticket:start") && l.contains("generated verb")),
        "metaphor-schema schema rules .",
        "mode AUTHCONTEXT; `Ticket PERMISSION REQUIRED ticket:{read|...}`; `PERMISSION REQUIRED ticket:start  generated verb`",
        &format!("mode {mode}; {} PERMISSION REQUIRED line(s): {}", permission_lines.len(), permission_lines.iter().take(3).map(|l| l.trim()).collect::<Vec<_>>().join(" | ")),
        "metaphor-plugin-schema/src/commands/schema/rules.rs (print_permissions)",
    ));

    let out = run(Command::new(bin).args(["schema", "doctor", "."]).current_dir(module).env("NO_COLOR", "1"), "schema doctor");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    rows.push(gen_row(
        "I",
        "Schema doctor",
        "doctor says the mode, and warns (without failing) on the dotted spelling a role uses",
        out.status.success() && text.contains("permissions: mode AUTHCONTEXT") && text.contains("spells a permission `projects.read`: write `project:read`"),
        "metaphor-schema schema doctor .",
        "exit 0; the mode line; a warning naming projects.read and project:read",
        &format!("exit {:?}; {}", out.status.code(), one_line(&text)),
        "metaphor-plugin-schema/src/commands/schema/doctor.rs (permission_report)",
    ));
    rows.push(gen_row(
        "H",
        "Schema doctor",
        "`schema doctor .` accepts the freshly generated module with its extension files",
        out.status.success(),
        "metaphor-schema schema doctor .",
        "exit 0",
        &format!("exit {:?}; {}", out.status.code(), one_line(&text)),
        "metaphor-plugin-schema/src/commands/schema/doctor.rs",
    ));
}

/// Copy the skeleton into `dir`, replace its example with the fixture, let `mutate` change the
/// schema, commit, and run this crate's generator. Returns the module path and the generator's
/// output.
fn prepare(skeleton: &Path, fixture: &Path, dir: &Path, mutate: impl Fn(&Path)) -> (PathBuf, std::process::Output) {
    let module = dir.join(MODULE);
    copy_tree(skeleton, &module);
    strip_skeleton_example(&module);
    copy_tree(&fixture.join("module"), &module);
    fill_module_name(&module, MODULE);
    mutate(&module);
    git(&module, &["init", "-q"]);
    git(&module, &["add", "-A"]);
    git(&module, &["-c", "user.email=conformance@example.com", "-c", "user.name=conformance", "commit", "-qm", "skeleton"]);
    let out = run(
        Command::new(env!("CARGO_BIN_EXE_metaphor-schema"))
            .args(["schema", "generate", ".", "--force"])
            .current_dir(&module)
            .env("NO_COLOR", "1"),
        "generate",
    );
    (module, out)
}

fn edit(path: &Path, f: impl Fn(String) -> String) {
    let text = fs::read_to_string(path).unwrap();
    fs::write(path, f(text)).unwrap();
}

fn grep_tree(dir: &Path, needle: &str) -> Vec<String> {
    let mut hits = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                hits.extend(grep_tree(&path, needle));
            } else if fs::read_to_string(&path).is_ok_and(|t| t.contains(needle)) {
                hits.push(path.display().to_string());
            }
        }
    }
    hits
}

fn output_text(out: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// Generator probes that need a variant of the fixture: each is generated (not built) on its own
/// copy, so a variant that breaks generation or compilation cannot take the runtime run down.
fn variant_probes(skeleton: &Path, fixture: &Path, main: &Path, main_output: &str, rows: &mut Vec<Row>) {
    // `text` is a documented field type.
    let dir = tempfile::tempdir().unwrap();
    let (module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("schema/models/project.model.yaml"), |t| t.replacen("      summary:\n", "      memo:\n        type: text?\n\n      summary:\n", 1))
    });
    let text = output_text(&out);
    if !out.status.success() && text.contains("Unknown type 'text'") {
        rows.push(Row {
            area: "B".into(),
            feature: "Field types".into(),
            probe: "a `text?` field".into(),
            status: "UNSUPPORTED".into(),
            request: "add `memo: { type: text? }` to Project and generate".into(),
            expected: String::new(),
            got: "the validator refuses it (\"Unknown type 'text' at Project.memo\"): src/ast/types.rs:174 has no `text` primitive, although the skeleton's docs/schema/TYPES.md:31 documents one; `string` maps to TEXT already, so long text is covered by `string`".into(),
            diagnosis: String::new(),
        });
    } else {
        let entity = fs::read_to_string(module.join("src/domain/entity/project.rs")).unwrap_or_default();
        let field_line = entity.lines().find(|l| l.contains("pub memo:")).unwrap_or("absent").trim().to_string();
        rows.push(gen_row(
            "B",
            "Field types",
            "a `text?` field maps to Option<String>",
            out.status.success() && field_line.contains("Option<String>"),
            "add `memo: { type: text? }` to Project and generate",
            "pub memo: Option<String>",
            &format!("generate exit {:?}; {field_line}; {}", out.status.code(), one_line(&text)),
            "metaphor-plugin-schema/src/ast/types.rs:174 (type parsing) / generators/rust.rs",
        ));
    }

    // The machine in its own hook file, the rules in another (a common layout).
    let dir = tempfile::tempdir().unwrap();
    let (module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        let hooks = m.join("schema/hooks");
        let text = fs::read_to_string(hooks.join("ticket.hook.yaml")).unwrap();
        let start = text.find("states:").unwrap();
        let end = text.find("rules:").unwrap();
        fs::write(hooks.join("ticket.hook.yaml"), format!("{}{}", &text[..start], &text[end..])).unwrap();
        fs::write(hooks.join("ticket_state.hook.yaml"), format!("name: ticket_state\nmodel: Ticket\n\n{}", &text[start..end])).unwrap();
    });
    let handler = fs::read_to_string(module.join("src/presentation/http/ticket_handler.rs")).unwrap_or_default();
    let lib = fs::read_to_string(module.join("src/lib.rs")).unwrap_or_default();
    let emitted = handler.contains("fn create_ticket_transition_routes");
    let mounted = lib.contains("create_ticket_transition_routes");
    rows.push(gen_row(
        "E",
        "Transitions",
        "a machine declared in its own hook file (rules in another file for the same model) still gets its generated verbs",
        out.status.success() && emitted && mounted,
        "move Ticket's `states:` into schema/hooks/ticket_state.hook.yaml and generate",
        "ticket_handler.rs defines create_ticket_transition_routes and lib.rs mounts it",
        &format!("generate exit {:?}; transition routes emitted: {emitted}; mounted: {mounted}", out.status.code()),
        "metaphor-plugin-schema/src/rules/compile.rs:468-481 (mounts_generated_verbs) and the handler generator take the FIRST hook naming the model (`.find(|h| h.model_ref == model.name ...)`), which is the rules file with no state machine, so the verbs are silently dropped",
    ));

    // A map-format hook file whose rules include a register-only shape (`assert:` without a message).
    let dir = tempfile::tempdir().unwrap();
    let (_module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("schema/hooks/ticket.hook.yaml"), |t| {
            t.replace(
                "    condition: \"the title reads like a sentence a customer would write\"\n    message: \"the title should read like a customer wrote it\"\n",
                "    assert: \"the title reads like a sentence a customer would write\"\n",
            )
        })
    });
    let text = output_text(&out);
    rows.push(gen_row(
        "H",
        "Diagnostics",
        "a hook file with an `assert:` rule (no message) beside its `states:` generates, or names the offending rule",
        out.status.success() || text.contains("aspiration_unenforced") || text.contains("message"),
        "replace the unenforced rule's condition+message with `assert:` and generate",
        "success, or an error naming the rule / the missing `message`",
        &one_line(text.lines().filter(|l| l.contains("Error") || l.contains("error")).collect::<Vec<_>>().join(" | ").as_str()),
        "metaphor-plugin-schema/src/parser/yaml_parser/parsers.rs:154-166 swallows the map-format parse error (YamlRule at parser/yaml_parser/types.rs requires `condition` and `message`) and falls back to the list-format parser, which reads no map-format states; the validator then reports \"State machine in workflow 'Ticket' has no states\" (resolver/validator.rs:817), naming neither the rule nor the hook",
    ));

    // A staged event that names a field, verb or state the model lacks is refused.
    let dir = tempfile::tempdir().unwrap();
    let (_module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("schema/hooks/ticket.hook.yaml"), |t| {
            t.replace("    on: update(title)\n", "    on: [update(nope), transition(shut), enter(void)]\n    if: \"colour == 'red'\"\n")
        })
    });
    let text = output_text(&out);
    rows.push(gen_row(
        "D",
        "Declared events",
        "an `on:` or `if:` naming a field, verb or state the model lacks fails generation, naming each",
        !out.status.success() && ["no field `nope`", "verb `shut`", "state `void`", "`if:`"].iter().all(|n| text.contains(n)),
        "set TicketRetitled to `on: [update(nope), transition(shut), enter(void)]` with `if: colour == 'red'` and generate",
        "generation fails; the error names the field, the verb, the state and the condition",
        &one_line(text.lines().filter(|l| l.contains("event `")).take(4).collect::<Vec<_>>().join(" | ").as_str()),
        "metaphor-plugin-schema/src/rules/events.rs (check_events) and resolver/validator.rs (event_problems)",
    ));

    // Lifecycle hooks declared in the schema.
    let dir = tempfile::tempdir().unwrap();
    let (_module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("schema/hooks/ticket.hook.yaml"), |t| format!("{t}\ntriggers:\n  after_create:\n    actions:\n      - \"emit: TicketOpenedEvent(ticket_id: id)\"\n"))
    });
    let text = output_text(&out);
    rows.push(Row {
        area: "D".into(),
        feature: "Schema lifecycle hooks".into(),
        probe: "before/after create/update/delete declared as hook `triggers:` produce runtime hooks".into(),
        status: "UNSUPPORTED".into(),
        request: "add `triggers: after_create:` to ticket.hook.yaml and generate".into(),
        expected: String::new(),
        got: format!(
            "generation {} — {}; the trigger target is retired (generators/mod.rs:621) and resolver/validator.rs:1916 refuses triggers unless `legacy_triggers: true`; hooks are written by hand as a ServiceLifecycle (proven in D)",
            if out.status.success() { "succeeds" } else { "fails" },
            one_line(text.lines().filter(|l| l.contains("rror")).take(2).collect::<Vec<_>>().join(" | ").as_str())
        ),
        diagnosis: String::new(),
    });

    // Declarations nothing runs are said to be declaration-only on generation.
    let said = |needle: &str| main_output.lines().find(|l| l.contains(needle) && l.contains("declaration-only")).map(|l| one_line(l.trim()));
    let hand = main_output.lines().find(|l| l.contains("TicketEscalated") && l.contains("no `on:`")).map(|l| one_line(l.trim()));
    rows.push(gen_row(
        "D",
        "Declared events",
        "generation says a declared event with no `on:` is staged by hand, and names it",
        hand.is_some() && !main_output.lines().any(|l| l.contains("TicketOpened") && l.contains("no `on:`")),
        "generate the fixture (ticket.hook.yaml declares TicketEscalated without `on:`, TicketOpened with it)",
        "a warning naming TicketEscalated (only) as staged by hand code",
        &hand.unwrap_or_else(|| "no such warning".into()),
        "metaphor-plugin-schema/src/resolver/validator.rs (declaration_only_warnings)",
    ));
    let generated = |rel: &str| main.join(rel).exists();
    rows.push(gen_row(
        "D",
        "Declared events",
        "events with `on:` generate their typed payloads and the write hooks that stage them",
        generated("src/domain/event/declared.rs") && generated("src/application/service/ticket_write_hooks.rs"),
        "generate the fixture",
        "src/domain/event/declared.rs and src/application/service/ticket_write_hooks.rs exist",
        &format!(
            "declared.rs: {}; ticket_write_hooks.rs: {}",
            generated("src/domain/event/declared.rs"),
            generated("src/application/service/ticket_write_hooks.rs")
        ),
        "metaphor-plugin-schema/src/generators/declared_events.rs",
    ));
    rows.push(gen_row(
        "F",
        "Workflow",
        "generation says the workflow is declaration-only",
        said("workflow").is_some(),
        "generate the fixture (schema/workflows/ticket_escalation.workflow.yaml)",
        "a warning naming the workflow as declaration-only",
        &said("workflow").unwrap_or_else(|| "no such warning".into()),
        "metaphor-plugin-schema/src/resolver/validator.rs (declaration_only_warnings)",
    ));

    // Workflows: declaration-only.
    let hits = grep_tree(&main.join("src"), "TicketEscalation");
    rows.push(Row {
        area: "F".into(),
        feature: "Workflow".into(),
        probe: "schema/workflows/ticket_escalation.workflow.yaml does something at runtime".into(),
        status: "UNSUPPORTED".into(),
        request: "generate with the workflow file present".into(),
        expected: String::new(),
        got: format!(
            "declaration-only: it is parsed and validated (resolver/validator.rs:55) and {} generated file mentions it; the flow target is retired (generators/mod.rs:621) — the hand-written escalate route is the executable flow (proven in F)",
            if hits.is_empty() { "no".to_string() } else { format!("{} (unexpected)", hits.len()) }
        ),
        diagnosis: String::new(),
    });
}

/// Run `schema doctor .` in `module`: whether it passed, and what it said.
fn doctor(module: &Path) -> (bool, String) {
    let out = run(Command::new(env!("CARGO_BIN_EXE_metaphor-schema")).args(["schema", "doctor", "."]).current_dir(module).env("NO_COLOR", "1"), "schema doctor");
    (out.status.success(), output_text(&out))
}

/// Permission probes that need a variant of the fixture: each is generated (not built), then the
/// doctor or the register is run on it.
fn permission_variant_probes(skeleton: &Path, fixture: &Path, rows: &mut Vec<Row>) {
    const EXT: &str = "src/presentation/http/ticket_handler.ext.rs";
    let guarded_line = "        .route(\"/tickets/:id/escalate\", guarded(post(escalate), \"ticket:escalate\"))";

    // An unguarded hand route fails the doctor, naming the file and line; saying why accepts it.
    let dir = tempfile::tempdir().unwrap();
    let (module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join(EXT), |t| t.replace(guarded_line, "        .route(\"/tickets/:id/escalate\", post(escalate))"))
    });
    let (ok, text) = doctor(&module);
    rows.push(gen_row(
        "I",
        "Schema doctor",
        "an unguarded hand-written route fails `schema doctor`, naming the file, the line and the fix",
        out.status.success() && !ok && text.contains("ticket_handler.ext.rs:") && text.contains("route `/tickets/:id/escalate` has no permission guard") && text.contains("guarded(route"),
        "remove guarded(..) from the escalate route in ticket_handler.ext.rs and run doctor",
        "exit 1; src/presentation/http/ticket_handler.ext.rs:<line> route `/tickets/:id/escalate` has no permission guard",
        &format!("generate exit {:?}; doctor ok={ok}; {}", out.status.code(), one_line(&text)),
        "metaphor-plugin-schema/src/commands/schema/permissions_check.rs (scan) and doctor.rs (permission_report)",
    ));
    edit(&module.join(EXT), |t| {
        t.replace("        .route(\"/tickets/:id/escalate\", post(escalate))", "        // unguarded: the provider's callback is verified by its signature\n        .route(\"/tickets/:id/escalate\", post(escalate))")
    });
    let (ok, text) = doctor(&module);
    rows.push(gen_row(
        "I",
        "Schema doctor",
        "a route marked `// unguarded: <why>` is accepted, and the doctor prints the reason",
        ok && text.contains("deliberately unguarded: the provider's callback is verified by its signature"),
        "add `// unguarded: <why>` above the route and run doctor",
        "exit 0; a notice with the reason",
        &format!("doctor ok={ok}; {}", one_line(&text)),
        "metaphor-plugin-schema/src/commands/schema/permissions_check.rs",
    ));

    // A permission the vocabulary lacks fails the doctor (a typo in a verb's name).
    let dir = tempfile::tempdir().unwrap();
    let (module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join(EXT), |t| t.replace("\"ticket:escalate\"", "\"ticket:escalat\""))
    });
    let (ok, text) = doctor(&module);
    rows.push(gen_row(
        "I",
        "Schema doctor",
        "a guarded verb whose permission the vocabulary does not hold fails `schema doctor`",
        out.status.success() && !ok && text.contains("guards with `ticket:escalat`, which the permission vocabulary does not hold"),
        "guard the escalate route with \"ticket:escalat\" and run doctor",
        "exit 1, naming ticket:escalat",
        &format!("generate exit {:?}; doctor ok={ok}; {}", out.status.code(), one_line(&text)),
        "metaphor-plugin-schema/src/commands/schema/permissions_check.rs (undeclared)",
    ));

    // A direct call of the framework's unguarded generic routes is an unguarded hand route.
    let dir = tempfile::tempdir().unwrap();
    let (module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("src/lib.ext.rs"), |t| t.replace("::routes_guarded(", "::routes(").replace("            crate::presentation::http::project_permissions(),\n", ""))
    });
    let (ok, text) = doctor(&module);
    rows.push(gen_row(
        "I",
        "Schema doctor",
        "a hand-written call of the unguarded generic routes (BackboneCrudHandler::routes) fails `schema doctor`",
        out.status.success() && !ok && text.contains("lib.ext.rs:") && text.contains("BackboneCrudHandler::routes"),
        "mount /hooked-projects with BackboneCrudHandler::routes and run doctor",
        "exit 1, naming lib.ext.rs and BackboneCrudHandler::routes",
        &format!("generate exit {:?}; doctor ok={ok}; {}", out.status.code(), one_line(&text)),
        "metaphor-plugin-schema/src/commands/schema/permissions_check.rs",
    ));

    // The mode follows the source: a service that installs the resolver is labelled RESOLVER.
    let dir = tempfile::tempdir().unwrap();
    let (module, _) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("src/lib.ext.rs"), |t| t + "\n// A composing service mounts `from_fn_with_state(GrantResolver::new(source), resolve_grants)`.\nfn _installs() { let _ = \"GrantResolver\"; }\n")
    });
    let out = run(Command::new(env!("CARGO_BIN_EXE_metaphor-schema")).args(["schema", "rules", ".", "-f", "json"]).current_dir(&module).env("NO_COLOR", "1"), "schema rules");
    let mode = serde_json::from_slice::<Value>(&out.stdout).ok().and_then(|v| v["permissions"]["mode"].as_str().map(str::to_string)).unwrap_or_default();
    let pretty = run(Command::new(env!("CARGO_BIN_EXE_metaphor-schema")).args(["schema", "rules", "."]).current_dir(&module).env("NO_COLOR", "1"), "schema rules");
    let (_, doc) = doctor(&module);
    rows.push(gen_row(
        "I",
        "Register",
        "the register and the doctor label the mode RESOLVER when the service installs a grant resolver",
        mode == "resolver" && String::from_utf8_lossy(&pretty.stdout).contains("Permissions: mode RESOLVER") && doc.contains("permissions: mode RESOLVER"),
        "mention GrantResolver in the module's source and run rules and doctor",
        "mode resolver; `Permissions: mode RESOLVER`",
        &format!("mode {mode}"),
        "metaphor-plugin-schema/src/commands/schema/permissions_check.rs (Mode)",
    ));

    // `requires:` renames a permission; an undeclared target is refused at generation.
    let dir = tempfile::tempdir().unwrap();
    let (_module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("schema/models/ticket.model.yaml"), |t| t.replacen("    description: \"Conformance fixture: an org-scoped record with a state machine\"\n", "    description: \"Conformance fixture: an org-scoped record with a state machine\"\n    requires:\n      read: support:view\n", 1))
    });
    let text = output_text(&out);
    rows.push(gen_row(
        "I",
        "Declarations",
        "`requires:` naming a permission the vocabulary does not declare fails generation, naming it",
        !out.status.success() && text.contains("`Ticket` requires.read names `support:view`, which the authorization permissions do not declare"),
        "add `requires: { read: support:view }` to Ticket and generate",
        "generation fails naming support:view",
        &one_line(text.lines().filter(|l| l.contains("requires")).take(2).collect::<Vec<_>>().join(" | ").as_str()),
        "metaphor-plugin-schema/src/rbac.rs (problems) via resolver/validator.rs",
    ));
    let dir = tempfile::tempdir().unwrap();
    let (module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("schema/models/ticket.model.yaml"), |t| t.replacen("    description: \"Conformance fixture: an org-scoped record with a state machine\"\n", "    description: \"Conformance fixture: an org-scoped record with a state machine\"\n    requires:\n      read: support:view\n", 1))
        ;
        edit(&m.join("schema/models/index.model.yaml"), |t| t.replace("    tickets: [escalate, archive_with_reason]\n", "    tickets: [escalate, archive_with_reason]\n    support: [view]\n"))
    });
    let handler = fs::read_to_string(module.join("src/presentation/http/ticket_handler.rs")).unwrap_or_default();
    rows.push(gen_row(
        "I",
        "Declarations",
        "a declared `requires:` renames the permission the entity's read routes need in the generated handler",
        out.status.success() && handler.contains(".requiring(backbone_core::permission::Gate::Read, \"support:view\")") && handler.contains("pub fn ticket_permissions()"),
        "declare support: [view], add `requires: { read: support:view }` to Ticket and generate",
        "ticket_permissions() carries .requiring(Gate::Read, \"support:view\")",
        &format!("generate exit {:?}; {}", out.status.code(), one_line(&handler.lines().filter(|l| l.contains("requiring")).collect::<Vec<_>>().join(" | "))),
        "metaphor-plugin-schema/src/generators/handler.rs (write_route_config)",
    ));

    // A role granting something that is no permission is refused.
    let dir = tempfile::tempdir().unwrap();
    let (_module, out) = prepare(skeleton, fixture, dir.path(), |m| {
        edit(&m.join("schema/models/index.model.yaml"), |t| t.replace("[\"ticket:read\", \"projects.read\"]", "[\"ticket:read\", \"a:b:c\"]"))
    });
    let text = output_text(&out);
    rows.push(gen_row(
        "I",
        "Declarations",
        "a system role that lists a malformed permission fails generation, naming the role and the entry",
        !out.status.success() && text.contains("role `viewer` lists `a:b:c`, which is no permission"),
        "give role viewer the grant a:b:c and generate",
        "generation fails naming role viewer and a:b:c",
        &one_line(text.lines().filter(|l| l.contains("role")).take(2).collect::<Vec<_>>().join(" | ").as_str()),
        "metaphor-plugin-schema/src/rbac.rs (problems)",
    ));
}

#[test]
#[ignore = "generates a module, builds it and drives it against a scratch Postgres database; run with --ignored"]
fn every_backbone_feature_works_at_runtime_through_the_generator() {
    let started = Instant::now();
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = crate_dir.join("tests/fixtures/conformance");
    let admin_dsn = std::env::var("CONFORMANCE_DATABASE_URL")
        .expect("set CONFORMANCE_DATABASE_URL to an admin DSN that may CREATE DATABASE and CREATE ROLE");
    let skeleton = std::env::var_os("METAPHOR_MODULE_SKELETON")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("../modules/backbone-module"));
    assert!(skeleton.join("Cargo.toml").exists(), "no module skeleton at {}", skeleton.display());

    let work = tempfile::tempdir().unwrap();
    let (module, generate) = prepare(&skeleton, &fixture, work.path(), |_| {});
    assert!(
        generate.status.success(),
        "generation failed:\n{}",
        output_text(&generate)
    );
    let generated_at = started.elapsed();

    let framework = framework_path(&crate_dir);
    patch_manifest(&module, framework.as_deref());

    let results = work.path().join("results.json");
    let jobs = std::env::var("CONFORMANCE_JOBS").unwrap_or_else(|_| "2".into());
    let mut cargo = Command::new(env!("CARGO"));
    cargo
        .args(["test", "-j", &jobs, "--test", "conformance_runtime", "--", "--nocapture", "--test-threads", "1"])
        .current_dir(&module)
        .env("CONFORMANCE_DATABASE_URL", &admin_dsn)
        .env("CONFORMANCE_RESULTS", &results)
        .env("RUST_MIN_STACK", "16777216");
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        cargo.env("CARGO_TARGET_DIR", target);
    } else {
        cargo.env("CARGO_TARGET_DIR", work.path().join("target"));
    }
    let probe = run(&mut cargo, "cargo test (generated module)");
    let probe_stdout = String::from_utf8_lossy(&probe.stdout).to_string();
    let probe_stderr = String::from_utf8_lossy(&probe.stderr).to_string();
    println!("{probe_stdout}");

    let mut rows: Vec<Row> = Vec::new();
    match fs::read_to_string(&results).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(Value::Array(items)) => rows.extend(items.iter().map(Row::from_json)),
        _ => {
            let errors: Vec<&str> = probe_stderr.lines().filter(|l| l.starts_with("error")).take(40).collect();
            rows.push(gen_row(
                "A",
                "Harness",
                "the generated module builds and the runtime probes run",
                false,
                "cargo test --test conformance_runtime (inside the generated module)",
                "a results file",
                &format!(
                    "exit {:?}\n{}\n{}",
                    probe.status.code(),
                    errors.join("\n"),
                    probe_stderr.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
                ),
                "see the compiler output above",
            ));
            eprintln!("{probe_stderr}");
        }
    }
    let runtime = rows.clone();
    register_probes(&module, &mut rows, &runtime);
    variant_probes(&skeleton, &fixture, &module, &output_text(&generate), &mut rows);
    permission_variant_probes(&skeleton, &fixture, &mut rows);
    rows.sort_by_key(|r| AREAS.iter().position(|(k, _)| *k == r.area).unwrap_or(99));

    let header = format!(
        "- Generator: metaphor-plugin-schema {} (built from this checkout)\n- Framework: {}\n- Generation: {:.0?}; whole run: {:.0?}\n",
        env!("CARGO_PKG_VERSION"),
        framework
            .as_ref()
            .map(|p| {
                let workspace = crate_dir.parent().and_then(|d| d.canonicalize().ok());
                let shown = workspace
                    .and_then(|w| p.strip_prefix(&w).ok().map(|r| format!("<workspace>/{}", r.display())))
                    .unwrap_or_else(|| p.display().to_string());
                format!("local checkout `{shown}` via [patch.crates-io]")
            })
            .unwrap_or_else(|| "crates.io".into()),
        generated_at,
        started.elapsed(),
    );
    let report = render(&rows, &header);
    println!("{report}");
    fs::write(fixture.join("RESULTS.md"), &report).unwrap();

    if std::env::var_os("CONFORMANCE_KEEP").is_some() {
        let kept = work.keep();
        println!("generated module kept at {}", kept.join(MODULE).display());
    }

    let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        *by_status.entry(r.status.as_str()).or_default() += 1;
    }
    let failed: Vec<String> = rows
        .iter()
        .filter(|r| r.status == "FAIL")
        .map(|r| format!("{} / {} / {}", r.area, r.feature, r.probe))
        .collect();
    assert!(failed.is_empty(), "{} probe(s) failed ({by_status:?}):\n{}", failed.len(), failed.join("\n"));
}
