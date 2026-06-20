# Kernel-vs-private triage: sapiens (52) + bucket (14)

Companion to `schema-per-module-proposal.md`. Classifies each module model as
**kernel** (stays in `public`, shared) or **private** (moves to the module's own
schema). Editable repos:

- sapiens models: `frameworks/metaphora/backbone-sapiens/schema/models/`
- bucket models:  `frameworks/metaphora/backbone-bucket/schema/models/`
- ORM (raw `FROM {}`): `frameworks/metaphora/backbone-framework/backbone-orm/src/query_builder.rs:157`
- codegen chokepoint: `frameworks/metaphora/metaphor-plugin-schema/src/ast/model.rs:46` (`collection_name()`)

## Principle

A model is **kernel** iff another module references it — either by FK or by
**direct raw SQL** from the consumer/other modules. Direct SQL counts because
codegen cannot re-qualify it and the table must resolve from the *other* module's
pool (which only has `public` in its search_path). Everything else is **private**.

## Evidence (bersihir → sapiens, measured)

- FK references from bersihir create-table migrations: **only `users`** (14×).
- Direct raw SQL from `bersihir-service/src` into sapiens tables:
  `roles` (17), `permissions` (11), `role_permissions` (9), `users` (10),
  `user_roles` (9, incl. `INSERT INTO`), `user_permissions` (6).
- No bersihir reference (FK or SQL) to any **bucket** table.

## Decisions (locked 2026-06-20)

1. **No cross-module SQL.** Modules never query another module's tables directly —
   only via that module's API/events. bersihir's direct RBAC reads/writes get
   refactored onto the sapiens API.
2. **Shared kernel stays in `public`** (REVISED 2026-06-20 — was a dedicated
   `identity` schema). Rationale: moving the kernel out of `public` would force a
   migration-runner / DB `search_path` change so cross-module bare `REFERENCES
   users` resolves at migration time. Keeping the kernel in `public` makes those
   references resolve trivially with zero runner change. `public` *is* the shared
   kernel namespace; every module (incl. bersihir) owns a private schema for its
   non-kernel tables. The only thing given up is the cosmetic `identity` name.
3. **`notifications` de-collided by schema scoping** (no rename).

Kernel = the 6 cross-referenced tables (`users` + the 5 RBAC tables), all kept in
`public`. Everything else in sapiens becomes private.

## SAPIENS — KERNEL → stays in `public`

| models | collection | schema |
|---|---|---|
| user | users | `""` (public) |
| role, permission, role_permission, user_role, user_permission | (their tables) | `""` (public) |

All 6 kernel models override the module default with `schema: ""` (public).
Everything else becomes sapiens-private. (RBAC is in the kernel only because
bersihir still SQLs it directly; after Phase D removes that, RBAC could move to
sapiens-private — but with the kernel in `public` there is no longer any pressure
to, so this is optional.)

### Interim (until the bersihir→sapiens-API refactor lands)

bersihir currently issues direct SQL (incl. `INSERT INTO user_roles / roles /
permissions / role_permissions`) against these 5 RBAC tables, so they **cannot be
moved out of a consumer-reachable schema until that code is gone**:

`roles, permissions, role_permissions, user_roles, user_permissions`

Sequencing: keep these in `public` (kernel) now. With the kernel in `public`,
there is no pressure to move them at all; *optionally*, after Phase D makes
bersihir's RBAC access API-only, they could move to sapiens-private — but it's no
longer required. Do not move them while bersihir still SQLs them directly.

## SAPIENS — PRIVATE (→ `sapiens` schema)

Everything else — no cross-module reference found. They FK *into* the kernel
(`*.user_id → public.users` etc.), which is an allowed module→kernel cross-schema
FK. ~44 models, grouped:

- **Auth/session:** session, session_limit, email_verification, password_reset,
  password_reset_token, password_reset_security, password_reset_verification_details,
  oauth_provider, saml_provider, user_saml_link, impersonation_session, device_trust
- **MFA:** mfa_device, mfa_session, mfa_backup_code, backup_code
- **Password policy:** password_policy, password_history, password_history_settings,
  password_expiration_settings, password_requirements, password_creation_context,
  password_strength(*)
- **Authz extras (not touched cross-module):** direct_permission_grant,
  temporary_permission, resource_permission, permission_conflict,
  effective_permission_cache, organization_role, organization_permission,
  organization_user, role_assignment
- **Platform/ops:** audit_log, security_event, analytics, system_settings,
  user_settings, ldap_directory, data_export, anonymization_record, bulk_operation,
  workflow
- **Messaging:** notification (collection `notifications`),
  notification_preference (collection `sapiens_notification_preferences`)

(*) `password_strength` and `request_source` are **enums**, not tables (confirmed),
and `index` is the module index file — none generate a table, so the module-level
`schema:` default doesn't affect them.

### Bonus: `notifications` collision resolved for free

bersihir owns its own `notification` model (collection `notifications`); sapiens'
`notification` also uses `notifications`. Today that is a latent collision (same as
`notification_preferences` was). Scoping sapiens private →
`sapiens.notifications` vs bersihir `public.notifications` resolves it without a
rename. (sapiens `notification_preferences` was already renamed to
`sapiens_notification_preferences` in v0.1.16; under schema scoping the rename
becomes unnecessary but is harmless.)

## BUCKET — all PRIVATE (→ `bucket` schema)

No bersihir reference to any bucket table. All 14 move cleanly:

access_log, bucket, content_hash, conversion_job, file_comment, file_lock,
file_share, file_version, processing_job, stored_file, thumbnail, upload_session,
user_quota, index(verify).

Cross-schema FK to watch: `bucket.user_quotas.user_id → public.users` (module→kernel,
allowed). Confirm no bucket table is FK'd *from* the consumer before finalizing.

## Roadmap (respecting each repo's CLAUDE.md)

Both module repos mandate: **schema YAML is the single source of truth; edit the
model, never hand-edit generated files outside `// <<< CUSTOM` markers; regenerate.**
The ordering below is dependency-driven — each phase unblocks the next.

**Phase A — codegen foundation (`metaphor-plugin-schema`) — ✅ DONE (released 0.2.25)**
Landed the `schema:` DSL key: parser + `Model.schema` + `qualified_table_name()` /
`audit_function_name()` + generator wiring (TABLE_NAME, CREATE TABLE, qualified FK
targets, `CREATE SCHEMA IF NOT EXISTS`, qualified audit fn), keeping bare names for
index/trigger identifiers. The standalone `audit-triggers` generator is schema-aware
too (0.2.25). 7 tests added; full lib suite green (341). See the
"Implementation status" + "Known limitations" sections of
`schema-per-module-proposal.md` — tracked follow-ups: M2M join tables, the
single-schema migration-diff snapshot, legacy `.model.schema` not inheriting the
module default, and unquoted schema identifiers.

**Phase B — bucket (cleanest, do first as the pilot) — ✅ MECHANISM VERIFIED**
Verified end-to-end: a single module-level `schema: bucket` in
`index.model.yaml` qualified all 13 tables (CREATE SCHEMA + qualified
table/index/trigger/FK/down; cross-module User FK correctly not a hard FK). The
verification regen was reverted — the actual *release* (full `--target rust` regen
to qualify repository `TABLE_NAME`, `cargo check`, version bump, `search_path =
bucket, public` consumer pool) is still pending.

Note: a **module-level `schema:` key in `index.model.yaml`** now exists (precedence
per-model > file-level > module-level), so a whole module is scoped with one line.

**Phase C — sapiens private tables — ✅ MECHANISM VERIFIED (regen reverted)**
Annotated module-level `schema: sapiens` in sapiens `index.model.yaml` + overrides:
all 6 kernel models → `schema: ""` (public; `User` per-model so its file-mate
`Profile` still inherits `sapiens`). Regen census: **identity 0, sapiens 55,
public 6 (users + RBAC)** = 61 tables. Cross-schema FKs correct
(`sapiens.sessions → users` bare/public; RBAC joins → bare public). Zero
`identity.` / `sapiens.users`. `notifications` → `sapiens.notifications`
(de-collides). Verification reverted.

⚠️ **Gotcha learned — file-level vs per-model.** `user.model.yaml` defines *two*
models (`User` + `Profile`). A file-level `schema:` scopes BOTH; `Profile` must stay
`sapiens`-private, so put the kernel override as a **per-model** `schema: ""` on the
`User` entry only. Audit confirms `user.model.yaml` is the ONLY sapiens file that
straddles a schema boundary (the other 5 multi-model files are entirely sapiens).

✅ **Finding 2 resolved by decision (no runner change).** With the kernel in
`public`, cross-module bare `REFERENCES users` resolves at migration time under the
default `public` search_path — no migration-runner / DB `search_path` change needed.
Remaining for *release*: full `--target rust` regen (qualified `TABLE_NAME` for the
private tables), `cargo check`, version bump. Runtime app pool for sapiens needs
`search_path = sapiens, public` (for the hand-written raw SQL hitting `sapiens.*`).

**Phase D — kill cross-module SQL (bersihir → sapiens API) — ❌ CLOSED (not worth it)**
Investigated and closed 2026-06-20. Reasons:
1. **Driver gone.** Phase C kept the kernel shared in `public`, so RBAC is not
   moving private — the original reason to eliminate bersihir's kernel SQL is moot.
2. **Reads are cross-domain.** bersihir's capability checks join its OWN
   `provider_staffs` table (`s.role::TEXT = r.name`, with tenant role-override
   semantics) against kernel `roles`/`role_permissions`/`permissions`. sapiens
   knows nothing about `provider_staffs`, so a generic sapiens `check_permission`
   can't replicate them — they're inherently a bersihir×kernel join.
3. **Writes are transactional / bersihir-specific.** `provider_staff_service`
   (4 `begin()` blocks) and `provider_role_repository` (`*_tx` methods) create
   user+staff+roles atomically in one transaction; sapiens CRUD services own their
   own pool and can't join that transaction, so moving the writes would break
   atomicity. `user_avatar` is a bersihir-specific `jsonb_set(metadata,'{avatar_url}')`
   patch with no matching `UpdateUserDto` field.

Net: the cleanly-extractable surface is ~zero, and forcing it through an API would
cost atomicity + code for no isolation gain (kernel is shared in `public`). bersihir's
reads/writes against the shared `public` kernel are accepted as legitimate use of the
shared contract. (sapiens' `AuthorizationServiceImpl` is also unimplemented — its
`ConflictDetector`/`PermissionCalculator`/`PermissionCacheManager` collaborators have
no concrete impls — so there was no ready authz API to call anyway.)

**Phase E — move RBAC to sapiens-private — ❌ MOOT.**
Depended on Phase D + a dedicated kernel schema. Kernel stays in `public`; RBAC stays
shared. Not happening.

## Remaining real work (releases, not new design)

The isolation goal is achieved in design + verified. What's left is the actual
*release* of the verified scoping for bucket and sapiens:
- Full `--target rust` regen (qualifies repository `TABLE_NAME` for private tables),
  `cargo check`, version bump per module.
- Consumer (bersihir): per-module pool `search_path = <module>, public` (for the
  ~170 sapiens / ~30 bucket hand-written raw-SQL sites — see proposal).
- M2M join-table scoping is still a tracked codegen follow-up (see proposal).
