# Conformance results

Written by `tests/conformance.rs`; do not edit by hand. Rerun with
`CONFORMANCE_DATABASE_URL=... cargo test --test conformance -- --ignored --nocapture`.

- Generator: metaphor-plugin-schema 0.35.0 (built from this checkout)
- Framework: local checkout `<workspace>/modules/backbone-framework` via [patch.crates-io]
- Generation: 2s; whole run: 62s

## Summary

| Area | PASS | FAIL | UNSUPPORTED |
|---|---|---|---|
| A. Generic routes | 57 | 0 | 1 |
| B. Validation | 40 | 2 | 0 |
| C. Custom validation | 14 | 1 | 0 |
| D. Hooks and events | 12 | 0 | 3 |
| E. State machine | 20 | 0 | 0 |
| F. Extension | 12 | 0 | 1 |
| G. Scoping | 33 | 1 | 0 |
| H. Debuggability | 20 | 0 | 0 |
| **Total** | **208** | **4** | **5** |

## Matrix

| Area | Feature | Probe | Result |
|---|---|---|---|
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
| A | History | row history of an `audited: true` model | UNSUPPORTED |
| A | Upsert | POST /projects/upsert answers 501 | PASS |
| A | Body errors | POST with an unknown key answers 422 unknown_field on that path | PASS |
| A | Body errors | PATCH with an unknown key answers 422 unknown_field | PASS |
| A | Body errors | a malformed JSON body answers 400 malformed_body | PASS |
| A | Body errors | POST with a wrong type answers 422 with the field's path | PASS |
| A | Body errors | PATCH with a wrong type answers 422 with the field's path | PASS |
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
| B | DB constraints | a duplicate @unique value answers 422 naming contact_email, without the raw index name | FAIL |
| B | DB constraints | a duplicate composite unique (project_id, title) answers 422 with a field path, without the raw index name | PASS |
| B | DB constraints | PATCH nulling a NOT NULL column answers 422 required on its path | PASS |
| B | DB constraints | a CHECK from @non_negative refuses a raw write and maps to non_negative on budget | PASS |
| B | DB constraints | a CHECK from a declared rule refuses a raw write and maps to its code, on the path the guard reports (kind) | FAIL |
| B | DB constraints | a CHECK from @required refuses a blank title and maps to required | PASS |
| B | Field types | a `text?` field maps to Option<String> | PASS |
| C | Declared rule | declared row rule refuses: amount over budget_cap answers 422 over_budget | PASS |
| C | Declared rule | declared row rule lets a valid row through (amount 50 <= budget_cap 100, both decimal) | PASS |
| C | Declared rule | declared row rule refuses on update too | PASS |
| C | Declared rule | declared row rule refuses: a discount without a reason answers 422 discount_reason_required | PASS |
| C | Declared rule | declared row rule refuses: a client project without a website answers 422 website_required_for_client | PASS |
| C | Declared uniqueness | declared uniqueness rule refuses a duplicate code in the company: 422 code_taken on path code | FAIL |
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
| D | Declared events | `events:` in a hook file produce publishing code | UNSUPPORTED |
| D | Outbox | an event staged in the write's transaction (outbox) | UNSUPPORTED |
| D | Schema lifecycle hooks | before/after create/update/delete declared as hook `triggers:` produce runtime hooks | UNSUPPORTED |
| D | Declared events | generation says the hook file's declared events are declaration-only | PASS |
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
| F | Workflow | generation says the workflow is declaration-only | PASS |
| F | Workflow | schema/workflows/ticket_escalation.workflow.yaml does something at runtime | UNSUPPORTED |
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
| G | Read fence | include=: B cannot read A's project through a relation | PASS |
| G | Write fence | B cannot attach its ticket to a project it cannot see (foreign keys check rows RLS hides) | FAIL |
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
| H | Rules register | machine `Ticket.status` is labelled MOVES REFUSED and the transition trigger refuses a raw UPDATE | PASS |
| H | Rules register | machine `Ticket.status` is labelled GUARDS IN GENERATED VERBS and the generated verb refuses a failing guard | PASS |
| H | Rules register | machine `Approval.state` (hand_set) is labelled MOVES REFUSED and its verbs refuse a wrong-state move | PASS |
| H | Schema doctor | `schema doctor .` accepts the freshly generated module with its extension files | PASS |
| H | Diagnostics | a hook file with an `assert:` rule (no message) beside its `states:` generates, or names the offending rule | PASS |

## Failures (4)

### X1 — B / DB constraints / a duplicate @unique value answers 422 naming contact_email, without the raw index name

- **Request:** POST /projects {"code":"P86E7D9","companyId":"1cf98c86-0ac0-449e-981b-b46f51420772","contactEmail":"u8b1197@example.com","name":"Project 6F02A3"}
- **Expected:** 422, violation path contact_email, no index name in the body
- **Got:** 422 {"success":false,"error":"repository error: validation failed: This value is already taken by another record","violations":[{"path":"","code":"unique","message":"This value is already taken by another record"}]}
- **Diagnosis:** metaphor-plugin-schema/src/generators/sql.rs:1063 names the @unique index idx_<table>_<field>; backbone-core/src/persistence/traits.rs:125/194 maps only uq_* names, so any other 23505 becomes AlreadyExists(raw pg message) → 409 with the constraint name

### X2 — B / DB constraints / a CHECK from a declared rule refuses a raw write and maps to its code, on the path the guard reports (kind)

- **Request:** UPDATE conformance.projects SET kind = 'client', website = NULL
- **Expected:** Violation {path: kind, code: website_required_for_client}, no ck_ name in the message
- **Got:** Violations([Violation { path: "", code: "website_required_for_client", params: {}, message: "The record does not satisfy one of its rules" }])
- **Diagnosis:** backbone-core/src/persistence/traits.rs:250 check_column reads the field out of `ck_<table>_<field>_<code>__<code>`; a hook rule's constraint is named after the rule (`ck_<table>_<rule>__<code>`, generators/sql.rs:2955), so its path comes back empty while the guard reports the rule's first field

### X3 — C / Declared uniqueness / declared uniqueness rule refuses a duplicate code in the company: 422 code_taken on path code

- **Request:** POST /projects {"code":"QAC89B7","companyId":"1cf98c86-0ac0-449e-981b-b46f51420772","name":"Unique two"}
- **Expected:** 422 {path: code, code: code_taken}
- **Got:** 422 {"success":false,"error":"repository error: validation failed: This value is already taken by another record","violations":[{"path":"","code":"code_taken","message":"This value is already taken by another record"}]}
- **Diagnosis:** backbone-core/src/persistence/traits.rs:194 takes the path from the DETAIL `Key (cols)=`, which Postgres omits for a table with RLS enabled (BuildIndexValueDescription), so a declared uniqueness rule on a fenced table answers path ""; the rule's own name carries no field to fall back on

### X4 — G / Write fence / B cannot attach its ticket to a project it cannot see (foreign keys check rows RLS hides)

- **Request:** POST /tickets {"orgUnitId":"bb67026c-5912-4899-b973-4ebde87c78e3","projectId":"8de7b436-3335-44e9-9f8d-1a7974074b8d","title":"B ticket on A project"}
- **Expected:** refused (422 reference_missing)
- **Got:** 201 {"success":true,"data":{"amount":"0","archiveReason":null,"budgetCap":null,"discount":null,"discountReason":null,"dueOn":null,"id":"8625e43e-704d-4731-955c-c37839a0194b","metadata":{"created_at":"2026-10-17T18:36:13.980349Z","updated_at":"2026-10-17T18:36:13.980349Z"},"note":null,"orgUnitId":"bb67026c-5912-4899-b973-4ebde87c78e3","projectId":"8de7b436-3335-44e9-9f8d-1a7974074b8d","reference":"…
- **Diagnosis:** Postgres referential checks bypass RLS, and nothing in the generated guard checks the referenced row is visible (rules/fields.rs; generators/sql.rs add_declared_foreign_keys): a scoped writer can probe for and reference another company's ids

## Unsupported (5)

- **A / History / row history of an `audited: true` model** — needs backbone-auditlog composed: the generated capture trigger calls auditlog.capture_data_change (generators/data_change_audit.rs:52) and the route needs an Extension<Arc<dyn HistoryProvider>> (backbone-core/src/http.rs:276); a standalone module has neither
- **D / Declared events / `events:` in a hook file produce publishing code** — nothing is generated: generation and schema doctor say the declared events are declaration-only (resolver/validator.rs declaration_only_warnings), and doctor warns about a declared event no hand code names (commands/schema/events_check.rs); the generic service publishes CrudEvents through the publisher the module attaches, proven above
- **D / Outbox / an event staged in the write's transaction (outbox)** — the generic service publishes fire-and-forget after the write, outside its transaction (backbone-core/src/service.rs, `let _ = self.event_publisher.publish(..)`); staging is hand code with backbone_outbox::outbox::stage, which the generator never emits
- **D / Schema lifecycle hooks / before/after create/update/delete declared as hook `triggers:` produce runtime hooks** — generation fails —   Error: Validation error: Hook 'Ticket' declares trigger 'after_create' with 1 action(s); hook triggers are not generated, so nothing runs them: move the behaviour into the service and declare the events it publishes under `events:` \\| Error: Schema validation failed with 1 error(s); the trigger target is retired (generators/mod.rs:621) and resolver/validator.rs:1916 refuses t…
- **F / Workflow / schema/workflows/ticket_escalation.workflow.yaml does something at runtime** — declaration-only: it is parsed and validated (resolver/validator.rs:55) and no generated file mentions it; the flow target is retired (generators/mod.rs:621) — the hand-written escalate route is the executable flow (proven in F)
