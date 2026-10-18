# Conformance results

Written by `tests/conformance.rs`; do not edit by hand. Rerun with
`CONFORMANCE_DATABASE_URL=... cargo test --test conformance -- --ignored --nocapture`.

- Generator: metaphor-plugin-schema 0.39.0 (built from this checkout)
- Framework: local checkout `<workspace>/modules/backbone-framework` via [patch.crates-io]
- Generation: 2s; whole run: 95s

## Summary

| Area | PASS | FAIL | UNSUPPORTED |
|---|---|---|---|
| A. Generic routes | 123 | 0 | 0 |
| B. Validation | 42 | 0 | 0 |
| C. Custom validation | 15 | 0 | 0 |
| D. Hooks and events | 79 | 0 | 0 |
| E. State machine | 20 | 0 | 0 |
| F. Extension | 66 | 0 | 0 |
| G. Scoping | 36 | 0 | 0 |
| H. Debuggability | 35 | 0 | 0 |
| I. Permissions | 142 | 0 | 0 |
| **Total** | **558** | **0** | **0** |

## Matrix

| Area | Feature | Probe | Result |
|---|---|---|---|
| A | Audit trail | the composed backbone-auditlog migrations apply before the module's own | PASS |
| A | Migrations | every generated up-migration applies to an empty database | PASS |
| A | Create | POST /projects answers 201 with the stored record | PASS |
| A | Get | GET /projects/:id answers 200 with the record | PASS |
| A | Get | GET an unknown id answers 404 | PASS |
| A | Get | GET a malformed id answers 400 invalid_id on path id | PASS |
| A | List | GET /projects lists with data[] and meta.total | PASS |
| A | List filters | operator filter priority[gte]=4 | PASS |
| A | List filters | equality filter kind=client (an enum column) | PASS |
| A | List filters | priority[in]=1,2 | PASS |
| A | List filters | priority[between]=2,4 | PASS |
| A | List filters | website[isnotnull] | PASS |
| A | List filters | two filters AND together: priority[ne]=3&billable=true | PASS |
| A | List filters | OR filter priority=1&priority[or]=5 | PASS |
| A | List filters | a filter on an unknown column answers 400, not 500 | PASS |
| A | List filters | a filter on an unknown column names it | PASS |
| A | List filters | an operator filter on an unknown column names the whole key | PASS |
| A | List filters | an unknown operator names the whole key | PASS |
| A | List filters | a key that is not a field name names itself | PASS |
| A | List filters | an integer filter given text names the parameter | PASS |
| A | List filters | a list filter with one bad element names the parameter | PASS |
| A | List filters | an enum filter given a non-member names the parameter | PASS |
| A | List filters | a date filter given text names the parameter | PASS |
| A | List filters | a pattern operator on a number names the parameter | PASS |
| A | List sort | sort_by naming a missing column names sort_by | PASS |
| A | List sort | sort_by with an unusable name names sort_by | PASS |
| A | List sort | orderby naming a missing column names orderby | PASS |
| A | List sort | sort naming a missing column names sort | PASS |
| A | List sort | the bracket sort names its whole key | PASS |
| A | List sort | sort_order that is not asc or desc names sort_order | PASS |
| A | List paging | a page that is not a number names page | PASS |
| A | List paging | a negative limit names limit | PASS |
| A | List paging | an offset that is not a number names offset | PASS |
| A | List paging | a page past the offset ceiling names page | PASS |
| A | List keyset | an after cursor nobody issued names after | PASS |
| A | List keyset | a before cursor nobody issued names before | PASS |
| A | List fields | a fields entry that is not a field name names fields | PASS |
| A | List include | an include that is not a relation names include | PASS |
| A | List include | a with that is not a relation names with | PASS |
| A | Trash | the trash list names a missing sort column | PASS |
| A | Trash | the trash list names a bad filter value | PASS |
| A | Trash | the trash list names a bad page | PASS |
| A | Count | the count names a bad filter value | PASS |
| A | Count | the count names a missing column | PASS |
| A | Aggregate | group_by naming a missing column names group_by | PASS |
| A | Aggregate | sum of a missing column names sum | PASS |
| A | Aggregate | avg of a missing column names avg | PASS |
| A | Aggregate | sum of a text column names sum | PASS |
| A | Aggregate | an aggregate filter given text names the parameter | PASS |
| A | List include | an include that is not a relation of a tickets list names include | PASS |
| A | Get | an include that is not a relation names include on a single record too | PASS |
| A | List filters | well-formed parameters are still served: sort_order in either case | PASS |
| A | List filters | well-formed parameters are still served: an include of a real relation | PASS |
| A | List filters | well-formed parameters are still served: an empty sort value asks for nothing | PASS |
| A | List sort | sort=-priority orders descending | PASS |
| A | List sort | orderby=priority:desc orders descending | PASS |
| A | List sort | the typed sort_by/sort_order parameters order the list | PASS |
| A | List search | search=Alpha matches the name | PASS |
| A | List paging | offset paging limit=2&page=2 gives the 3rd and 4th rows | PASS |
| A | List paging | an oversized limit is clamped to the page cap (100) | PASS |
| A | List paging | page=0 answers 200 or 400, never 5xx | PASS |
| A | List keyset | a sorted page hands out meta.next_cursor | PASS |
| A | List keyset | after=<cursor> continues with the next rows | PASS |
| A | List count modes | estimate=1 counts exactly on a small table | PASS |
| A | List fields | fields=code,name keeps only id, code, name | PASS |
| A | List include | include=project hydrates the referenced project | PASS |
| A | Update | PUT /projects/:id replaces the record | PASS |
| A | Patch | PATCH writes only the columns it changes (summary is not in the SET list) | PASS |
| A | Patch | an absent key keeps its stored value | PASS |
| A | Patch | an explicit null clears a nullable column | PASS |
| A | Soft delete | DELETE /projects/:id soft-deletes (200) | PASS |
| A | Soft delete | a soft-deleted record is gone from GET (404) | PASS |
| A | Soft delete | the row stays with metadata.deleted_at set | PASS |
| A | Trash | GET /projects/trash lists it | PASS |
| A | Trash | the trash list takes filters | PASS |
| A | Trash | GET /projects/trash/count counts it | PASS |
| A | Trash | GET /projects/:id/deleted reads the trashed record | PASS |
| A | Restore | POST /projects/:id/restore brings it back | PASS |
| A | Restore | POST /projects/restore/all restores the whole trash | PASS |
| A | Empty trash | DELETE /projects/empty purges the trash | PASS |
| A | Permanent delete | DELETE /projects/trash/:id removes it for good (204) | PASS |
| A | Bulk | POST /projects/bulk creates every item (201) | PASS |
| A | Bulk | PATCH /projects/bulk patches every id | PASS |
| A | Bulk | PUT /projects/bulk replaces every item | PASS |
| A | Bulk | POST /projects/delete/bulk soft-deletes every id | PASS |
| A | Bulk | POST /projects/restore/bulk restores every id | PASS |
| A | Bulk | DELETE /projects/trash/bulk removes every id for good | PASS |
| A | Bulk | a bulk create with one invalid item refuses the batch with the item index in the path | PASS |
| A | Count | GET /projects/count honours filters | PASS |
| A | Aggregate | GET /projects/aggregate?group_by=kind&sum=budget groups and sums | PASS |
| A | History | GET /projects/:id/history on a model that is not audited answers 501 (no history provider) | PASS |
| A | Upsert | POST /projects/upsert answers 501 | PASS |
| A | Body errors | POST with an unknown key answers 422 unknown_field on that path | PASS |
| A | Body errors | PATCH with an unknown key answers 422 unknown_field | PASS |
| A | Body errors | a malformed JSON body answers 400 malformed_body | PASS |
| A | Body errors | POST with a wrong type answers 422 with the field's path | PASS |
| A | Body errors | PATCH with a wrong type answers 422 with the field's path | PASS |
| A | Audit trail | creating an audited row answers 201 | PASS |
| A | Audit trail | the create records an insert row with the full image, the actor and the request's correlation id | PASS |
| A | Audit trail | the trail row inherits the audited row's org unit | PASS |
| A | Audit trail | the update records an update row naming only the fields that changed, with its own correlation id | PASS |
| A | History | GET /ledgers/:id/history with the auditlog provider lists the trail newest first with actor and correlation | PASS |
| A | History | a table without the capture trigger answers audited false, not an empty timeline | PASS |
| A | History | the nested history endpoint lists the trail newest first, numbered from the oldest | PASS |
| A | History | as_of between the create and the update reconstructs the row's earlier state | PASS |
| A | History | as_of after the update reconstructs the later state | PASS |
| A | History | as_of before the first captured event answers 404 history_before_subject | PASS |
| A | Audit trail | another company's token reads nothing from the nested history of a ledger it cannot see | PASS |
| A | Audit trail | another company's token cannot reconstruct the row either | PASS |
| A | Audit trail | another company's token reads nothing through the provider route | PASS |
| A | Audit trail | the nested history route asks for ledger:read | PASS |
| A | Audit trail | a create the write guard refuses answers 422 over_limit | PASS |
| A | Audit trail | an update the write guard refuses answers 422 over_limit | PASS |
| A | Audit trail | an update the field rule refuses answers 422 | PASS |
| A | Audit trail | an update the permission guard refuses answers 403 | PASS |
| A | Audit trail | an update from another company is refused by the fence | PASS |
| A | Audit trail | none of the refused writes left a trail row | PASS |
| A | Audit trail | a write that rolls back takes its trail row with it (same-transaction truth) | PASS |
| A | Audit trail | the trail is append-only, even for the owner | PASS |
| A | Audit trail | a soft delete records an update row and a purge records a delete row with the full image | PASS |
| A | History | a purged subject still resolves its history, newest entry a delete | PASS |
| A | History | as_of after the purge answers 404 subject_deleted_before_as_of | PASS |
| A | History | as_of an earlier instant still reconstructs the row after it was purged | PASS |
| B | Field attributes | @required refuses a blank name | PASS |
| B | Field attributes | @length(min=3) refuses a short name | PASS |
| B | Field attributes | @length(max=12) refuses a long code | PASS |
| B | Field attributes | @pattern refuses a lower-case code | PASS |
| B | Field attributes | @max(500) on text refuses a long summary | PASS |
| B | Field attributes | the email type refuses a malformed address | PASS |
| B | Field attributes | the url type refuses a malformed url | PASS |
| B | Field attributes | the @email attribute on a string field refuses a malformed address | PASS |
| B | Field attributes | the @url attribute on a string field refuses a malformed url | PASS |
| B | Field attributes | @non_negative refuses a negative budget | PASS |
| B | Field attributes | @positive refuses a zero fee | PASS |
| B | Field attributes | @range(1,5) refuses priority 9 | PASS |
| B | Field attributes | @min(1) refuses headcount 0 (int64) | PASS |
| B | Field attributes | @max(100000) refuses headcount 200000 (int64) | PASS |
| B | Field attributes | a missing required key answers 422 required on its path | PASS |
| B | Field attributes | every broken rule is reported at once | PASS |
| B | Defaults | @default fills omitted fields (literal int/bool/enum/decimal, uuid id) | PASS |
| B | Defaults | @default(now) and audit metadata stamp created_at | PASS |
| B | Defaults | @default(uuid) on a non-id field fills it | PASS |
| B | Defaults | a state field is born at its initial state | PASS |
| B | Immutable | an @immutable field is accepted on create | PASS |
| B | Immutable | PATCH changing an @immutable field answers 422 field_not_writable | PASS |
| B | Immutable | PATCH carrying an @immutable field unchanged is accepted | PASS |
| B | Immutable | PUT changing an @immutable field is refused | PASS |
| B | Private field | an @private field never appears in a create response | PASS |
| B | Private field | an @private field never appears in a GET response | PASS |
| B | Private field | an @private field never appears in a list response | PASS |
| B | Excluded from DTO | POST carrying an @exclude_from_dto field answers 422 unknown_field | PASS |
| B | Excluded from DTO | PATCH cannot set an @exclude_from_dto field | PASS |
| B | State field | a generic PATCH of a machine's state field answers 422 state_field_verbs_only | PASS |
| B | Lifecycle projection | a generic PATCH of a projection field answers 422 field_not_writable | PASS |
| B | Lifecycle projection | POST carrying a projection field answers 422 unknown_field (born values stay out of the create DTO) | PASS |
| B | Lifecycle hand_set | an entity with a hand_set field mounts no generic create (405) | PASS |
| B | DB constraints | a dangling foreign key answers 422 reference_missing on project_id | PASS |
| B | DB constraints | permanently deleting a referenced row answers 422 still_referenced | PASS |
| B | DB constraints | a duplicate @unique value answers 422 naming contact_email, without the raw index name | PASS |
| B | DB constraints | a duplicate composite unique (project_id, title) answers 422 with a field path, without the raw index name | PASS |
| B | DB constraints | PATCH nulling a NOT NULL column answers 422 required on its path | PASS |
| B | DB constraints | a CHECK from @non_negative refuses a raw write and maps to non_negative on budget | PASS |
| B | DB constraints | a CHECK from a declared rule refuses a raw write and maps to its code, on the path the guard reports (kind) | PASS |
| B | DB constraints | a CHECK from @required refuses a blank title and maps to required | PASS |
| B | Field types | a `text?` field maps to Option<String> | PASS |
| C | Declared rule | declared row rule refuses: amount over budget_cap answers 422 over_budget | PASS |
| C | Declared rule | declared row rule lets a valid row through (amount 50 <= budget_cap 100, both decimal) | PASS |
| C | Declared rule | declared row rule refuses on update too | PASS |
| C | Declared rule | declared row rule refuses: a discount without a reason answers 422 discount_reason_required | PASS |
| C | Declared rule | declared row rule refuses: a client project without a website answers 422 website_required_for_client | PASS |
| C | Declared uniqueness | declared uniqueness rule refuses a duplicate code in the company: 422 code_taken on path code | PASS |
| C | Declared uniqueness | declared uniqueness rule is per company: the same code in another company is accepted | PASS |
| C | Declared uniqueness | declared uniqueness rule ignores soft-deleted rows (its filter) | PASS |
| C | Shadow rule | shadow rule lets the write through | PASS |
| C | Unenforced rule | unenforced rule lets the write through | PASS |
| C | Extension check | the hand-written check refuses through the extension's service: 422 name_forbidden | PASS |
| C | Extension check | the schema guard and the hand-written check report together | PASS |
| C | Extension check | the generated service (no extension) does not run the hand-written check | PASS |
| C | Verb rule | rule held by a hand-written verb refuses a blank reason: 422 archive_reason_required | PASS |
| C | Verb rule | rule held by a hand-written verb is not held by the generated archive verb (VERB holds on its own path only, as documented) | PASS |
| D | Lifecycle hooks | create fires before_create, after_create and a created event exactly once each | PASS |
| D | Lifecycle hooks | hooks run in order: before, after, event | PASS |
| D | Lifecycle hooks | PATCH fires before_update/after_update once and an event carrying before and after | PASS |
| D | Lifecycle hooks | before_update sees the row as it will be stored | PASS |
| D | Lifecycle hooks | PUT fires the update hooks once and an updated event with before and after | PASS |
| D | Lifecycle hooks | a refused write fires no hook and no event | PASS |
| D | Lifecycle hooks | soft delete fires before_delete, after_delete and a soft_deleted event once each | PASS |
| D | Lifecycle hooks | restore fires before_restore and a restored event once each | PASS |
| D | Lifecycle hooks | permanent delete fires before_hard_delete and a hard_deleted event once each | PASS |
| D | Lifecycle hooks | bulk create fires before_create and after_create once per item | PASS |
| D | Lifecycle hooks | bulk patch fires the update hooks once per item | PASS |
| D | Declared events | a create stages exactly one TicketOpened row | PASS |
| D | Declared events | the staged row carries the request's correlation id | PASS |
| D | Declared events | the staged row carries the declared version and the payload read from the stored row | PASS |
| D | Declared events | a create with no correlation header still stages one, under an id minted for the request | PASS |
| D | Declared events | a create the guard refuses stages no event | PASS |
| D | Declared events | a create the database refuses (a duplicate title) stages no event | PASS |
| D | Declared events | when staging the event fails the create fails and no ticket row exists | PASS |
| D | Declared events | the failed staging does not leak the outbox error to the client | PASS |
| D | Declared events | a transition stages its event (resolve stages TicketResolved) | PASS |
| D | Declared events | a transition's event carries the request's correlation id and the stored row's values | PASS |
| D | Declared events | an event whose `if:` does not hold is not staged (amount 50 < 100) | PASS |
| D | Declared events | a refused transition stages nothing (a failing guard, a wrong state) | PASS |
| D | Declared events | entering a state stages its event (archive enters archived: TicketArchived) | PASS |
| D | Declared events | update(title) fires only when the title changed | PASS |
| D | Declared events | a delete stages its event in the same transaction | PASS |
| D | Hand-staged events | a hand-staged declared event is staged by hand code inside the flow's transaction (TicketEscalated) | PASS |
| D | Hand-staged events | an escalation of an unknown ticket stages nothing | PASS |
| D | Write hooks | a hook that fails after staging rolls back the write and its event | PASS |
| D | Write hooks | a hand-written WriteHooks stages its event with the write, under the correlation id | PASS |
| D | Declared events | a bulk create refused partway commits no row and no event | PASS |
| D | Declared events | a bulk create stages one event per row with the rows | PASS |
| D | Declared events | a bulk patch refused partway changes no row and stages no event | PASS |
| D | Declared events | a bulk patch stages one event per changed row | PASS |
| D | Relay | a relay drain marks the staged row published | PASS |
| D | Relay | a failing publish leaves the row pending, an exhausted one makes it dead | PASS |
| D | Relay | re-emitting a dead row delivers it again | PASS |
| D | Relay | a consumer's inbox::once dedupes a replay of the same event | PASS |
| D | Notification delivery | the relay hands the NotificationRequested rows a trigger staged to the registered delivery, once each, under the request's correlation id | PASS |
| D | Notification delivery | a replayed row is deduped by the inbox: the delivery is not called again | PASS |
| D | Notification delivery | a Retry leaves the request pending, and the limit dead-letters it with the reason | PASS |
| D | Notification delivery | a Reject dead-letters the request at once with the delivery's reason | PASS |
| D | Hook triggers | a before_create `set:` is persisted (sla, handled_by) | PASS |
| D | Hook triggers | a trigger keyed by a label with `timing: before_create` and a `condition:` runs after the one before it, and only when it holds | PASS |
| D | Hook triggers | after_create stages what its compiled actions say: the emitted event and a request per notify and send_email | PASS |
| D | Hook triggers | every row a trigger staged carries the request's correlation id | PASS |
| D | Hook triggers | `emit:` builds the declared payload from its arguments | PASS |
| D | Hook triggers | `notify` stages a NotificationRequested with its text interpolated, its recipients and variables | PASS |
| D | Hook triggers | `send_email` stages a NotificationRequested naming its template and recipient | PASS |
| D | Hook triggers | `log()` writes a line carrying the correlation id, the trigger it came from and its fields | PASS |
| D | Hook triggers | a `log()` field the write cannot know ($context.ip_address) is left out, not invented | PASS |
| D | Hook triggers | a write that leaves status and title alone fires neither after_update_status nor after_update_title | PASS |
| D | Hook triggers | after_update_title fires when a generic write changes the title (its `if:` over $old holds) | PASS |
| D | Hook triggers | after_update_status fires when a transition changes the state, and carries the state it left ($old) | PASS |
| D | Hook triggers | on_enter emits on a transition into the state, on_exit on a transition out of it | PASS |
| D | Hook triggers | after_update_status_resolved does not fire for a change to another state | PASS |
| D | Hook triggers | after_update_status_resolved fires when the state becomes resolved, beside after_update_status | PASS |
| D | Hook triggers | an after_<verb> trigger runs its hand-written method on that transition and no other | PASS |
| D | Hook triggers | a hand method returning an error refuses the write with its code (422) and leaves no row | PASS |
| D | Hook triggers | and takes down what the hooks staged before it failed (the event and both requests) | PASS |
| D | Hook triggers | triggers that write through the generated service join the transaction: a chain of three commits all three rows | PASS |
| D | Hook triggers | nesting beyond depth 8 is refused with the code trigger_depth_exceeded | PASS |
| D | Hook triggers | and the refusal rolls the whole chain back: no row, no staged request | PASS |
| D | Hook triggers | a hand-fired trigger (after_page) runs when hand code calls fire_after_page: its emit and its hand method stage in one transaction | PASS |
| D | Hook triggers | a soft delete fires after_delete | PASS |
| D | Hook triggers | and before_delete, which runs before the row goes | PASS |
| D | Hook triggers | a `schedule:` entry registers a scheduled job (its name and schedule) and the generated runner calls its required method | PASS |
| D | Declared events | an `on:` or `if:` naming a field, verb or state the model lacks fails generation, naming each | PASS |
| D | Hook triggers | an `emit:` of an event no schema file declares fails generation, naming the event and showing the declaration to add | PASS |
| D | Hook triggers | a trigger's own `if:` or `condition:` the rule language cannot compile fails generation (a guard is never guessed) | PASS |
| D | Hook triggers | a trigger keyed on a field the model does not have fails generation, naming the key | PASS |
| D | Hook triggers | triggers that need hand methods fail generation in a module without the extension layout, saying where the implementation lives | PASS |
| D | Hook triggers | `generators: legacy_triggers: true` keeps the old behaviour: generation succeeds, writes no trigger code, and `schema validate` warns that the triggers run nothing | PASS |
| D | Declared events | generation says a declared event with no `on:` is staged by hand, and names it | PASS |
| D | Declared events | events with `on:` generate their typed payloads and the write hooks that stage them | PASS |
| D | Hook triggers | `schema doctor` fails on an extension that lacks a required trigger method, naming the method and the trigger that needs it | PASS |
| D | Hook triggers | `schema doctor` fails on a missing extension file, saying how many methods it owes | PASS |
| D | Hook triggers | `schema doctor` warns, without failing, on a trigger extension that calls an HTTP client inside the write's transaction | PASS |
| D | Hook triggers | an implementation of the generated trait that lacks a required method breaks the build with a named error | PASS |
| E | Transitions | an allowed transition (start: open → in_progress) answers 200 and moves the state | PASS |
| E | Transitions | a disallowed transition (archive from in_progress) is refused with 4xx | PASS |
| E | Transitions | a disallowed transition names a stable code | PASS |
| E | Transition guard | a transition whose guard does not hold (amount <= 1000) is refused 422 | PASS |
| E | Transition guard | a refused guard names a stable code | PASS |
| E | Transition guard | the same transition passes once its guard holds | PASS |
| E | Final state | archive enters the final state | PASS |
| E | Final state | a final state cannot be left by an ordinary edge | PASS |
| E | Final state | the from: "*" edge leaves the final state | PASS |
| E | Generic writes | a generic PATCH of the state field is refused: 422 state_field_verbs_only | PASS |
| E | Generic writes | a generic PUT cannot move the state field | PASS |
| E | Permissions | a caller without <model>:transition:<verb> is refused 403 | PASS |
| E | Scoping | another company's caller cannot transition the ticket (404) | PASS |
| E | DB trigger | raw UPDATE refused by the transition trigger and mapped to state_transition_not_declared on status | PASS |
| E | hand_set + machine | a generic PATCH of the hand_set machine field is refused (no generic write route: 405) | PASS |
| E | hand_set + machine | the generated verb moves the hand_set field (submit: draft → submitted) | PASS |
| E | hand_set + machine | a verb from the wrong state is refused | PASS |
| E | hand_set + machine | the final verb (approve) completes | PASS |
| E | hand_set + machine | the read routes of a hand_set entity are mounted | PASS |
| E | Transitions | a machine declared in its own hook file (rules in another file for the same model) still gets its generated verbs | PASS |
| F | Custom route | the custom escalate route is reachable through the stack and moves the ticket by its verb | PASS |
| F | Custom route | the custom route reads the org scope the stack bound (the acting unit stamped in the note) | PASS |
| F | Custom route | the custom route is guarded: no token answers 401 | PASS |
| F | Custom route | the custom route is fenced: another company's caller gets 404 | PASS |
| F | Custom route | the custom route's write still meets the database's transition trigger (in_progress → in_progress is not a move; archived → in_progress is refused) | PASS |
| F | Custom route | escalating an archived ticket is refused by the transition trigger: 422 state_transition_not_declared | PASS |
| F | Custom route | the custom route's write passes the schema guard (a valid row is accepted) | PASS |
| F | Custom service method | the custom service method archives with a reason | PASS |
| F | Custom service method | the custom service method consults the generated machine: archiving an open ticket answers 422 transition_not_allowed | PASS |
| F | Extension routes | the extension's generic routes sit behind the stack (no token answers 401) | PASS |
| F | Extension routes | the extension's generic routes are fenced (company B sees none of A's projects) | PASS |
| F | Workflow engine | an event starts one instance, and the same event delivered twice still makes one | PASS |
| F | Workflow engine | the instance runs its steps to the wait and stops there: waiting on await_ack, each step logged, the start event's correlation id kept | PASS |
| F | Workflow engine | a step's `set:` and the trigger's `extract:` land in the instance context (severity read from the incident through the generated service) | PASS |
| F | Workflow engine | a condition step takes its branch: a severity 1 incident is not paged | PASS |
| F | Workflow engine | a bespoke verb runs through the Actions method of the extension, inside the step's transaction, and a declared function evaluates in an expression (who = on_call_name(severity)) | PASS |
| F | Workflow engine | `send_email` stages a NotificationRequested on the step's transaction (the effect is the relay's, never the step's): channel, template, recipient, variables and the step that asked | PASS |
| F | Workflow engine | a row a step stages carries the instance's correlation id and the starting event as its cause | PASS |
| F | Workflow engine | a wait ignores an event whose key does not match: another incident's acknowledgement leaves the instance waiting | PASS |
| F | Workflow engine | a wait resumes on the matching event: acknowledging the incident (the real transition, the real staged event) completes the instance | PASS |
| F | Workflow engine | a transition step moves the record through its state machine: the incident is `resolved` | PASS |
| F | Workflow engine | an entity action goes through the generated service, so the entity's own triggers and declared events apply: the machine's `after_update_status_resolved` staged IncidentResolved | PASS |
| F | Workflow engine | the step log shows each stage: waiting, resumed, then the transition, the event and the terminal | PASS |
| F | Workflow engine | `emit_event` stages a declared event on the step's transaction with the instance's correlation and causation | PASS |
| F | Workflow engine | a resume event delivered again changes nothing (the relay delivers at least once) | PASS |
| F | Workflow engine | a wait expires through workflow_timer_sweep when its due time is in the past (and not before): the on_timeout branch runs | PASS |
| F | Workflow engine | a transition the machine refuses (resolve from open) is a step failure: the failure branch runs and the error reaches the expression | PASS |
| F | Workflow engine | the refused transition changed nothing: the incident is still open | PASS |
| F | Workflow engine | an instance past the workflow's deadline (config.timeout) ends `expired` | PASS |
| F | Workflow engine | a step that fails after writing leaves none of its writes: the row it staged is gone, the earlier step's follow-up stayed, and the failure is logged | PASS |
| F | Workflow engine | a transient failure stores its backoff and waits: nothing is retried before it passes | PASS |
| F | Workflow engine | a failing step is tried three times (backoff observable between them), then the instance fails and compensates: compensated | PASS |
| F | Workflow engine | compensation runs through the generated service: the follow-up is soft-deleted and its IncidentDeleted event staged | PASS |
| F | Workflow engine | compensation is idempotent: running it again repeats nothing and changes nothing | PASS |
| F | Workflow engine | a business failure (the verb refuses with a validation error) is not retried: one failed attempt, then compensation | PASS |
| F | Workflow engine | a run cut inside a step's transaction (a crash) leaves the instance running on that step, with the step's row rolled back and the completed step logged once | PASS |
| F | Workflow engine | the sweep drives an instance nobody is running and the completed step is not repeated (one follow-up incident, never two) | PASS |
| F | Workflow engine | an event of company B for company A's key never resumes company A's instance | PASS |
| F | Workflow engine | another company's scope cannot read the instance or its step log; its own company can | PASS |
| F | Workflow engine | the fence is the database: the app role with no company scope sees no instance and no step | PASS |
| F | Workflow engine | the sweep still serves every company: company A's due wait runs under company A's scope | PASS |
| F | Workflow engine | every instance carries its event's company | PASS |
| F | Workflow engine | `emit_event`, also on the branch the sweep resumed stages its row under the org unit of the event that started the instance, on an outbox fenced by org unit | PASS |
| F | Workflow engine | `send_email` stages its row under the org unit of the event that started the instance, on an outbox fenced by org unit | PASS |
| F | Workflow engine | a terminal step's `emit` stages its row under the org unit of the event that started the instance, on an outbox fenced by org unit | PASS |
| F | Workflow engine | a compensation step's `emit_event` stages its row under the org unit of the event that started the instance, on an outbox fenced by org unit | PASS |
| F | Workflow engine | no row the engine staged has no org unit (the fence refuses one, loudly, rather than lose it) | PASS |
| F | Workflow engine | the instance and each of its step rows carry the org unit of its start event | PASS |
| F | Workflow engine | the outbox the engine stages into is fenced by org unit, as serpa's is (backbone-outbox `multi_tenant`): a row staged with no unit in scope would be refused | PASS |
| F | Workflow engine | an event of company A staged under an org unit outside the instance's unit cannot resume it: the fence, not the key, refuses | PASS |
| F | Workflow engine | an instance of an org-fenced module is readable only under its own unit: a company scope with no unit and another unit see nothing | PASS |
| F | Workflow engine | the same event under the instance's own unit resumes it | PASS |
| F | Workflow engine | an org-fenced module's workflow tables carry the org-unit policy, not the company one | PASS |
| F | Workflow engine | the module's background tasks list the events the engine reacts to: each trigger and each awaited event | PASS |
| F | Workflow engine | deliver accepts an event nobody subscribed to and does nothing (the relay's sink can be shared) | PASS |
| F | Workflow | generation says a workflow with no executor and no `implemented_by` is documentation only, and names it (and no other) | PASS |
| F | Workflow | a workflow with no `executor` generates nothing and runs nothing: no generated file mentions it | PASS |
| F | Workflow | the two engine workflows generate their spec, trait and definition; the module hands over its engine; one new migration creates the tables | PASS |
| F | Workflow | a bespoke verb and a declared function are required methods of the generated trait with no default body; the doc-only file generates none | PASS |
| F | Workflow engine | a function call in a workflow condition fails generation unless it is listed under `functions:`, naming the function and the step | PASS |
| F | Workflow engine | a wait condition that is not an equality key fails validation: an instance is found by key, not by evaluating a condition against every waiting instance | PASS |
| F | Workflow engine | with `executor` absent a workflow generates nothing: no workflows module, no engine in the module, no tables, and generation says it is documentation only | PASS |
| F | Workflow engine | `schema doctor` fails on an extension that lacks a required workflow method, naming the method and what needs it | PASS |
| F | Workflow engine | `schema doctor` fails on a missing extension file, saying how many methods it owes | PASS |
| F | Workflow engine | `schema doctor` fails on a documented saga whose `implemented_by` names a function that does not exist | PASS |
| F | Workflow engine | an implementation of the generated Actions trait that lacks a required method breaks the build with a named error | PASS |
| G | RLS | the app connects as a role that is neither superuser nor BYPASSRLS | PASS |
| G | Setup | company A creates a project and a branch ticket | PASS |
| G | Read fence | list: B sees none of A's projects | PASS |
| G | Read fence | get: B gets 404 for A's project | PASS |
| G | Read fence | get: B gets 404 for A's org-fenced ticket | PASS |
| G | Read fence | the company's session sees its branch's ticket (subtree union) | PASS |
| G | Write fence | update: B's PATCH of A's project answers 404 | PASS |
| G | Write fence | update: B's PUT of A's project answers 404 | PASS |
| G | Write fence | delete: B's DELETE of A's project answers 404 | PASS |
| G | Write fence | bulk: B's bulk patch and bulk delete of A's ids change nothing | PASS |
| G | Write fence | bulk: B's bulk permanent delete of A's id removes nothing | PASS |
| G | Write fence | B cannot attach its ticket to a project it cannot see (foreign keys check rows RLS hides) | PASS |
| G | Read fence | include=: B cannot read A's project through a relation (list) | PASS |
| G | Read fence | include=: B cannot read A's project through a relation (get by id) | PASS |
| G | Read fence | include=: A's own ticket still hydrates A's project | PASS |
| G | Read fence | aggregate: B's aggregate sees none of A's rows | PASS |
| G | Read fence | count: B's count of A's code is 0 | PASS |
| G | Read fence | an OR-filter attempt (?code[or]=…) cannot widen past the fence | PASS |
| G | Read fence | ?__base_condition= has no effect | PASS |
| G | Read fence | ?__base_condition= cannot surface soft-deleted rows | PASS |
| G | Read fence | trash: B's trash list does not show A's deleted rows | PASS |
| G | Write fence | restore-all: B cannot restore A's trash | PASS |
| G | Write fence | empty-trash: B cannot purge A's trash | PASS |
| G | Write fence | a create naming another company is refused (RLS WITH CHECK) and writes nothing | PASS |
| G | Write fence | a ticket naming another company's org unit is refused and writes nothing | PASS |
| G | Write fence | the org kind guard refuses a ticket on the root node with a 4xx naming org_unit_id | PASS |
| G | Tokens | a token minted for another tenant (aud) is refused 401 | PASS |
| G | Tokens | a token naming no tenant is refused 401 on a tenant's surface | PASS |
| G | Tokens | no token is refused 401 | PASS |
| G | Tokens | an expired token is refused 401 | PASS |
| G | Tokens | a refresh token is refused 401 on a guarded route | PASS |
| G | Tokens | a token whose org unit is not in this tenant's tree is refused 403 | PASS |
| G | Tokens | a host that names an unknown tenant is refused before auth (503/404) | PASS |
| G | RLS | the app role without a scope reads zero rows from fenced tables (fail-closed) | PASS |
| G | RLS | the app role scoped to B reads none of A's rows even when asking for them | PASS |
| G | RLS | every scoped table has RLS enabled and forced | PASS |
| H | Refusals | every violation the probes received names a code and carries a path key | PASS |
| H | Refusals | every 400/409/422 refusal carries a stable code (a violation code) | PASS |
| H | Refusals | no refusal leaks a raw constraint, index or trigger name | PASS |
| H | Refusals | a 404 names a stable code | PASS |
| H | Refusals | a 404 does not leak the Rust type path | PASS |
| H | Correlation | a caller's X-Correlation-ID is echoed on the response | PASS |
| H | Correlation | a refusal carries a minted correlation id header when the caller sent none | PASS |
| H | Rules register | `schema rules . -f json` runs and parses | PASS |
| H | Rules register | rule `price_within_budget` is labelled GUARD and the runtime agrees | PASS |
| H | Rules register | rule `discount_needs_reason` is labelled GUARD and the runtime agrees | PASS |
| H | Rules register | rule `client_needs_website` is labelled GUARD and the runtime agrees | PASS |
| H | Rules register | rule `code_unique_per_company` is labelled GUARD and the runtime agrees | PASS |
| H | Rules register | rule `note_shadowed` is labelled SHADOW and the runtime agrees | PASS |
| H | Rules register | rule `archive_reason_verb` is labelled VERB and the runtime agrees | PASS |
| H | Rules register | rule `aspiration_unenforced` is labelled UNENFORCED and the runtime agrees | PASS |
| H | Rules register | event `TicketOpened` is labelled EVENT STAGED (on create, in tx) and the runtime agrees | PASS |
| H | Rules register | event `TicketResolved` is labelled EVENT STAGED (on transition(resolve), in tx) and the runtime agrees | PASS |
| H | Rules register | event `TicketEscalated` is labelled EVENT HAND-STAGED and the runtime agrees | PASS |
| H | Rules register | trigger `after_update_status` is labelled TRIGGER RUNS (after_update status, in tx) and the runtime agrees | PASS |
| H | Rules register | trigger `after_create` is labelled TRIGGER RUNS (after_create, in tx) and the runtime agrees | PASS |
| H | Rules register | trigger `before_create` is labelled TRIGGER RUNS (before_create, in tx) and the runtime agrees | PASS |
| H | Rules register | trigger `on_enter acknowledged` is labelled TRIGGER RUNS (on_enter acknowledged, in tx) and the runtime agrees | PASS |
| H | Rules register | trigger `sweep_stale` is labelled TRIGGER SCHEDULED (every 5.minutes) and the runtime agrees | PASS |
| H | Rules register | trigger `after_page` is labelled TRIGGER RUNS (hand-fired by fire_after_page, in tx) and the runtime agrees | PASS |
| H | Rules register | a trigger's hand actions are labelled TRIGGER HAND with the extension method each needs, and the runtime agrees | PASS |
| H | Rules register | an event a trigger emits is labelled EVENT STAGED (by trigger <Model>.<key>, in tx), not hand-staged | PASS |
| H | Rules register | machine `Ticket.status` is labelled MOVES REFUSED and the transition trigger refuses a raw UPDATE | PASS |
| H | Rules register | machine `Ticket.status` is labelled GUARDS IN GENERATED VERBS and the generated verb refuses a failing guard | PASS |
| H | Rules register | machine `Approval.state` (hand_set) is labelled MOVES REFUSED and its verbs refuse a wrong-state move | PASS |
| H | Rules register | `schema rules` labels a workflow the engine runs WORKFLOW RUNS (on its event, persisted, executor=engine) and lists its hand methods | PASS |
| H | Rules register | `schema rules` labels a saga that names its hand function WORKFLOW DOCUMENTS and one with neither executor nor function WORKFLOW DOCUMENTED | PASS |
| H | Schema doctor | `schema doctor .` counts the module's triggers and the hand methods they owe | PASS |
| H | Schema doctor | `schema doctor .` counts the workflows the engine runs, the hand methods they owe, and points at where a stuck instance shows | PASS |
| H | Schema doctor | `schema doctor .` accepts the freshly generated module with its extension files | PASS |
| H | Diagnostics | a hook file with an `assert:` rule (no message) beside its `states:` generates, or names the offending rule | PASS |
| I | Seeds | the seed migration writes every permission the routes require, the declared custom verbs and the system roles' grants | PASS |
| I | Seeds | a declared system role is seeded with its baseline grants (the dotted spelling normalised: projects.read -> project:read) | PASS |
| I | Seeds | the ADMIN system role is declared with *:* in the seed | PASS |
| I | Generic routes | a holder of ticket:read passes the permission layer on list | PASS |
| I | Generic routes | a holder of ticket:read passes the permission layer on get | PASS |
| I | Generic routes | a holder of ticket:read passes the permission layer on count | PASS |
| I | Generic routes | a holder of ticket:read passes the permission layer on trash | PASS |
| I | Generic routes | a holder of ticket:read passes the permission layer on trash count | PASS |
| I | Generic routes | a holder of ticket:read passes the permission layer on aggregate | PASS |
| I | Generic routes | a holder of ticket:read passes the permission layer on history | PASS |
| I | Generic routes | a caller without ticket:read is refused on list: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | a caller without ticket:read is refused on get: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | a caller without ticket:read is refused on count: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | a caller without ticket:read is refused on aggregate: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | a caller without ticket:read is refused on history: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | a caller without ticket:read is refused on trash: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | a caller without ticket:read is refused on trash count: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | a caller without ticket:read is refused on deleted get: 403 permission_required naming ticket:read | PASS |
| I | Generic routes | create needs ticket:create: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | bulk create needs ticket:create: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | upsert needs ticket:create: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | put needs ticket:update: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | patch needs ticket:update: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | bulk put needs ticket:update: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | bulk patch needs ticket:update: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | bulk preview needs ticket:update: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | delete needs ticket:delete: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | bulk delete needs ticket:delete: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | restore needs ticket:restore: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | bulk restore needs ticket:restore: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | restore all needs ticket:restore: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | purge needs ticket:purge: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | bulk purge needs ticket:purge: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | empty trash needs ticket:purge: a holder of read only is refused 403 naming it | PASS |
| I | Generic routes | no refused write changed anything | PASS |
| I | Generic routes | a caller without ticket:update learns nothing about an unknown id (403, not 404) | PASS |
| I | Generic routes | a malformed body from a caller without ticket:create is still 403, not 400 | PASS |
| I | Generic routes | project:read (a seeded grant of the viewer role) opens the project list | PASS |
| I | Generic routes | an entity the caller holds nothing for is refused: approval:read | PASS |
| I | Generic routes | a read-only entity mounts no write route (405), whatever the caller holds | PASS |
| I | Transitions | a transition needs <model>:<verb>: 403 naming ticket:start | PASS |
| I | Transitions | the permission is checked before the record is read: an unknown id is 403 for a caller without it | PASS |
| I | Transitions | a refused transition changed nothing | PASS |
| I | Custom verbs | a hand-written verb guarded by guarded(): 403 naming ticket:escalate | PASS |
| I | Custom verbs | a second guarded verb names its own permission: ticket:archive_with_reason | PASS |
| I | Custom verbs | the extension's generic routes are guarded too: project:read | PASS |
| I | Wildcards | ticket:* grants every action of the entity (create) | PASS |
| I | Wildcards | ticket:* grants the entity's transitions | PASS |
| I | Wildcards | ticket:* does not reach another entity | PASS |
| I | Wildcards | *:read grants every read and no write | PASS |
| I | Wildcards | *:* grants everything (create and purge) | PASS |
| I | Wildcards | the ADMIN role grants everything by name, with no permission row of its own for the person | PASS |
| I | Legacy spellings | *:transition:* still grants a verb, and opens no generic read or write | PASS |
| I | Legacy spellings | the dotted spelling tickets.read still grants the read (normalised) | PASS |
| I | Legacy spellings | a legacy spelling that grants access raises the deprecation signal with the canonical form to rewrite to | PASS |
| I | Tenant roles as data | before the role exists the person is refused | PASS |
| I | Tenant roles as data | a role created as rows grants access at the next request, with no regeneration and no restart | PASS |
| I | Tenant roles as data | the role grants only what its rows say: ticket:delete is still refused | PASS |
| I | Tenant roles as data | an upsert needs create and update: a holder of update only is refused on create | PASS |
| I | Tenant roles as data | removing the assignment removes the access at the next request | PASS |
| I | Resolver mode | with a resolver installed the token is not the source: a subject the identity tables do not know holds nothing | PASS |
| I | Resolver mode | a grant store that cannot be read refuses the request (503 grants_unavailable), never reads as no grants and carries on | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: list is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: create is 403 naming ticket:create | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: patch is 403 naming ticket:update | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: delete is 403 naming ticket:delete | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: a transition is 403 naming ticket:start | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: a custom verb is 403 naming ticket:escalate | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: the extension's generic list is 403 naming project:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: get is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: count is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: aggregate is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: history is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: trash is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: trash count is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: deleted get is 403 naming ticket:read | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: create is 403 naming ticket:create | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: bulk create is 403 naming ticket:create | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: upsert is 403 naming ticket:create | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: put is 403 naming ticket:update | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: patch is 403 naming ticket:update | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: bulk put is 403 naming ticket:update | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: bulk patch is 403 naming ticket:update | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: bulk preview is 403 naming ticket:update | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: delete is 403 naming ticket:delete | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: bulk delete is 403 naming ticket:delete | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: restore is 403 naming ticket:restore | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: bulk restore is 403 naming ticket:restore | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: restore all is 403 naming ticket:restore | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: purge is 403 naming ticket:purge | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: bulk purge is 403 naming ticket:purge | PASS |
| I | Fail closed | no resolver and no permission on the AuthContext: empty trash is 403 naming ticket:purge | PASS |
| I | Fail closed | no resolver: the permissions on the AuthContext are what counts (ticket:read reads, and only reads) | PASS |
| I | Fail closed | no AuthContext on the request at all is 401 authentication_required, never allowed | PASS |
| I | Fail closed | a transition with no AuthContext is 401 too | PASS |
| I | Fail closed | a request an outer gate admitted on a narrower rule (PermissionWaiver) passes the layer, and only that request | PASS |
| I | Seeds | the generator wrote one seed migration, named by a digest of its content | PASS |
| I | Seeds | re-running the seed migration changes nothing | PASS |
| I | Seeds | a tenant-edited role grant survives a re-run: the revoked baseline grant stays revoked (no duplicate row) and the added grant stays | PASS |
| I | Seeds | and the edit is what the next request sees: the viewer lost the read and gained the create | PASS |
| I | Public routes | setup: an admin creates a label (label:create is still required to write it) | PASS |
| I | Public routes | a public read with no authentication answers 200: the list | PASS |
| I | Public routes | a public read with no authentication answers 200: one record | PASS |
| I | Public routes | a public read with no authentication answers 200: count | PASS |
| I | Public routes | a logged-in caller who holds no permission reads the public model, and is refused a non-public one (ticket:read) | PASS |
| I | Public routes | the same model's create is not public: 401 with no authentication | PASS |
| I | Public routes | the same model's patch is not public: 401 with no authentication | PASS |
| I | Public routes | the same model's put is not public: 401 with no authentication | PASS |
| I | Public routes | the same model's delete is not public: 401 with no authentication | PASS |
| I | Public routes | the same model's bulk create is not public: 401 with no authentication | PASS |
| I | Public routes | the same model's restore is not public: 401 with no authentication | PASS |
| I | Public routes | the same model's empty trash is not public: 401 with no authentication | PASS |
| I | Public routes | the same model's create still needs label:create: 403 for a caller who holds nothing | PASS |
| I | Public routes | the same model's patch still needs label:update: 403 for a caller who holds nothing | PASS |
| I | Public routes | the same model's delete still needs label:delete: 403 for a caller who holds nothing | PASS |
| I | Public routes | a refused anonymous write changed nothing | PASS |
| I | Public routes | a non-public sibling is 401 beside it in the same mount: project list | PASS |
| I | Public routes | a non-public sibling is 401 beside it in the same mount: ticket list | PASS |
| I | Public routes | a non-public sibling is 401 beside it in the same mount: approval list | PASS |
| I | Public routes | `public_routes()` serves the public model's reads with no login, and mounts no other entity | PASS |
| I | Public routes | `public_routes()` still refuses an anonymous write to the public model: 401 | PASS |
| I | Public routes | the tenant fence still applies to a public route: an unknown tenant is refused, not served | PASS |
| I | Public routes | the seed holds the permissions the public model's writes require and none for its public reads | PASS |
| I | Register | `schema rules` lists the permission every generic route, generated verb and declared verb requires | PASS |
| I | Register | a declared system role's grants are normalised to the canonical spelling (projects.read -> project:read) | PASS |
| I | Register | `schema rules` labels each route group PERMISSION REQUIRED <permission> and says the mode (no resolver installed: AUTHCONTEXT) | PASS |
| I | Register | `schema rules` labels a `public: true` model PERMISSION PUBLIC (read), and no route group of it PERMISSION REQUIRED label:read | PASS |
| I | Schema doctor | doctor says the mode, and warns (without failing) on the dotted spelling a role uses | PASS |
| I | Schema doctor | an unguarded hand-written route fails `schema doctor`, naming the file, the line and the fix | PASS |
| I | Schema doctor | a route marked `// unguarded: <why>` is accepted, and the doctor prints the reason | PASS |
| I | Schema doctor | a guarded verb whose permission the vocabulary does not hold fails `schema doctor` | PASS |
| I | Schema doctor | a hand-written call of the unguarded generic routes (BackboneCrudHandler::routes) fails `schema doctor` | PASS |
| I | Register | the register and the doctor label the mode RESOLVER when the service installs a grant resolver | PASS |
| I | Declarations | `requires:` naming a permission the vocabulary does not declare fails generation, naming it | PASS |
| I | Declarations | a declared `requires:` renames the permission the entity's read routes need in the generated handler | PASS |
| I | Declarations | a system role that lists a malformed permission fails generation, naming the role and the entry | PASS |
| I | Public routes | a public write is a doctor warning, not a failure | PASS |
| I | Public routes | the generated handler declares each public action, and only those | PASS |
| I | Public routes | `public:` on a model that carries a company fence fails generation unless it says `public_scoped: true` | PASS |
| I | Public routes | `public_scoped: true` accepts a public model that carries a fence, and the doctor says it is fenced | PASS |
| I | Public routes | `public:` naming something that is no generic action fails generation, naming it | PASS |
| I | Public routes | a public action that also carries a `requires:` rename fails generation | PASS |

## Failures (0)

## Unsupported (0)

