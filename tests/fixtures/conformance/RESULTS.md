# Conformance results

Written by `tests/conformance.rs`; do not edit by hand. Rerun with
`CONFORMANCE_DATABASE_URL=... cargo test --test conformance -- --ignored --nocapture`.

- Generator: metaphor-plugin-schema 0.36.0 (built from this checkout)
- Framework: local checkout `<workspace>/modules/backbone-framework` via [patch.crates-io]
- Generation: 2s; whole run: 44s

## Summary

| Area | PASS | FAIL | UNSUPPORTED |
|---|---|---|---|
| A. Generic routes | 57 | 0 | 1 |
| B. Validation | 42 | 0 | 0 |
| C. Custom validation | 15 | 0 | 0 |
| D. Hooks and events | 41 | 0 | 1 |
| E. State machine | 20 | 0 | 0 |
| F. Extension | 12 | 0 | 1 |
| G. Scoping | 36 | 0 | 0 |
| H. Debuggability | 23 | 0 | 0 |
| I. Permissions | 112 | 0 | 0 |
| **Total** | **358** | **0** | **3** |

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
| D | Declared events | an `on:` or `if:` naming a field, verb or state the model lacks fails generation, naming each | PASS |
| D | Schema lifecycle hooks | before/after create/update/delete declared as hook `triggers:` produce runtime hooks | UNSUPPORTED |
| D | Declared events | generation says a declared event with no `on:` is staged by hand, and names it | PASS |
| D | Declared events | events with `on:` generate their typed payloads and the write hooks that stage them | PASS |
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
| H | Rules register | machine `Ticket.status` is labelled MOVES REFUSED and the transition trigger refuses a raw UPDATE | PASS |
| H | Rules register | machine `Ticket.status` is labelled GUARDS IN GENERATED VERBS and the generated verb refuses a failing guard | PASS |
| H | Rules register | machine `Approval.state` (hand_set) is labelled MOVES REFUSED and its verbs refuse a wrong-state move | PASS |
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
| I | Register | `schema rules` lists the permission every generic route, generated verb and declared verb requires | PASS |
| I | Register | a declared system role's grants are normalised to the canonical spelling (projects.read -> project:read) | PASS |
| I | Register | `schema rules` labels each route group PERMISSION REQUIRED <permission> and says the mode (no resolver installed: AUTHCONTEXT) | PASS |
| I | Schema doctor | doctor says the mode, and warns (without failing) on the dotted spelling a role uses | PASS |
| I | Schema doctor | an unguarded hand-written route fails `schema doctor`, naming the file, the line and the fix | PASS |
| I | Schema doctor | a route marked `// unguarded: <why>` is accepted, and the doctor prints the reason | PASS |
| I | Schema doctor | a guarded verb whose permission the vocabulary does not hold fails `schema doctor` | PASS |
| I | Schema doctor | a hand-written call of the unguarded generic routes (BackboneCrudHandler::routes) fails `schema doctor` | PASS |
| I | Register | the register and the doctor label the mode RESOLVER when the service installs a grant resolver | PASS |
| I | Declarations | `requires:` naming a permission the vocabulary does not declare fails generation, naming it | PASS |
| I | Declarations | a declared `requires:` renames the permission the entity's read routes need in the generated handler | PASS |
| I | Declarations | a system role that lists a malformed permission fails generation, naming the role and the entry | PASS |

## Failures (0)

## Unsupported (3)

- **A / History / row history of an `audited: true` model** — needs backbone-auditlog composed: the generated capture trigger calls auditlog.capture_data_change (generators/data_change_audit.rs:52) and the route needs an Extension<Arc<dyn HistoryProvider>> (backbone-core/src/http.rs:276); a standalone module has neither
- **D / Schema lifecycle hooks / before/after create/update/delete declared as hook `triggers:` produce runtime hooks** — generation fails —   Error: Validation error: Hook 'Ticket' declares trigger 'after_create' with 1 action(s); hook triggers are not generated, so nothing runs them: move the behaviour into the service and declare the events it publishes under `events:` \\| Error: Schema validation failed with 1 error(s); the trigger target is retired (generators/mod.rs:621) and resolver/validator.rs:1916 refuses t…
- **F / Workflow / schema/workflows/ticket_escalation.workflow.yaml does something at runtime** — declaration-only: it is parsed and validated (resolver/validator.rs:55) and no generated file mentions it; the flow target is retired (generators/mod.rs:621) — the hand-written escalate route is the executable flow (proven in F)
