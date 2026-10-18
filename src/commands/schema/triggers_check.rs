//! `schema doctor` for hook triggers: what hand code owes the generated trait, and what it must
//! not do inside a write's transaction.
//!
//! The generated trait `<Model>Triggers` has no default bodies, so the compiler refuses a build
//! whose implementation lacks a method. The doctor says it first, without a build: the extension
//! file that should hold the implementation (`src/application/triggers/<model>_triggers.ext.rs`)
//! must exist and define every required method. It also warns on an implementation that calls a
//! mail, HTTP or socket client directly: such a call holds the transaction (and the row's locks)
//! open for as long as it takes and is not undone by a rollback. An effect outside the database is
//! staged as an event, `NotificationRequested` for mail, and the outbox relay performs it.

use std::path::Path;

use crate::ast::ModuleSchema;
use crate::utils::to_snake_case;

/// What the doctor found about a module's triggers.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TriggerReport {
    /// Models with triggers, entries and compiled steps, hand methods: the summary line.
    pub models: usize,
    pub entries: usize,
    pub hand_methods: usize,
    /// Lines that fail the doctor: a missing extension file or method.
    pub failures: Vec<String>,
    /// Lines that warn: a client called inside the transaction.
    pub warnings: Vec<String>,
}

/// Clients that perform an effect outside the database. A mention in a triggers extension is a
/// call made while the write's transaction is open.
pub(super) const CLIENTS: &[&str] = &[
    "reqwest",
    "lettre",
    "ureq",
    "isahc",
    "surf::",
    "hyper::Client",
    "hyper_util::client",
    "TcpStream",
    "UdpSocket",
    "tokio::process",
    "std::process::Command",
];

/// Check `root`'s triggers extension files against what `schema` requires.
pub fn scan(schema: &ModuleSchema, root: &Path) -> TriggerReport {
    let mut report = TriggerReport::default();
    for model in &schema.models {
        let triggers = crate::generators::triggers::model_triggers(schema, model);
        if triggers.is_empty() {
            continue;
        }
        report.models += 1;
        report.entries += triggers.len();
        let methods = crate::generators::triggers::hand_methods(schema, model);
        report.hand_methods += methods.len();
        let rel = format!(
            "src/application/triggers/{}_triggers.ext.rs",
            to_snake_case(&model.name)
        );
        let path = root.join(&rel);
        let text = std::fs::read_to_string(&path).ok();
        if !methods.is_empty() {
            match &text {
                None => report.failures.push(format!(
                    "{rel}: missing. {} needs {} hand method(s) of `{}Triggers` ({}{}); the generated file lists their signatures",
                    model.name,
                    methods.len(),
                    model.name,
                    methods.iter().take(3).map(|m| format!("`{}`", m.name)).collect::<Vec<_>>().join(", "),
                    if methods.len() > 3 { ", ..." } else { "" }
                )),
                Some(text) => {
                    for m in &methods {
                        let declared = regex::Regex::new(&format!(r"\basync\s+fn\s+{}\b", regex::escape(&m.name))).expect("an escaped name");
                        if !declared.is_match(text) {
                            report.failures.push(format!(
                                "{rel}: no method `{}` ({} of {}, `{}`): the build would refuse the implementation of `{}Triggers`",
                                m.name, m.key, m.source, m.text, model.name
                            ));
                        }
                    }
                    if !text.contains(&format!("{}TriggersImpl", model.name)) {
                        report.failures.push(format!("{rel}: defines no `{}TriggersImpl`, the type the generated service builder attaches", model.name));
                    }
                }
            }
        }
        if let Some(text) = &text {
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if let Some(client) = CLIENTS.iter().find(|c| code.contains(**c)) {
                    report.warnings.push(format!(
                        "{rel}:{} calls `{client}` inside a write's transaction: an effect outside the database is not undone by a rollback and holds the row's locks while it runs; stage a `NotificationRequested` or an event and let the relay perform it",
                        n + 1
                    ));
                }
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Field, Model, PrimitiveType, TypeRef};

    fn schema() -> ModuleSchema {
        let mut s = ModuleSchema::new("shop");
        let mut ticket = Model::new("Ticket");
        for name in ["id", "status"] {
            ticket
                .fields
                .push(Field::new(name, TypeRef::Primitive(PrimitiveType::String)));
        }
        s.models.push(ticket);
        s.rule_register.read_file(
            "ticket.hook.yaml",
            "name: Ticket\nmodel: Ticket\ntriggers:\n  after_create:\n    actions: [\"trigger: refresh\", \"trigger: again\", \"log(made)\"]\n",
        );
        let models = s.models.clone();
        s.rule_register.bind(&models);
        s.rule_register.check(&models);
        s.rule_register.mark_compiled(&models, &Default::default());
        s
    }

    fn root(ext: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(text) = ext {
            let path = dir
                .path()
                .join("src/application/triggers/ticket_triggers.ext.rs");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn a_missing_extension_file_and_a_missing_method_fail() {
        let s = schema();
        let r = scan(&s, root(None).path());
        assert_eq!((r.models, r.entries, r.hand_methods), (1, 1, 2));
        assert_eq!(r.failures.len(), 1, "{:?}", r.failures);
        assert!(
            r.failures[0].contains("ticket_triggers.ext.rs: missing")
                && r.failures[0].contains("after_create__refresh"),
            "{:?}",
            r.failures
        );

        let partial = "pub struct TicketTriggersImpl;\nimpl TicketTriggers for TicketTriggersImpl {\n    async fn after_create__refresh(&self, ctx: &WriteCtx, e: &Ticket) -> ServiceResult<()> { Ok(()) }\n}\n";
        let r = scan(&s, root(Some(partial)).path());
        assert_eq!(r.failures.len(), 1, "{:?}", r.failures);
        assert!(
            r.failures[0].contains("no method `after_create__again`"),
            "{:?}",
            r.failures
        );

        let whole = "pub struct TicketTriggersImpl;\nasync fn after_create__refresh() {}\nasync fn after_create__again() {}\n";
        let r = scan(&s, root(Some(whole)).path());
        assert!(r.failures.is_empty(), "{:?}", r.failures);
        let nameless = "async fn after_create__refresh() {}\nasync fn after_create__again() {}\n";
        assert!(scan(&s, root(Some(nameless)).path()).failures[0]
            .contains("defines no `TicketTriggersImpl`"));
    }

    #[test]
    fn a_client_called_inside_the_transaction_is_a_warning() {
        let s = schema();
        let text = "pub struct TicketTriggersImpl;\nasync fn after_create__refresh() { let c = reqwest::Client::new(); }\nasync fn after_create__again() {} // reqwest in a comment\n";
        let r = scan(&s, root(Some(text)).path());
        assert!(r.failures.is_empty());
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert!(
            r.warnings[0].contains("ticket_triggers.ext.rs:2")
                && r.warnings[0].contains("NotificationRequested"),
            "{:?}",
            r.warnings
        );
    }
}
