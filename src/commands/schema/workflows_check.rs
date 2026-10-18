//! `schema doctor` for workflows: what hand code owes the generated `Actions` trait of a workflow
//! the engine runs, the function a documented saga says carries it out, and where to look when an
//! instance is stuck.
//!
//! The generated trait `<Name>Actions` has no default bodies, so the compiler refuses a build whose
//! implementation lacks a method. The doctor says it first, without a build: the extension file
//! (`src/application/workflows/<name>_workflow.ext.rs`) must exist and define every required
//! method, and `<Name>ActionsImpl`. A workflow with `implemented_by: "<file>::<fn>"` documents a hand
//! service: the function must exist, so the declaration cannot outlive the code it names.

use std::path::Path;

use crate::ast::ModuleSchema;
use crate::generators::workflows::workflow_schema;
use crate::rules::workflows::{self as wf, function_method};
use crate::utils::{to_pascal_case, to_snake_case};

/// What the doctor found about a module's workflows.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorkflowReport {
    /// Workflows the engine runs.
    pub engine: usize,
    /// Workflows documenting a hand function (`implemented_by`).
    pub documents: usize,
    /// Workflows that are documentation only (no `executor: engine`, no `implemented_by`).
    pub documentation_only: usize,
    /// Required methods across the engine workflows.
    pub hand_methods: usize,
    /// Lines that fail the doctor.
    pub failures: Vec<String>,
    /// Lines that warn.
    pub warnings: Vec<String>,
    /// Pointers for an operator: how to find a stuck instance, a dead event.
    pub notes: Vec<String>,
}

/// Check `root`'s workflow sources against what `schema` declares.
pub fn scan(schema: &ModuleSchema, root: &Path) -> WorkflowReport {
    let mut report = WorkflowReport::default();
    for w in &schema.workflows {
        if w.runs_on_engine() {
            report.engine += 1;
            let compiled = wf::compile(schema, w);
            if !compiled.problems.is_empty() {
                // Validation reports each of these when the schema loads; the doctor says it too.
                report.failures.extend(compiled.problems.iter().cloned());
                continue;
            }
            report.hand_methods += compiled.required_methods();
            let name = to_pascal_case(&w.name);
            let rel = format!(
                "src/application/workflows/{}_workflow.ext.rs",
                to_snake_case(&w.name)
            );
            let text = std::fs::read_to_string(root.join(&rel)).ok();
            let owed: Vec<(String, String)> = compiled
                .hand
                .iter()
                .map(|h| {
                    (
                        h.method.clone(),
                        format!("the verb `{}` (step {})", h.verb, h.steps.join(", ")),
                    )
                })
                .chain(
                    compiled
                        .functions
                        .iter()
                        .map(|f| (function_method(f), format!("the function `{f}`"))),
                )
                .collect();
            if !owed.is_empty() {
                match &text {
                    None => report.failures.push(format!(
                        "{rel}: missing. {name} needs {} hand method(s) of `{name}Actions` ({}{}); the generated file lists their signatures",
                        owed.len(),
                        owed.iter().take(3).map(|(m, _)| format!("`{m}`")).collect::<Vec<_>>().join(", "),
                        if owed.len() > 3 { ", ..." } else { "" }
                    )),
                    Some(text) => {
                        for (method, what) in &owed {
                            let declared = regex::Regex::new(&format!(r"\basync\s+fn\s+{}\b", regex::escape(method))).expect("an escaped name");
                            if !declared.is_match(text) {
                                report.failures.push(format!(
                                    "{rel}: no method `{method}` ({what}): the build would refuse the implementation of `{name}Actions`"
                                ));
                            }
                        }
                        if !text.contains(&format!("{name}ActionsImpl")) {
                            report.failures.push(format!("{rel}: defines no `{name}ActionsImpl`, the type the module's build attaches"));
                        }
                    }
                }
            }
            if let Some(text) = &text {
                for (n, line) in text.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    if let Some(client) = super::triggers_check::CLIENTS
                        .iter()
                        .find(|c| code.contains(**c))
                    {
                        report.warnings.push(format!(
                            "{rel}:{} calls `{client}` inside a step's transaction: an effect outside the database is not undone by a rollback and holds the row's locks while it runs; stage a `NotificationRequested` or an event with `send_email`, `notify` or `emit_event` and let the relay perform it",
                            n + 1
                        ));
                    }
                }
            }
            let pg = workflow_schema(schema);
            let stuck_after = compiled
                .spec
                .get("timeout")
                .and_then(|t| t.get("secs"))
                .and_then(|s| s.as_i64())
                .map(|s| s * 2)
                .unwrap_or(7200);
            report.notes.push(format!(
                "{}: a stuck instance is one `running` or `compensating` for more than twice its timeout ({stuck_after}s): SELECT id, status, current_step, last_error, updated_at FROM {pg}.bb_workflow_instances WHERE workflow = '{}' AND status IN ('running', 'compensating') AND updated_at < now() - interval '{stuck_after} seconds'; its step log is {pg}.bb_workflow_steps",
                w.name,
                to_pascal_case(&w.name)
            ));
        } else if let Some(claim) = &w.implemented_by {
            report.documents += 1;
            if !crate::rules::claim_holds(root, claim) {
                report.failures.push(format!(
                    "workflow '{}' says it is implemented by `{claim}`, but no such function exists; write it as `<file under the module>::<fn>`",
                    w.name
                ));
            }
        } else {
            report.documentation_only += 1;
        }
    }
    if report.engine > 0 {
        let pg = workflow_schema(schema);
        report.notes.push(format!(
            "events: SELECT count(*) FROM {pg}.outbox_events WHERE failed_at IS NOT NULL is the number of effects the relay gave up on (`relay::reemit` revives them); a step or a resume that did not happen is one of them, or an event the engine never received (the composing service wires `background_tasks().deliver` into its relay and runs `workflow_timer_sweep` every minute)"
        ));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::workflows::test_support::{schema_with, BASE};
    use std::fs;

    fn engine_schema() -> ModuleSchema {
        let mut s = schema_with(BASE);
        s.generators_config = Some(crate::parser::yaml_parser::GeneratorsConfig {
            extensions: Some(true),
            ..Default::default()
        });
        s
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_doctor_names_a_missing_extension_file_then_a_missing_method_then_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = engine_schema();
        let r = scan(&s, &dir);
        assert_eq!((r.engine, r.hand_methods), (1, 1));
        assert!(
            r.failures
                .iter()
                .any(|f| f.contains("escalation_workflow.ext.rs: missing")
                    && f.contains("notify_on_call")),
            "{:?}",
            r.failures
        );

        write(
            &dir,
            "src/application/workflows/escalation_workflow.ext.rs",
            "pub struct EscalationActionsImpl;\n",
        );
        let r = scan(&s, &dir);
        assert!(
            r.failures.iter().any(
                |f| f.contains("no method `notify_on_call`") && f.contains("EscalationActions")
            ),
            "{:?}",
            r.failures
        );

        write(
            &dir,
            "src/application/workflows/escalation_workflow.ext.rs",
            "#[derive(Default)]\npub struct EscalationActionsImpl;\n#[async_trait]\nimpl EscalationActions for EscalationActionsImpl {\n    async fn notify_on_call(&self, ctx: &WriteCtx, params: &Value, wf: &Value) -> ServiceResult<Value> { Ok(Value::Null) }\n}\n",
        );
        let r = scan(&s, &dir);
        assert!(r.failures.is_empty(), "{:?}", r.failures);
        assert!(
            r.notes
                .iter()
                .any(|n| n.contains("bb_workflow_instances") && n.contains("'Escalation'")),
            "{:?}",
            r.notes
        );
    }

    #[test]
    fn a_client_called_inside_a_step_is_warned_about() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        write(
            &dir,
            "src/application/workflows/escalation_workflow.ext.rs",
            "pub struct EscalationActionsImpl;\nasync fn notify_on_call() { let c = reqwest::Client::new(); }\n",
        );
        let r = scan(&engine_schema(), &dir);
        assert!(
            r.warnings
                .iter()
                .any(|w| w.contains("reqwest") && w.contains("step's transaction")),
            "{:?}",
            r.warnings
        );
    }

    #[test]
    fn a_documented_saga_must_name_a_function_that_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let mut s = schema_with(&BASE.replace("executor: engine", "executor: hand"));
        s.workflows[0].implemented_by =
            Some("src/application/service/posting_service.rs::post".into());
        let r = scan(&s, &dir);
        assert_eq!((r.documents, r.documentation_only), (1, 0));
        assert!(
            r.failures
                .iter()
                .any(|f| f.contains("implemented by") && f.contains("posting_service.rs::post")),
            "{:?}",
            r.failures
        );
        write(
            &dir,
            "src/application/service/posting_service.rs",
            "impl X {\n    pub async fn post(&self) {}\n}\n",
        );
        assert!(scan(&s, &dir).failures.is_empty());
        s.workflows[0].implemented_by = None;
        let r = scan(&s, &dir);
        assert_eq!((r.documents, r.documentation_only), (0, 1));
    }
}
