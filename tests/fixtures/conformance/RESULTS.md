# Conformance results

Written by `tests/conformance.rs`; do not edit by hand. Rerun with
`CONFORMANCE_DATABASE_URL=... cargo test --test conformance -- --ignored --nocapture`.

- Generator: metaphor-plugin-schema 0.35.0 (built from this checkout)
- Framework: local checkout `<workspace>/modules/backbone-framework` via [patch.crates-io]
- Generation: 1s; whole run: 24s

## Summary

| Area | PASS | FAIL | UNSUPPORTED |
|---|---|---|---|
| A. Generic routes | 53 | 4 | 1 |
| B. Validation | 33 | 8 | 1 |
| C. Custom validation | 13 | 2 | 0 |
| D. Hooks and events | 11 | 0 | 3 |
| E. State machine | 17 | 3 | 0 |
| F. Extension | 11 | 0 | 1 |
| G. Scoping | 30 | 4 | 0 |
| H. Debuggability | 15 | 5 | 0 |
| **Total** | **183** | **26** | **6** |

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
| A | List sort | the typed sort_by/sort_order parameters order the list | FAIL |
| A | List search | search=Alpha matches the name | PASS |
| A | List paging | offset paging limit=2&page=2 gives the 3rd and 4th rows | PASS |
| A | List paging | an oversized limit is clamped to the page cap (100) | FAIL |
| A | List paging | page=0 answers 200 or 400, never 5xx | FAIL |
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
| A | Body errors | PATCH with a wrong type answers 422 with the field's path | FAIL |
| B | Field attributes | @required refuses a blank name | PASS |
| B | Field attributes | @length(min=3) refuses a short name | PASS |
| B | Field attributes | @length(max=12) refuses a long code | PASS |
| B | Field attributes | @pattern refuses a lower-case code | PASS |
| B | Field attributes | @max(500) on text refuses a long summary | PASS |
| B | Field attributes | the email type refuses a malformed address | PASS |
| B | Field attributes | the url type refuses a malformed url | PASS |
| B | Field attributes | the @email attribute on a string field refuses a malformed address | FAIL |
| B | Field attributes | the @url attribute on a string field refuses a malformed url | FAIL |
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
| B | Excluded from DTO | PATCH cannot set an @exclude_from_dto field | FAIL |
| B | State field | a generic PATCH of a machine's state field answers 422 state_field_verbs_only | PASS |
| B | Lifecycle projection | a generic PATCH of a projection field answers 422 field_not_writable | PASS |
| B | Lifecycle projection | POST carrying a projection field answers 422 unknown_field (born values stay out of the create DTO) | PASS |
| B | Lifecycle hand_set | an entity with a hand_set field mounts no generic create (405) | PASS |
| B | DB constraints | a dangling foreign key answers 422 reference_missing on project_id | FAIL |
| B | DB constraints | permanently deleting a referenced row answers 422 still_referenced | PASS |
| B | DB constraints | a duplicate @unique value answers 422 naming contact_email, without the raw index name | FAIL |
| B | DB constraints | a duplicate composite unique (project_id, title) answers 422 with a field path, without the raw index name | FAIL |
| B | DB constraints | PATCH nulling a NOT NULL column answers 422 required on its path | FAIL |
| B | DB constraints | a CHECK from @non_negative refuses a raw write and maps to non_negative on budget | PASS |
| B | DB constraints | a CHECK from a declared rule refuses a raw write and maps to its code, on the path the guard reports (kind) | FAIL |
| B | DB constraints | a CHECK from @required refuses a blank title and maps to required | PASS |
| B | Field types | a `text?` field | UNSUPPORTED |
| C | Declared rule | declared row rule refuses: amount over budget_cap answers 422 over_budget | PASS |
| C | Declared rule | declared row rule lets a valid row through (amount 50 <= budget_cap 100, both decimal) | FAIL |
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
| E | Transitions | an allowed transition (start: open → in_progress) answers 200 and moves the state | PASS |
| E | Transitions | a disallowed transition (archive from in_progress) is refused with 4xx | PASS |
| E | Transitions | a disallowed transition names a stable code | FAIL |
| E | Transition guard | a transition whose guard does not hold (amount <= 1000) is refused 422 | PASS |
| E | Transition guard | a refused guard names a stable code | FAIL |
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
| E | Transitions | a machine declared in its own hook file (rules in another file for the same model) still gets its generated verbs | FAIL |
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
| G | Write fence | a create naming another company is refused (RLS WITH CHECK) and writes nothing | FAIL |
| G | Write fence | a ticket naming another company's org unit is refused and writes nothing | FAIL |
| G | Write fence | the org kind guard refuses a ticket on the root node with a 4xx naming org_unit_id | FAIL |
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
| H | Refusals | every 400/409/422 refusal carries a stable code (a violation code) | FAIL |
| H | Refusals | no refusal leaks a raw constraint, index or trigger name | FAIL |
| H | Refusals | a 404 names a stable code | FAIL |
| H | Refusals | a 404 does not leak the Rust type path | FAIL |
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
| H | Diagnostics | a hook file with an `assert:` rule (no message) beside its `states:` generates, or names the offending rule | FAIL |

## Failures (26)

### X1 — A / List sort / the typed sort_by/sort_order parameters order the list

- **Request:** GET /projects?code[startwith]=L6C08FA&sort_by=priority&sort_order=desc
- **Expected:** [5,4,3,2,1]
- **Got:** [1, 2, 3, 4, 5] 200
- **Diagnosis:** backbone-core/src/http.rs:393-408 repository_filters drops sort_by/sort_order (ListQueryParams declares them at http.rs:220-240)

### X2 — A / List paging / an oversized limit is clamped to the page cap (100)

- **Request:** GET /projects?code[startwith]=L6C08FA&limit=1000
- **Expected:** meta.limit <= 100
- **Got:** meta.limit 1000; 200
- **Diagnosis:** backbone-core/src/macros.rs:221 builds PaginationParams as a struct literal, bypassing PaginationParams::new's clamp

### X3 — A / List paging / page=0 answers 200 or 400, never 5xx

- **Request:** GET /projects?code[startwith]=L6C08FA&page=0
- **Expected:** 200 or 400
- **Got:** 0 the handler panicked: attempt to subtract with overflow
- **Diagnosis:** backbone-orm/src/repository.rs:48 offset() underflows (page - 1) for page 0

### X4 — A / Body errors / PATCH with a wrong type answers 422 with the field's path

- **Request:** PATCH /projects/bd0cf311-faa7-4d22-9793-8fcd3a06eb28 {"priority":"high"}
- **Expected:** 422, violation path priority
- **Got:** 400 {"success":false,"error":"validation failed: invalid type: string \"high\", expected i32"}
- **Diagnosis:** backbone-core/src/service.rs:1034 merge_patch answers ServiceError::Validation (400, no violations) for a value that does not deserialize

### X5 — B / Field attributes / the @email attribute on a string field refuses a malformed address

- **Request:** POST /projects {"backupEmail":"not-an-email","code":"P358854","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","name":"Project 11F78D"}
- **Expected:** 422 {path: backup_email, code: email}
- **Got:** 201 {"success":true,"data":{"backupEmail":"not-an-email","billable":true,"budget":"0","code":"P358854","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","contactEmail":null,"docsUrl":null,"externalRef":null,"fee":null,"headcount":null,"id":"b868a78a-e264-4ef0-bce8-47a1c7a5d8bc","internalScore":null,"kickoffAt":null,"kind":"internal","metadata":{"created_at":"2026-10-17T18:09:29.118742Z","updated_…
- **Diagnosis:** metaphor-plugin-schema/src/rules/fields.rs:308-313 derives the email/url checks from the field type only; the @email attribute is parsed (parser/yaml_parser/helpers.rs) and dropped

### X6 — B / Field attributes / the @url attribute on a string field refuses a malformed url

- **Request:** POST /projects {"code":"PAD1362","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","docsUrl":"not a url","name":"Project 89C705"}
- **Expected:** 422 {path: docs_url, code: url}
- **Got:** 201 {"success":true,"data":{"backupEmail":null,"billable":true,"budget":"0","code":"PAD1362","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","contactEmail":null,"docsUrl":"not a url","externalRef":null,"fee":null,"headcount":null,"id":"eda680de-27af-44a1-918a-16fa310961e9","internalScore":null,"kickoffAt":null,"kind":"internal","metadata":{"created_at":"2026-10-17T18:09:29.123755Z","updated_at"…
- **Diagnosis:** metaphor-plugin-schema/src/rules/fields.rs:308-313 derives the email/url checks from the field type only; the @url attribute is dropped

### X7 — B / Excluded from DTO / PATCH cannot set an @exclude_from_dto field

- **Request:** PATCH /projects/94987d68-0825-49e1-b7c4-b96b84490a9f {"internalScore":7}
- **Expected:** 422 and the column stays NULL
- **Got:** 200 {"success":true,"data":{"backupEmail":null,"billable":true,"budget":"0","code":"PA664F8","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","contactEmail":null,"docsUrl":null,"externalRef":"EXT-1","fee":null,"headcount":null,"id":"94987d68-0825-49e1-b7c4-b96b84490a9f","internalScore":7,"kickoffAt":null,"kind":"internal","metadata":{"created_at":"2026-10-17T18:09:29.169413Z","updated_at":"2026-…
- **Diagnosis:** backbone-core/src/service.rs:1034 merge_patch accepts any entity field; Model::write_protected_fields (metaphor-plugin-schema/src/ast/model.rs:173) leaves @exclude_from_dto out

### X8 — B / DB constraints / a dangling foreign key answers 422 reference_missing on project_id

- **Request:** POST /tickets {"orgUnitId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","projectId":"2ddc4ff8-b6f0-4021-bd59-43fa54b69b94","title":"Ticket B128D1"}
- **Expected:** 422 {path: project_id, code: reference_missing}
- **Got:** 422 {"success":false,"error":"repository error: validation failed: The record refers to a record that does not exist","violations":[{"path":"","code":"reference_missing","message":"The record refers to a record that does not exist"}]}
- **Diagnosis:** backbone-core/src/persistence/traits.rs:271 key_column reads the column from the DETAIL `Key (col)=`, which Postgres leaves out when the table has RLS enabled; the constraint name fk_<table>_<col> (generators/sql.rs) is not parsed, so the path comes back empty on every fenced table

### X9 — B / DB constraints / a duplicate @unique value answers 422 naming contact_email, without the raw index name

- **Request:** POST /projects {"code":"PE2EAAD","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","contactEmail":"ue25d1a@example.com","name":"Project 959595"}
- **Expected:** 422, violation path contact_email, no index name in the body
- **Got:** 409 {"success":false,"error":"repository error: Entity already exists: duplicate key value violates unique constraint \"idx_projects_contact_email\""}
- **Diagnosis:** metaphor-plugin-schema/src/generators/sql.rs:1063 names the @unique index idx_<table>_<field>; backbone-core/src/persistence/traits.rs:125/194 maps only uq_* names, so any other 23505 becomes AlreadyExists(raw pg message) → 409 with the constraint name

### X10 — B / DB constraints / a duplicate composite unique (project_id, title) answers 422 with a field path, without the raw index name

- **Request:** POST /tickets {"orgUnitId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","projectId":"1b6f4546-0574-4dca-bfb7-90b9ec5902cc","title":"Same title 29DBCD"}
- **Expected:** 422 with a violation, no index name in the body
- **Got:** 409 {"success":false,"error":"repository error: Entity already exists: duplicate key value violates unique constraint \"idx_tickets_project_id_title\""}
- **Diagnosis:** metaphor-plugin-schema/src/generators/sql.rs:1501 names `indexes: unique` idx_<table>_<cols>; backbone-core/src/persistence/traits.rs:125/194 maps only uq_* names → 409 AlreadyExists with the raw message

### X11 — B / DB constraints / PATCH nulling a NOT NULL column answers 422 required on its path

- **Request:** PATCH /tickets/7935523f-7aec-48be-a62e-259b02cfdce2 {"title":null}
- **Expected:** 422 {path: title, code: required}
- **Got:** 400 {"success":false,"error":"validation failed: invalid type: null, expected a string"}
- **Diagnosis:** backbone-core/src/service.rs:1034 merge_patch / persistence/traits.rs (23502)

### X12 — B / DB constraints / a CHECK from a declared rule refuses a raw write and maps to its code, on the path the guard reports (kind)

- **Request:** UPDATE conformance.projects SET kind = 'client', website = NULL
- **Expected:** Violation {path: kind, code: website_required_for_client}, no ck_ name in the message
- **Got:** Violations([Violation { path: "", code: "website_required_for_client", params: {}, message: "The record does not satisfy one of its rules" }])
- **Diagnosis:** backbone-core/src/persistence/traits.rs:250 check_column reads the field out of `ck_<table>_<field>_<code>__<code>`; a hook rule's constraint is named after the rule (`ck_<table>_<rule>__<code>`, generators/sql.rs:2955), so its path comes back empty while the guard reports the rule's first field

### X13 — C / Declared rule / declared row rule lets a valid row through (amount 50 <= budget_cap 100, both decimal)

- **Request:** POST /tickets {"amount":50,"budgetCap":100,"orgUnitId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","projectId":"0ec86eeb-47e0-4b4e-83a5-796974414fd5","title":"Ticket D56DDB"}
- **Expected:** 201
- **Got:** 422 {"success":false,"error":"validation failed: amount exceeds the budget cap","violations":[{"path":"budget_cap","code":"over_budget","message":"amount exceeds the budget cap"}]}
- **Diagnosis:** backbone-core/src/row_rule.rs:195-197 order() compares two Val::Text lexicographically, and a rust_decimal field serializes as a JSON string, so the guard compares "50" <= "100" as text (false); the generator emits r::field for decimals without a numeric cast (metaphor-plugin-schema/src/rules/compile.rs)

### X14 — C / Declared uniqueness / declared uniqueness rule refuses a duplicate code in the company: 422 code_taken on path code

- **Request:** POST /projects {"code":"Q67C9CD","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","name":"Unique two"}
- **Expected:** 422 {path: code, code: code_taken}
- **Got:** 422 {"success":false,"error":"repository error: validation failed: This value is already taken by another record","violations":[{"path":"","code":"code_taken","message":"This value is already taken by another record"}]}
- **Diagnosis:** backbone-core/src/persistence/traits.rs:194 takes the path from the DETAIL `Key (cols)=`, which Postgres omits for a table with RLS enabled (BuildIndexValueDescription), so a declared uniqueness rule on a fenced table answers path ""; the rule's own name carries no field to fall back on

### X15 — E / Transitions / a disallowed transition names a stable code

- **Request:** POST /tickets/6bf86f6d-17bd-49b8-bba4-afff5fe2dc5a/transitions/archive {}
- **Expected:** a violation or code (e.g. transition_not_allowed)
- **Got:** 400 {"success":false,"error":"Transition not allowed from current state"}
- **Diagnosis:** metaphor-plugin-schema/src/generators/handler.rs (the 400 'Transition not allowed from current state' is ApiResponse::error with no code)

### X16 — E / Transition guard / a refused guard names a stable code

- **Request:** POST /tickets/6bf86f6d-17bd-49b8-bba4-afff5fe2dc5a/transitions/resolve {}
- **Expected:** a violation with a code
- **Got:** 422 {"success":false,"error":"The resolve transition's guard does not hold: amount <= 1000"}
- **Diagnosis:** metaphor-plugin-schema/src/generators/handler.rs (guard refusal is ApiResponse::error with no code)

### X17 — E / Transitions / a machine declared in its own hook file (rules in another file for the same model) still gets its generated verbs

- **Request:** move Ticket's `states:` into schema/hooks/ticket_state.hook.yaml and generate
- **Expected:** ticket_handler.rs defines create_ticket_transition_routes and lib.rs mounts it
- **Got:** generate exit Some(0); transition routes emitted: false; mounted: false
- **Diagnosis:** metaphor-plugin-schema/src/rules/compile.rs:468-481 (mounts_generated_verbs) and the handler generator take the FIRST hook naming the model (`.find(\|h\| h.model_ref == model.name ...)`), which is the rules file with no state machine, so the verbs are silently dropped

### X18 — G / Write fence / B cannot attach its ticket to a project it cannot see (foreign keys check rows RLS hides)

- **Request:** POST /tickets {"orgUnitId":"20aaf6d0-5782-41e5-accb-d2ee3ea53401","projectId":"8aa1b846-c9bf-40e0-97a9-75e751474a0b","title":"B ticket on A project"}
- **Expected:** refused (422 reference_missing)
- **Got:** 201 {"success":true,"data":{"amount":"0","archiveReason":null,"budgetCap":null,"discount":null,"discountReason":null,"dueOn":null,"id":"c9dfbee0-fe2e-48a1-bc87-807500abeff6","metadata":{"created_at":"2026-10-17T18:09:29.694351Z","updated_at":"2026-10-17T18:09:29.694351Z"},"note":null,"orgUnitId":"20aaf6d0-5782-41e5-accb-d2ee3ea53401","projectId":"8aa1b846-c9bf-40e0-97a9-75e751474a0b","reference":"…
- **Diagnosis:** Postgres referential checks bypass RLS, and nothing in the generated guard checks the referenced row is visible (rules/fields.rs; generators/sql.rs add_declared_foreign_keys): a scoped writer can probe for and reference another company's ids

### X19 — G / Write fence / a create naming another company is refused (RLS WITH CHECK) and writes nothing

- **Request:** POST /projects {"code":"F7E25F3","companyId":"20aaf6d0-5782-41e5-accb-d2ee3ea53401","name":"Forged company"}
- **Expected:** 4xx, no row
- **Got:** 500 {"success":false,"error":"An internal error occurred. It has been logged."}; rows 0
- **Diagnosis:** Postgres raises 42501 for a WITH CHECK breach; backbone-core/src/persistence/traits.rs:108-150 has no mapping for it, so it surfaces as a 500 DatabaseError

### X20 — G / Write fence / a ticket naming another company's org unit is refused and writes nothing

- **Request:** POST /tickets {"orgUnitId":"20aaf6d0-5782-41e5-accb-d2ee3ea53401","projectId":"8aa1b846-c9bf-40e0-97a9-75e751474a0b","title":"Forged unit"}
- **Expected:** 4xx
- **Got:** 500 {"success":false,"error":"An internal error occurred. It has been logged."}
- **Diagnosis:** 42501 unmapped in backbone-core/src/persistence/traits.rs:108-150

### X21 — G / Write fence / the org kind guard refuses a ticket on the root node with a 4xx naming org_unit_id

- **Request:** POST /tickets {"orgUnitId":"ca64377b-645c-4a24-ba12-ecfa15be5805","projectId":"8aa1b846-c9bf-40e0-97a9-75e751474a0b","title":"Root unit"}
- **Expected:** 422 on org_unit_id
- **Got:** 500 {"success":false,"error":"An internal error occurred. It has been logged."}
- **Diagnosis:** the kind guard (generators/sql.rs:1238, enable_org_fence) raises a plain RAISE EXCEPTION (P0001) with no ERRCODE/CONSTRAINT, which backbone-core/src/persistence/traits.rs:150 cannot map → 500

### X22 — H / Refusals / every 400/409/422 refusal carries a stable code (a violation code)

- **Request:** 53 refusals
- **Expected:** every one coded
- **Got:** GET /projects?code[startwith]=L6C08FA&nonexistent_column=1 → {"success":false,"data":[],"meta":{"total":0,"page":0,"limit":0,"total_pages":0},"error":"Invalid query parameter or filter: repository error: Database error: c… \| PATCH /projects/bd0cf311-faa7-4d22-9793-8fcd3a06eb28 {"priority":"high"} → {"success":false,"error":"validation failed: invalid type: string \"high\", expected i32"} \| POST …
- **Diagnosis:** generated transition handlers (generators/handler.rs: ApiResponse::error without a code); backbone-core ServiceError::Validation (400, no violations); RepositoryError::AlreadyExists (409, no violations)

### X23 — H / Refusals / no refusal leaks a raw constraint, index or trigger name

- **Request:** 75 refusals scanned
- **Expected:** no ck_/uq_/idx_/tg_/fk_/duplicate key in a body
- **Got:** POST /projects {"code":"PE2EAAD","companyId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","contactEmail":"ue25d1a@example.com","name":"Project 959595"} → {"success":false,"error":"repository error: Entity already exists: duplicate key value violates unique constraint \"idx_projects_contact_email\""} \| POST /tickets {"orgUnitId":"a1c9f7f9-92d4-4c94-a317-7a9e672ad569","projectId":"1b6f4546-0574-4dca-bfb7…
- **Diagnosis:** backbone-core/src/persistence/traits.rs:125 (AlreadyExists carries the raw pg message)

### X24 — H / Refusals / a 404 names a stable code

- **Request:** GET /projects/93e30ffd-0f54-45ae-8ee1-2f16f884e31e
- **Expected:** a code such as not_found
- **Got:** 404 {"success":false,"error":"backbone_module_skeleton::domain::entity::project::Project with id '93e30ffd-0f54-45ae-8ee1-2f16f884e31e' not found"}
- **Diagnosis:** backbone-core/src/service.rs:1173 (not found is a sentence, no code)

### X25 — H / Refusals / a 404 does not leak the Rust type path

- **Request:** GET /projects/93e30ffd-0f54-45ae-8ee1-2f16f884e31e
- **Expected:** no `crate::path::Type` in the body
- **Got:** 404 {"success":false,"error":"backbone_module_skeleton::domain::entity::project::Project with id '93e30ffd-0f54-45ae-8ee1-2f16f884e31e' not found"}
- **Diagnosis:** backbone-core/src/service.rs:1173-1174 formats std::any::type_name::<E>()

### X26 — H / Diagnostics / a hook file with an `assert:` rule (no message) beside its `states:` generates, or names the offending rule

- **Request:** replace the unenforced rule's condition+message with `assert:` and generate
- **Expected:** success, or an error naming the rule / the missing `message`
- **Got:**   Error: State machine error: State machine in workflow 'Ticket' has no states \\| Error: Schema validation failed with 1 error(s)
- **Diagnosis:** metaphor-plugin-schema/src/parser/yaml_parser/parsers.rs:154-166 swallows the map-format parse error (YamlRule at parser/yaml_parser/types.rs requires `condition` and `message`) and falls back to the list-format parser, which reads no map-format states; the validator then reports "State machine in workflow 'Ticket' has no states" (resolver/validator.rs:817), naming neither the rule nor the hook

## Unsupported (6)

- **A / History / row history of an `audited: true` model** — needs backbone-auditlog composed: the generated capture trigger calls auditlog.capture_data_change (generators/data_change_audit.rs:52) and the route needs an Extension<Arc<dyn HistoryProvider>> (backbone-core/src/http.rs:276); a standalone module has neither
- **B / Field types / a `text?` field** — the validator refuses it ("Unknown type 'text' at Project.memo"): src/ast/types.rs:174 has no `text` primitive, although the skeleton's docs/schema/TYPES.md:31 documents one; `string` maps to TEXT already, so long text is covered by `string`
- **D / Declared events / `events:` in a hook file produce publishing code** — nothing is generated: commands/schema/module_loader.rs:315-331 drops them; schema doctor only warns about a declared event no hand code names (commands/schema/events_check.rs); the generic service publishes CrudEvents through the publisher the module attaches, proven above
- **D / Outbox / an event staged in the write's transaction (outbox)** — the generic service publishes fire-and-forget after the write, outside its transaction (backbone-core/src/service.rs, `let _ = self.event_publisher.publish(..)`); staging is hand code with backbone_outbox::outbox::stage, which the generator never emits
- **D / Schema lifecycle hooks / before/after create/update/delete declared as hook `triggers:` produce runtime hooks** — generation fails —   Error: Validation error: Hook 'Ticket' declares trigger 'after_create' with 1 action(s); hook triggers are not generated, so nothing runs them: move the behaviour into the service and declare the events it publishes under `events:` \\| Error: Schema validation failed with 1 error(s); the trigger target is retired (generators/mod.rs:621) and resolver/validator.rs:1916 refuses t…
- **F / Workflow / schema/workflows/ticket_escalation.workflow.yaml does something at runtime** — declaration-only: it is parsed and validated (resolver/validator.rs:55) and no generated file mentions it; the flow target is retired (generators/mod.rs:621) — the hand-written escalate route is the executable flow (proven in F)
