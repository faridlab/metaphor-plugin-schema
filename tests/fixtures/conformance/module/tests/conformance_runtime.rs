//! Runtime conformance probes, run inside the module `tests/conformance.rs` generates.
//!
//! The module's generated routes, its transition verbs and its extension routes are mounted
//! behind the framework's real stack, outermost first: `tenant_route` (resolves the tenant from
//! the host and hands its pool on), `org_auth` with a verifier bound to that tenant (proves the
//! token, resolves the org scope over the tenant's tree, binds `app.scope_unit_ids` and
//! `app.company_id` on a request-dedicated connection), and a small bridge that publishes the
//! `AuthContext` the generated transition handlers read (a composing service derives it from its
//! identity tables; here an `admin-*` subject holds every permission and any other subject none).
//!
//! The database is a scratch one created from `CONFORMANCE_DATABASE_URL`. The module's own
//! migrations build it as the owner; the app connects as a role that is neither superuser nor
//! BYPASSRLS, so row-level security is enforced. Database and role are dropped at the end, also
//! when a probe panics. Each probe is recorded as PASS, FAIL or UNSUPPORTED into the JSON file
//! named by `CONFORMANCE_RESULTS`; a failing probe never stops the run.

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::Response;
use axum::Router;
use backbone_auth::org::{org_auth, OrgClaims, OrgContext, OrgIssuer, OrgVerifier};
use backbone_auth::{resolve_grants, GrantResolver, IdentityGrantSource};
use backbone_tenant::axum_router::{tenant_audience, tenant_route, TenantRouter};
use backbone_tenant::resolve::HostResolver;
use backbone_tenant::{TenantId, TenantRegistry, TenantRuntimeFactory};
use futures::FutureExt;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_module_skeleton::ConformanceModule;

const SECRET: &[u8] = b"conformance-signing-secret";
const TENANT: &str = "acme";
const HOST: &str = "acme.conf.test";

// =============================================================================
// Recording
// =============================================================================

#[derive(Debug, Clone, serde::Serialize)]
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

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<Row>>>);

impl Rec {
    #[allow(clippy::too_many_arguments)]
    fn check(&self, area: &str, feature: &str, probe: &str, ok: bool, request: &str, expected: &str, got: &str, diagnosis: &str) -> bool {
        println!("[{}] {area} {feature} — {probe}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            println!("       request:  {request}\n       expected: {expected}\n       got:      {}", trunc(got, 600));
        }
        self.0.lock().unwrap().push(Row {
            area: area.into(),
            feature: feature.into(),
            probe: probe.into(),
            status: if ok { "PASS" } else { "FAIL" }.into(),
            request: request.into(),
            expected: expected.into(),
            got: trunc(got, 800),
            diagnosis: if ok { String::new() } else { diagnosis.into() },
        });
        ok
    }

    fn unsupported(&self, area: &str, feature: &str, probe: &str, evidence: &str) {
        println!("[UNSUPPORTED] {area} {feature} — {probe}: {evidence}");
        self.0.lock().unwrap().push(Row {
            area: area.into(),
            feature: feature.into(),
            probe: probe.into(),
            status: "UNSUPPORTED".into(),
            request: String::new(),
            expected: String::new(),
            got: evidence.into(),
            diagnosis: String::new(),
        });
    }

    fn rows(&self) -> Vec<Row> {
        self.0.lock().unwrap().clone()
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}

// =============================================================================
// HTTP
// =============================================================================

#[derive(Debug, Clone)]
struct Resp {
    status: u16,
    body: Value,
    raw: String,
    headers: HeaderMap,
    request: String,
}

impl Resp {
    fn got(&self) -> String {
        format!("{} {}", self.status, trunc(&self.raw, 500))
    }
    fn data(&self) -> &Value {
        &self.body["data"]
    }
    fn violations(&self) -> Vec<Value> {
        self.body["violations"].as_array().cloned().unwrap_or_default()
    }
    fn violation(&self, path: &str, code: &str) -> bool {
        self.violations()
            .iter()
            .any(|v| v["path"].as_str() == Some(path) && v["code"].as_str() == Some(code))
    }
    fn has_code(&self, code: &str) -> bool {
        self.violations().iter().any(|v| v["code"].as_str() == Some(code))
    }
    fn items(&self) -> Vec<Value> {
        self.body["data"].as_array().cloned().unwrap_or_default()
    }
    fn ids(&self) -> Vec<String> {
        self.items().iter().filter_map(|i| i["id"].as_str().map(str::to_string)).collect()
    }
    fn id(&self) -> String {
        self.data()["id"].as_str().unwrap_or_default().to_string()
    }
}

#[derive(Clone)]
struct App {
    router: Router,
    /// Every 4xx answer the probes received, for the debuggability sweep.
    refusals: Arc<Mutex<Vec<Resp>>>,
}

impl App {
    async fn send(&self, method: &str, path: &str, token: Option<&str>, body: Option<String>, extra: &[(&str, &str)]) -> Resp {
        let mut b = Request::builder().method(method).uri(path).header("host", HOST);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        let request = format!("{method} {path}{}", body.as_deref().map(|s| format!(" {}", trunc(s, 300))).unwrap_or_default());
        let req = match body {
            Some(text) => b.header("content-type", "application/json").body(Body::from(text)).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        // A handler that panics would take the probe run down; a server's connection task would
        // die the same way. Record it as status 0 and carry on.
        let res = match AssertUnwindSafe(self.router.clone().oneshot(req)).catch_unwind().await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => match e {},
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
                return Resp { status: 0, body: Value::Null, raw: format!("the handler panicked: {msg}"), headers: HeaderMap::new(), request };
            }
        };
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
        let raw = String::from_utf8_lossy(&bytes).to_string();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let resp = Resp { status, body, raw, headers, request };
        if (400..500).contains(&status) {
            self.refusals.lock().unwrap().push(resp.clone());
        }
        resp
    }
    async fn get(&self, path: &str, token: &str) -> Resp {
        self.send("GET", path, Some(token), None, &[]).await
    }
    async fn post(&self, path: &str, token: &str, body: Value) -> Resp {
        self.send("POST", path, Some(token), Some(body.to_string()), &[]).await
    }
    async fn put(&self, path: &str, token: &str, body: Value) -> Resp {
        self.send("PUT", path, Some(token), Some(body.to_string()), &[]).await
    }
    async fn patch(&self, path: &str, token: &str, body: Value) -> Resp {
        self.send("PATCH", path, Some(token), Some(body.to_string()), &[]).await
    }
    async fn delete(&self, path: &str, token: &str) -> Resp {
        self.send("DELETE", path, Some(token), None, &[]).await
    }
    async fn delete_body(&self, path: &str, token: &str, body: Value) -> Resp {
        self.send("DELETE", path, Some(token), Some(body.to_string()), &[]).await
    }
}

// =============================================================================
// Stack
// =============================================================================

/// The tenant runtime factory: one tenant, the scratch database's app pool.
struct Factory {
    pool: PgPool,
}

#[async_trait::async_trait]
impl TenantRuntimeFactory for Factory {
    type Runtime = PgPool;
    type Error = std::io::Error;
    async fn build(&self, tenant: &TenantId) -> Result<PgPool, std::io::Error> {
        if tenant.as_str() == TENANT {
            Ok(self.pool.clone())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "unknown tenant"))
        }
    }
}

/// The default stack's `AuthContext`: what a host with no grant resolver installed produces from its
/// own authentication (`admin-*` holds `*:*`, `reader-*` holds `ticket:read`, anyone else nothing).
/// Every probe outside area I runs behind it, so the generic-route probes run fail-closed: a caller
/// holding no permission is refused.
async fn auth_bridge(mut req: Request, next: Next) -> Response {
    if let Some(org) = req.extensions().get::<OrgContext>().cloned() {
        let permissions = if org.user_id.starts_with("admin") {
            vec!["*:*".to_string()]
        } else if org.user_id.starts_with("reader") {
            vec!["ticket:read".to_string()]
        } else {
            Vec::new()
        };
        req.extensions_mut().insert(backbone_auth::middleware::AuthContext {
            user_id: org.user_id,
            roles: Vec::new(),
            permissions,
        });
    }
    next.run(req).await
}

/// A gate that admitted the request on a narrower rule than a permission says so.
async fn waive(mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(backbone_auth::PermissionWaiver);
    next.run(req).await
}

fn with_db(dsn: &str, db: &str) -> String {
    let (base, query) = match dsn.split_once('?') {
        Some((b, q)) => (b, format!("?{q}")),
        None => (dsn, String::new()),
    };
    let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    let path_start = base[scheme_end..].find('/').map(|i| i + scheme_end).unwrap_or(base.len());
    format!("{}/{db}{query}", &base[..path_start])
}

fn with_user(dsn: &str, user: &str, password: &str) -> String {
    let scheme_end = dsn.find("://").map(|i| i + 3).unwrap_or(0);
    let rest = &dsn[scheme_end..];
    let host = match rest.find('@') {
        Some(at) if rest.find('/').map_or(true, |slash| at < slash) => &rest[at + 1..],
        _ => rest,
    };
    format!("{}{user}:{password}@{host}", &dsn[..scheme_end])
}

struct Org {
    root: Uuid,
    company_a: Uuid,
    branch_a: Uuid,
    company_b: Uuid,
}

const SPINE: &[&str] = &[
    "CREATE SCHEMA IF NOT EXISTS organization",
    "CREATE TABLE organization.org_units (id uuid PRIMARY KEY, kind text NOT NULL, parent_id uuid, code text, name text NOT NULL, metadata jsonb NOT NULL DEFAULT '{}')",
    "CREATE UNIQUE INDEX one_root ON organization.org_units (kind) WHERE kind = 'root'",
    "CREATE OR REPLACE FUNCTION organization.org_unit_subtree(p_roots uuid[]) RETURNS SETOF uuid LANGUAGE sql STABLE AS $$ \
       WITH RECURSIVE tree AS (SELECT o.id FROM organization.org_units o WHERE o.id = ANY(p_roots) \
       UNION ALL SELECT o.id FROM organization.org_units o JOIN tree t ON o.parent_id = t.id) SELECT id FROM tree $$",
    "CREATE OR REPLACE FUNCTION organization.org_unit_root() RETURNS uuid LANGUAGE sql STABLE AS $$ \
       SELECT id FROM organization.org_units WHERE kind = 'root' $$",
];

// =============================================================================
// Entry
// =============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conformance_runtime() {
    let admin_dsn = std::env::var("CONFORMANCE_DATABASE_URL").expect("CONFORMANCE_DATABASE_URL");
    let results = std::env::var("CONFORMANCE_RESULTS").unwrap_or_else(|_| "conformance-results.json".into());
    let suffix = Uuid::new_v4().simple().to_string()[..8].to_string();
    let db = format!("conformance_{suffix}");
    let role = format!("conformance_app_{suffix}");
    let password = format!("pw{suffix}");
    let rec = Rec::default();

    let admin = PgPoolOptions::new().max_connections(2).connect(&admin_dsn).await.expect("admin connection");
    sqlx::query(&format!("CREATE DATABASE {db}")).execute(&admin).await.expect("create database");
    sqlx::query(&format!("CREATE ROLE {role} LOGIN PASSWORD '{password}' NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE"))
        .execute(&admin)
        .await
        .expect("create role");

    let outcome = AssertUnwindSafe(run(&admin_dsn, &db, &role, &password, rec.clone())).catch_unwind().await;
    if let Err(panic) = outcome {
        let msg = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic".into());
        rec.check("A", "Harness", "the probe run completed without a panic", false, "", "no panic", &msg, "see the panic message");
    }

    // Teardown, also after a panic: the database (forcing out pooled connections) and the role.
    let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")).execute(&admin).await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS {role}")).execute(&admin).await;
    admin.close().await;

    let rows = rec.rows();
    std::fs::write(&results, serde_json::to_string_pretty(&rows).unwrap()).expect("write results");
    let failed = rows.iter().filter(|r| r.status == "FAIL").count();
    println!("conformance runtime: {} probes, {failed} failed; results in {results}", rows.len());
}

async fn run(admin_dsn: &str, db: &str, role: &str, password: &str, rec: Rec) {
    let owner_dsn = with_db(admin_dsn, db);
    let owner = PgPoolOptions::new().max_connections(3).connect(&owner_dsn).await.expect("owner connection");

    for stmt in SPINE {
        sqlx::raw_sql(stmt).execute(&owner).await.expect("org spine");
    }
    // The identity module's tables, which the module's seed migration writes into (a composing
    // service runs the identity migrations first).
    for stmt in IDENTITY_DDL {
        sqlx::raw_sql(stmt).execute(&owner).await.expect("identity tables");
    }

    // The module's migrations, in file order, as the owner.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".up.sql"))
        .collect();
    files.sort();
    let mut migration_errors = Vec::new();
    for f in &files {
        let sql = std::fs::read_to_string(f).unwrap();
        if let Err(e) = sqlx::raw_sql(&sql).execute(&owner).await {
            migration_errors.push(format!("{}: {e}", f.file_name().unwrap().to_string_lossy()));
        }
    }
    rec.check(
        "A",
        "Migrations",
        "every generated up-migration applies to an empty database",
        migration_errors.is_empty(),
        &format!("{} files under migrations/", files.len()),
        "all apply",
        &migration_errors.join("; "),
        "the failing migration file (generators/sql.rs)",
    );

    // The outbox and inbox tables of the module's staged events, created as the composing service
    // does at startup with a role that may create tables.
    backbone_module_skeleton::domain::event::declared::migrate_outbox(&owner).await.expect("migrate the outbox");

    // The app role, as scripts/rls_app_role.sql grants it.
    for stmt in [
        format!("GRANT CONNECT ON DATABASE {db} TO {role}"),
        format!("GRANT USAGE ON SCHEMA conformance, organization TO {role}"),
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA conformance TO {role}"),
        format!("GRANT SELECT ON ALL TABLES IN SCHEMA organization TO {role}"),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA conformance TO {role}"),
        format!("GRANT USAGE ON SCHEMA sapiens TO {role}"),
        format!("GRANT SELECT ON sapiens.organization_users, sapiens.direct_permission_grants, public.roles, public.permissions, public.role_permissions, public.user_roles, public.user_permissions TO {role}"),
    ] {
        sqlx::query(&stmt).execute(&owner).await.expect("grant");
    }

    let org = Org { root: Uuid::new_v4(), company_a: Uuid::new_v4(), branch_a: Uuid::new_v4(), company_b: Uuid::new_v4() };
    sqlx::query(
        "INSERT INTO organization.org_units (id, kind, parent_id, code, name) VALUES \
         ($1,'root',NULL,'ROOT','Root'), ($2,'company',$1,'CA','Company A'), ($3,'branch',$2,'BA','Branch A'), ($4,'company',$1,'CB','Company B')",
    )
    .bind(org.root)
    .bind(org.company_a)
    .bind(org.branch_a)
    .bind(org.company_b)
    .execute(&owner)
    .await
    .expect("seed org tree");

    // A probe of which columns an UPDATE writes: `BEFORE UPDATE OF summary` fires whenever the
    // statement's SET list names `summary`, changed or not.
    for stmt in [
        "CREATE TABLE conformance.update_probe (id bigserial PRIMARY KEY, project_id uuid, at timestamptz DEFAULT now())".to_string(),
        "CREATE FUNCTION conformance.update_probe_fn() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER AS $$ \
           BEGIN INSERT INTO conformance.update_probe (project_id) VALUES (NEW.id); RETURN NEW; END $$"
            .to_string(),
        "CREATE TRIGGER projects_update_probe BEFORE UPDATE OF summary ON conformance.projects \
           FOR EACH ROW EXECUTE FUNCTION conformance.update_probe_fn()"
            .to_string(),
    ] {
        sqlx::raw_sql(&stmt).execute(&owner).await.expect("update probe");
    }

    let app_dsn = with_user(&owner_dsn, role, password);
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&app_dsn)
        .await
        .expect("app connection");

    let (superuser, bypass): (bool, bool) =
        sqlx::query_as("SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user")
            .fetch_one(&pool)
            .await
            .unwrap();
    rec.check(
        "G",
        "RLS",
        "the app connects as a role that is neither superuser nor BYPASSRLS",
        !superuser && !bypass,
        "SELECT rolsuper, rolbypassrls",
        "false, false",
        &format!("{superuser}, {bypass}"),
        "harness role setup",
    );

    let module = Arc::new(ConformanceModule::builder().with_database(pool.clone()).build().expect("module builds"));
    let verifier = OrgVerifier::hs256(SECRET).bind_audience(tenant_audience);
    let routes = || module.all_crud_routes().merge(module.transition_routes()).merge(module.extension_routes());
    let tenant_pool = pool.clone();
    // The stack every variant shares: the tenant router outermost, then `org_auth`.
    let stack = |inner: Router| {
        let tenant_router = TenantRouter::new(HostResolver::new("conf.test"), TenantRegistry::new(Factory { pool: tenant_pool.clone() }, 4));
        inner
            .route_layer(from_fn_with_state(verifier.clone(), org_auth))
            .layer(from_fn_with_state(tenant_router, tenant_route))
    };
    // The default stack: the host's own `AuthContext`, no grant resolver.
    let router = stack(routes().route_layer(from_fn(auth_bridge)));
    // A resolver installed: grants are read from the identity tables once per request.
    let resolver = GrantResolver::new(IdentityGrantSource::new(pool.clone()));
    let resolved = stack(routes().route_layer(from_fn_with_state(resolver, resolve_grants)));
    // Nothing produces an `AuthContext`.
    let bare = stack(routes());
    // A gate that admits on a narrower rule marks the request.
    let waived = stack(routes().route_layer(from_fn(waive)).route_layer(from_fn(auth_bridge)));
    // Routes mounted with no authentication layer at all (the tenant router only), as a service that
    // mounted `public_routes()` outside its auth layer would; and the module's `public_routes()`
    // alone, mounted the same way.
    let unauthenticated = |inner: Router| {
        let tenant_router = TenantRouter::new(HostResolver::new("conf.test"), TenantRegistry::new(Factory { pool: tenant_pool.clone() }, 4));
        inner.layer(from_fn_with_state(tenant_router, tenant_route))
    };
    let open = unauthenticated(routes());
    let public_only = unauthenticated(module.public_routes());
    let new_app = |router: Router| App { router, refusals: Arc::new(Mutex::new(Vec::new())) };
    let app = new_app(router);
    let (rapp, bare, waived) = (new_app(resolved), new_app(bare), new_app(waived));
    let (open, public_only) = (new_app(open), new_app(public_only));

    let issuer = OrgIssuer::hs256(SECRET).for_audience(TENANT);
    let hour = Duration::from_secs(3600);
    let ta = issuer.issue_access("admin-a", org.company_a, &[], Some(org.company_a), hour).unwrap();
    let tb = issuer.issue_access("admin-b", org.company_b, &[], Some(org.company_b), hour).unwrap();
    let clerk = issuer.issue_access("clerk-a", org.company_a, &[], Some(org.company_a), hour).unwrap();

    let ctx = Ctx { app, rapp, bare, waived, open, public_only, rec: rec.clone(), org, ta, tb, clerk, owner: owner.clone(), pool: pool.clone(), module: module.clone(), issuer };

    // Each area runs isolated: a panic in one is recorded and the next still runs.
    for (name, fut) in [
        ("A", AssertUnwindSafe(area_a(&ctx)).boxed()),
        ("B", AssertUnwindSafe(area_b(&ctx)).boxed()),
        ("C", AssertUnwindSafe(area_c(&ctx)).boxed()),
        ("D", AssertUnwindSafe(area_d(&ctx)).boxed()),
        ("E", AssertUnwindSafe(area_e(&ctx)).boxed()),
        ("F", AssertUnwindSafe(area_f(&ctx)).boxed()),
        ("F", AssertUnwindSafe(area_f_workflows(&ctx)).boxed()),
        ("G", AssertUnwindSafe(area_g(&ctx)).boxed()),
        ("I", AssertUnwindSafe(area_i(&ctx)).boxed()),
        ("I", AssertUnwindSafe(area_i_public(&ctx)).boxed()),
    ] {
        if let Err(panic) = AssertUnwindSafe(fut).catch_unwind().await {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            rec.check(name, "Harness", "the area's probes ran to the end", false, "", "no panic", &msg, "harness or a framework panic; see the message");
        }
    }
    area_h(&ctx).await;

    pool.close().await;
    owner.close().await;
}

struct Ctx {
    app: App,
    /// The same routes behind a grant resolver reading the scratch identity tables.
    rapp: App,
    /// The same routes with nothing producing an `AuthContext`.
    bare: App,
    /// The same routes behind the default stack, with a gate that waives the permission check.
    waived: App,
    /// Every route of the module mounted with no authentication layer at all.
    open: App,
    /// The module's `public_routes()` alone, mounted with no authentication layer.
    public_only: App,
    rec: Rec,
    org: Org,
    ta: String,
    tb: String,
    clerk: String,
    owner: PgPool,
    pool: PgPool,
    module: Arc<ConformanceModule>,
    issuer: OrgIssuer,
}

fn uniq(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::new_v4().simple().to_string()[..6].to_uppercase())
}

impl Ctx {
    fn project_body(&self, company: Uuid, code: &str, name: &str) -> Value {
        json!({ "companyId": company, "code": code, "name": name })
    }
    async fn project(&self, token: &str, company: Uuid, extra: Value) -> Resp {
        let mut body = self.project_body(company, &uniq("P"), &format!("Project {}", uniq("")));
        if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        self.app.post("/projects", token, body).await
    }
    async fn ticket(&self, token: &str, unit: Uuid, project: &str, extra: Value) -> Resp {
        let mut body = json!({ "orgUnitId": unit, "projectId": project, "title": format!("Ticket {}", uniq("")) });
        if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        self.app.post("/tickets", token, body).await
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

// =============================================================================
// A. Generic routes
// =============================================================================

async fn area_a(c: &Ctx) {
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const CRUD: &str = "backbone-core/src/http.rs (BackboneCrudHandler)";

    // create, get, unknown, malformed
    let created = c.project(ta, c.org.company_a, json!({ "summary": "first" })).await;
    r.check("A", "Create", "POST /projects answers 201 with the stored record", created.status == 201 && !created.id().is_empty(), &created.request, "201, data.id", &created.got(), CRUD);
    let id = created.id();
    let got = app.get(&format!("/projects/{id}"), ta).await;
    r.check("A", "Get", "GET /projects/:id answers 200 with the record", got.status == 200 && got.data()["id"] == json!(id), &got.request, "200, same id", &got.got(), CRUD);
    let unknown = app.get(&format!("/projects/{}", Uuid::new_v4()), ta).await;
    r.check("A", "Get", "GET an unknown id answers 404", unknown.status == 404, &unknown.request, "404", &unknown.got(), CRUD);
    let bad = app.get("/projects/not-a-uuid", ta).await;
    r.check("A", "Get", "GET a malformed id answers 400 invalid_id on path id", bad.status == 400 && bad.violation("id", "invalid_id"), &bad.request, "400, violation {path: id, code: invalid_id}", &bad.got(), "backbone-core/src/http.rs (parse_id)");

    // a small population for the list probes
    let tag = uniq("L");
    let mut listed = Vec::new();
    for (i, (prio, kind, site)) in [(1, "internal", None), (2, "research", None), (3, "client", Some("https://c.example")), (4, "internal", None), (5, "research", None)].iter().enumerate() {
        let mut extra = json!({ "code": format!("{tag}{i}"), "name": format!("{tag} item {i} {}", if i == 2 { "Alpha" } else { "beta" }), "priority": prio, "kind": kind, "budget": (i as i64) * 100 });
        if let Some(s) = site {
            extra["website"] = json!(s);
        }
        let p = c.project(ta, c.org.company_a, extra).await;
        if p.status != 201 {
            r.check("A", "List", "list fixture rows are created", false, &p.request, "201", &p.got(), "harness");
        }
        listed.push(p.id());
    }
    let q = |s: &str| format!("/projects?code[startwith]={tag}&{s}");
    let lst = app.get(&format!("/projects?code[startwith]={tag}"), ta).await;
    r.check("A", "List", "GET /projects lists with data[] and meta.total", lst.status == 200 && lst.items().len() == 5 && lst.body["meta"]["total"] == json!(5), &lst.request, "200, 5 items, meta.total 5", &lst.got(), CRUD);

    let f = app.get(&q("priority[gte]=4"), ta).await;
    r.check("A", "List filters", "operator filter priority[gte]=4", f.status == 200 && f.items().len() == 2, &f.request, "2 items", &f.got(), "backbone-orm/src/filter/parser.rs");
    let f = app.get(&q("kind=client"), ta).await;
    r.check("A", "List filters", "equality filter kind=client (an enum column)", f.status == 200 && f.items().len() == 1, &f.request, "1 item", &f.got(), "backbone-orm/src/filter/parser.rs");
    let f = app.get(&q("priority[in]=1,2"), ta).await;
    r.check("A", "List filters", "priority[in]=1,2", f.status == 200 && f.items().len() == 2, &f.request, "2 items", &f.got(), "backbone-orm/src/filter/parser.rs");
    let f = app.get(&q("priority[between]=2,4"), ta).await;
    r.check("A", "List filters", "priority[between]=2,4", f.status == 200 && f.items().len() == 3, &f.request, "3 items", &f.got(), "backbone-orm/src/filter/parser.rs");
    let f = app.get(&q("website[isnotnull]=1"), ta).await;
    r.check("A", "List filters", "website[isnotnull]", f.status == 200 && f.items().len() == 1, &f.request, "1 item", &f.got(), "backbone-orm/src/filter/parser.rs");
    let f = app.get(&q("priority[ne]=3&billable=true"), ta).await;
    r.check("A", "List filters", "two filters AND together: priority[ne]=3&billable=true", f.status == 200 && f.items().len() == 4, &f.request, "4 items", &f.got(), "backbone-orm/src/filter/parser.rs");
    let f = app.get(&q("priority=1&priority[or]=5"), ta).await;
    r.check("A", "List filters", "OR filter priority=1&priority[or]=5", f.status == 200 && f.items().len() == 2, &f.request, "2 items (priority 1 or 5)", &f.got(), "backbone-orm/src/filter/parser.rs ([or])");
    let f = app.get(&q("nonexistent_column=1"), ta).await;
    r.check("A", "List filters", "a filter on an unknown column answers 400, not 500", f.status == 400, &f.request, "400", &f.got(), "backbone-core/src/http.rs (is_bad_query_error)");

    let prios = |r: &Resp| r.items().iter().map(|i| i["priority"].as_i64().unwrap_or(-1)).collect::<Vec<_>>();
    let s = app.get(&q("sort=-priority"), ta).await;
    r.check("A", "List sort", "sort=-priority orders descending", s.status == 200 && prios(&s) == vec![5, 4, 3, 2, 1], &s.request, "[5,4,3,2,1]", &format!("{:?} {}", prios(&s), s.status), "backbone-orm/src/filter/parser.rs (sort)");
    let s = app.get(&q("orderby=priority:desc"), ta).await;
    r.check("A", "List sort", "orderby=priority:desc orders descending", s.status == 200 && prios(&s) == vec![5, 4, 3, 2, 1], &s.request, "[5,4,3,2,1]", &format!("{:?} {}", prios(&s), s.status), "backbone-orm/src/filter/parser.rs (sort)");
    let s = app.get(&q("sort_by=priority&sort_order=desc"), ta).await;
    r.check("A", "List sort", "the typed sort_by/sort_order parameters order the list", s.status == 200 && prios(&s) == vec![5, 4, 3, 2, 1], &s.request, "[5,4,3,2,1]", &format!("{:?} {}", prios(&s), s.status), "backbone-core/src/http.rs:393-408 repository_filters drops sort_by/sort_order (ListQueryParams declares them at http.rs:220-240)");

    let s = app.get(&q("search=Alpha"), ta).await;
    r.check("A", "List search", "search=Alpha matches the name", s.status == 200 && s.items().len() == 1, &s.request, "1 item", &s.got(), "backbone-orm generic_repository search_fields");

    let p = app.get(&q("sort=priority&limit=2&page=2"), ta).await;
    r.check("A", "List paging", "offset paging limit=2&page=2 gives the 3rd and 4th rows", p.status == 200 && prios(&p) == vec![3, 4] && p.body["meta"]["page"] == json!(2) && p.body["meta"]["total"] == json!(5), &p.request, "priorities [3,4], meta.page 2, total 5", &p.got(), CRUD);
    let p = app.get(&q("limit=1000"), ta).await;
    let lim = p.body["meta"]["limit"].as_i64().unwrap_or(-1);
    r.check("A", "List paging", "an oversized limit is clamped to the page cap (100)", p.status == 200 && lim <= 100 && lim > 0, &p.request, "meta.limit <= 100", &format!("meta.limit {lim}; {}", p.status), "backbone-core/src/macros.rs:221 builds PaginationParams as a struct literal, bypassing PaginationParams::new's clamp");
    let p = app.get(&q("page=0"), ta).await;
    r.check("A", "List paging", "page=0 answers 200 or 400, never 5xx", p.status == 200 || p.status == 400, &p.request, "200 or 400", &p.got(), "backbone-orm/src/repository.rs:48 offset() underflows (page - 1) for page 0");

    let k1 = app.get(&q("sort=priority&limit=2"), ta).await;
    let cursor = k1.body["meta"]["next_cursor"].as_str().map(str::to_string);
    r.check("A", "List keyset", "a sorted page hands out meta.next_cursor", k1.status == 200 && cursor.is_some(), &k1.request, "meta.next_cursor present", &k1.got(), "backbone-orm/src/repository.rs (keyset)");
    if let Some(cur) = cursor {
        let k2 = app.get(&q(&format!("sort=priority&limit=2&after={cur}")), ta).await;
        r.check("A", "List keyset", "after=<cursor> continues with the next rows", k2.status == 200 && prios(&k2) == vec![3, 4], &k2.request, "priorities [3,4]", &format!("{:?}; {}", prios(&k2), k2.got()), "backbone-orm/src/repository.rs (keyset)");
    }
    let e = app.get(&q("estimate=1"), ta).await;
    r.check("A", "List count modes", "estimate=1 counts exactly on a small table", e.status == 200 && e.body["meta"]["total"] == json!(5), &e.request, "meta.total 5", &e.got(), "backbone-orm/src/repository.rs (estimate)");

    let fl = app.get(&q("fields=code,name"), ta).await;
    let keys: Vec<String> = fl.items().first().and_then(|i| i.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default();
    let mut sorted = keys.clone();
    sorted.sort();
    r.check("A", "List fields", "fields=code,name keeps only id, code, name", fl.status == 200 && sorted == vec!["code", "id", "name"], &fl.request, "[code, id, name]", &format!("{sorted:?}"), "backbone-core/src/http.rs (fields projection)");

    // include= over the Ticket → Project foreign key
    let t = c.ticket(ta, c.org.company_a, &id, json!({})).await;
    let inc = app.get(&format!("/tickets?id={}&include=project", t.id()), ta).await;
    let hydrated = inc.items().first().map(|i| i["project"]["id"] == json!(id)).unwrap_or(false);
    r.check("A", "List include", "include=project hydrates the referenced project", inc.status == 200 && hydrated, &inc.request, "items[0].project.id == the project", &inc.got(), "backbone-core/src/http.rs:631-675 (include)");

    // PUT, PATCH
    let full = json!({ "companyId": c.org.company_a, "code": created.data()["code"], "name": "Renamed by PUT", "summary": "put summary", "priority": 2, "budget": 0, "billable": true, "kind": "internal" });
    let put = app.put(&format!("/projects/{id}"), ta, full).await;
    r.check("A", "Update", "PUT /projects/:id replaces the record", put.status == 200 && put.data()["name"] == json!("Renamed by PUT"), &put.request, "200, name changed", &put.got(), CRUD);

    let probe_count = |c: &Ctx, id: String| {
        let owner = c.owner.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conformance.update_probe WHERE project_id = $1::uuid")
                .bind(id)
                .fetch_one(&owner)
                .await
                .unwrap_or(-1)
        }
    };
    let before = probe_count(c, id.clone()).await;
    let pa = app.patch(&format!("/projects/{id}"), ta, json!({ "name": "Renamed by PATCH" })).await;
    let after = probe_count(c, id.clone()).await;
    r.check("A", "Patch", "PATCH writes only the columns it changes (summary is not in the SET list)", pa.status == 200 && pa.data()["name"] == json!("Renamed by PATCH") && after == before, &pa.request, "200; the BEFORE UPDATE OF summary trigger did not fire", &format!("{}; summary trigger fired {} time(s)", pa.got(), after - before), "backbone-orm/src/repository.rs:760-855 (update_changed)");
    r.check("A", "Patch", "an absent key keeps its stored value", pa.data()["summary"] == json!("put summary") && pa.data()["priority"] == json!(2), &pa.request, "summary and priority unchanged", &pa.got(), "backbone-core/src/service.rs (merge_patch)");
    let pn = app.patch(&format!("/projects/{id}"), ta, json!({ "summary": null })).await;
    let stored: Option<String> = sqlx::query_scalar("SELECT summary FROM conformance.projects WHERE id = $1").bind(Uuid::parse_str(&id).unwrap()).fetch_one(&c.owner).await.unwrap_or(Some("?".into()));
    r.check("A", "Patch", "an explicit null clears a nullable column", pn.status == 200 && pn.data()["summary"].is_null() && stored.is_none(), &pn.request, "200, summary null in the response and the row", &format!("{}; stored {stored:?}", pn.got()), "backbone-core/src/service.rs (merge_patch)");

    // delete, trash, restore, permanent
    let d = app.delete(&format!("/projects/{id}"), ta).await;
    r.check("A", "Soft delete", "DELETE /projects/:id soft-deletes (200)", d.status == 200, &d.request, "200", &d.got(), CRUD);
    let g = app.get(&format!("/projects/{id}"), ta).await;
    r.check("A", "Soft delete", "a soft-deleted record is gone from GET (404)", g.status == 404, &g.request, "404", &g.got(), CRUD);
    let row_kept: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.projects WHERE id = $1 AND metadata->>'deleted_at' IS NOT NULL").bind(Uuid::parse_str(&id).unwrap()).fetch_one(&c.owner).await.unwrap_or(0);
    r.check("A", "Soft delete", "the row stays with metadata.deleted_at set", row_kept == 1, "SELECT ... metadata->>'deleted_at'", "1 row", &row_kept.to_string(), "backbone-orm soft delete");
    let tr = app.get("/projects/trash", ta).await;
    r.check("A", "Trash", "GET /projects/trash lists it", tr.status == 200 && tr.ids().contains(&id), &tr.request, "contains the id", &tr.got(), CRUD);
    let trf = app.get(&format!("/projects/trash?code={}", created.data()["code"].as_str().unwrap_or("")), ta).await;
    r.check("A", "Trash", "the trash list takes filters", trf.status == 200 && trf.ids() == vec![id.clone()], &trf.request, "exactly the id", &trf.got(), CRUD);
    let tc = app.get("/projects/trash/count", ta).await;
    r.check("A", "Trash", "GET /projects/trash/count counts it", tc.status == 200 && tc.data()["count"].as_i64().unwrap_or(0) >= 1, &tc.request, "count >= 1", &tc.got(), CRUD);
    let gd = app.get(&format!("/projects/{id}/deleted"), ta).await;
    r.check("A", "Trash", "GET /projects/:id/deleted reads the trashed record", gd.status == 200 && gd.data()["id"] == json!(id), &gd.request, "200", &gd.got(), CRUD);
    let rs = app.post(&format!("/projects/{id}/restore"), ta, json!({})).await;
    let g = app.get(&format!("/projects/{id}"), ta).await;
    r.check("A", "Restore", "POST /projects/:id/restore brings it back", rs.status == 200 && g.status == 200, &rs.request, "200 then GET 200", &format!("{} / GET {}", rs.got(), g.status), CRUD);

    let (x, y) = (c.project(ta, c.org.company_a, json!({})).await.id(), c.project(ta, c.org.company_a, json!({})).await.id());
    app.delete(&format!("/projects/{x}"), ta).await;
    app.delete(&format!("/projects/{y}"), ta).await;
    let ra = app.post("/projects/restore/all", ta, json!({})).await;
    let back = app.get(&format!("/projects/{x}"), ta).await.status == 200 && app.get(&format!("/projects/{y}"), ta).await.status == 200;
    r.check("A", "Restore", "POST /projects/restore/all restores the whole trash", ra.status == 200 && back, &ra.request, "200, both back", &ra.got(), CRUD);

    app.delete(&format!("/projects/{x}"), ta).await;
    let et = app.delete("/projects/empty", ta).await;
    let left = app.get("/projects/trash/count", ta).await;
    r.check("A", "Empty trash", "DELETE /projects/empty purges the trash", et.status == 200 && et.data()["deleted_count"].as_i64().unwrap_or(0) >= 1 && left.data()["count"] == json!(0), &et.request, "deleted_count >= 1, trash count 0", &format!("{} / {}", et.got(), left.got()), CRUD);

    app.delete(&format!("/projects/{y}"), ta).await;
    let pd = app.delete(&format!("/projects/trash/{y}"), ta).await;
    let gone = app.get(&format!("/projects/{y}/deleted"), ta).await;
    r.check("A", "Permanent delete", "DELETE /projects/trash/:id removes it for good (204)", pd.status == 204 && gone.status == 404, &pd.request, "204, then /deleted 404", &format!("{} / {}", pd.got(), gone.status), CRUD);

    // bulk
    let bc = app.post("/projects/bulk", ta, json!([c.project_body(c.org.company_a, &uniq("B"), "Bulk one"), c.project_body(c.org.company_a, &uniq("B"), "Bulk two")])).await;
    let bids: Vec<String> = bc.data()["items"].as_array().cloned().unwrap_or_default().iter().filter_map(|i| i["id"].as_str().map(str::to_string)).collect();
    r.check("A", "Bulk", "POST /projects/bulk creates every item (201)", bc.status == 201 && bids.len() == 2, &bc.request, "201, 2 items", &bc.got(), "backbone-core/src/http.rs (bulk create)");
    let bp = app.patch("/projects/bulk", ta, json!({ "ids": bids, "patch": { "priority": 5 } })).await;
    let mut both5 = true;
    for i in &bids {
        both5 &= app.get(&format!("/projects/{i}"), ta).await.data()["priority"] == json!(5);
    }
    r.check("A", "Bulk", "PATCH /projects/bulk patches every id", bp.status == 200 && both5, &bp.request, "200, priority 5 on both", &bp.got(), "backbone-core/src/http.rs (bulk patch)");
    let bput = app.put("/projects/bulk", ta, json!(bids.iter().map(|i| json!({ "id": i, "companyId": c.org.company_a, "code": uniq("B"), "name": "Bulk put", "budget": 0, "priority": 3, "billable": true, "kind": "internal" })).collect::<Vec<_>>())).await;
    r.check("A", "Bulk", "PUT /projects/bulk replaces every item", bput.status == 200, &bput.request, "200", &bput.got(), "backbone-core/src/http.rs (bulk put)");
    let bd = app.post("/projects/delete/bulk", ta, json!({ "ids": bids })).await;
    r.check("A", "Bulk", "POST /projects/delete/bulk soft-deletes every id", bd.status == 200 && bd.data()["soft_deleted"] == json!(2), &bd.request, "soft_deleted 2", &bd.got(), "backbone-core/src/http.rs (bulk delete)");
    let br = app.post("/projects/restore/bulk", ta, json!({ "ids": bids })).await;
    let restored = app.get(&format!("/projects/{}", bids.first().cloned().unwrap_or_default()), ta).await.status == 200;
    r.check("A", "Bulk", "POST /projects/restore/bulk restores every id", br.status == 200 && restored, &br.request, "200, back", &br.got(), "backbone-core/src/http.rs (bulk restore)");
    app.post("/projects/delete/bulk", ta, json!({ "ids": bids })).await;
    let bpd = app.delete_body("/projects/trash/bulk", ta, json!({ "ids": bids })).await;
    r.check("A", "Bulk", "DELETE /projects/trash/bulk removes every id for good", bpd.status == 200 && bpd.data()["permanently_deleted"] == json!(2), &bpd.request, "permanently_deleted 2", &bpd.got(), "backbone-core/src/http.rs (bulk permanent delete)");
    let bad_bulk = app.post("/projects/bulk", ta, json!([c.project_body(c.org.company_a, &uniq("B"), "Bulk fine"), c.project_body(c.org.company_a, "bad code", "x")])).await;
    r.check("A", "Bulk", "a bulk create with one invalid item refuses the batch with the item index in the path", bad_bulk.status == 422 && bad_bulk.violations().iter().any(|v| v["path"].as_str().is_some_and(|p| p.starts_with("1/"))), &bad_bulk.request, "422, a violation path starting 1/", &bad_bulk.got(), "backbone-core/src/http.rs (bulk create)");

    // count, aggregate, history, upsert
    let cnt = app.get(&format!("/projects/count?code[startwith]={tag}"), ta).await;
    r.check("A", "Count", "GET /projects/count honours filters", cnt.status == 200 && cnt.data()["count"] == json!(5), &cnt.request, "count 5", &cnt.got(), CRUD);
    let ag = app.get(&format!("/projects/aggregate?code[startwith]={tag}&group_by=kind&sum=budget"), ta).await;
    let groups = ag.data()["groups"].as_array().cloned().unwrap_or_default();
    let research = groups.iter().find(|g| g["key"] == json!("research"));
    r.check("A", "Aggregate", "GET /projects/aggregate?group_by=kind&sum=budget groups and sums", ag.status == 200 && groups.len() == 3 && research.map(|g| g["count"] == json!(2) && num(&g["sum"]["budget"]) == Some(500.0)).unwrap_or(false), &ag.request, "3 groups; research count 2, sum 500", &ag.got(), "backbone-core/src/http.rs (aggregate)");
    let h = app.get(&format!("/projects/{}/history", listed[0]), ta).await;
    r.check("A", "History", "GET /projects/:id/history on a model that is not audited answers 501 (no history provider)", h.status == 501, &h.request, "501", &h.got(), "backbone-core/src/http.rs:276 (history)");
    c.rec.unsupported("A", "History", "row history of an `audited: true` model", "needs backbone-auditlog composed: the generated capture trigger calls auditlog.capture_data_change (generators/data_change_audit.rs:52) and the route needs an Extension<Arc<dyn HistoryProvider>> (backbone-core/src/http.rs:276); a standalone module has neither");
    let up = app.post("/projects/upsert", ta, c.project_body(c.org.company_a, &uniq("U"), "Upsert")).await;
    r.check("A", "Upsert", "POST /projects/upsert answers 501", up.status == 501, &up.request, "501", &up.got(), "backbone-core/src/service.rs:1169");

    // body errors
    let mut uk = c.project_body(c.org.company_a, &uniq("K"), "Unknown key");
    uk["bogus"] = json!(1);
    let u = app.post("/projects", ta, uk).await;
    r.check("A", "Body errors", "POST with an unknown key answers 422 unknown_field on that path", u.status == 422 && u.violation("bogus", "unknown_field"), &u.request, "422 {path: bogus, code: unknown_field}", &u.got(), "backbone-core/src/extractors.rs:186-209");
    let u = app.patch(&format!("/projects/{}", listed[0]), ta, json!({ "bogus": 1 })).await;
    r.check("A", "Body errors", "PATCH with an unknown key answers 422 unknown_field", u.status == 422 && u.has_code("unknown_field"), &u.request, "422 unknown_field", &u.got(), "backbone-core/src/service.rs:1034 (merge_patch)");
    let m = app.send("POST", "/projects", Some(ta), Some("{\"code\": ".into()), &[]).await;
    r.check("A", "Body errors", "a malformed JSON body answers 400 malformed_body", m.status == 400 && m.has_code("malformed_body"), &m.request, "400 malformed_body", &m.got(), "backbone-core/src/extractors.rs:271");
    let mut wt = c.project_body(c.org.company_a, &uniq("W"), "Wrong type");
    wt["priority"] = json!("high");
    let w = app.post("/projects", ta, wt).await;
    r.check("A", "Body errors", "POST with a wrong type answers 422 with the field's path", w.status == 422 && w.violations().iter().any(|v| v["path"] == json!("priority")), &w.request, "422, violation path priority", &w.got(), "backbone-core/src/extractors.rs (json_refusal)");
    let w = app.patch(&format!("/projects/{}", listed[0]), ta, json!({ "priority": "high" })).await;
    r.check("A", "Body errors", "PATCH with a wrong type answers 422 with the field's path", w.status == 422 && w.violations().iter().any(|v| v["path"] == json!("priority")), &w.request, "422, violation path priority", &w.got(), "backbone-core/src/service.rs:1034 merge_patch answers ServiceError::Validation (400, no violations) for a value that does not deserialize");
}

// =============================================================================
// B. Validation
// =============================================================================

async fn area_b(c: &Ctx) {
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const GUARD: &str = "generated <Entity>RowRules (metaphor-plugin-schema/src/rules/fields.rs)";

    struct Case {
        what: &'static str,
        body: Value,
        path: &'static str,
        code: &'static str,
        param: Option<(&'static str, Value)>,
        diag: &'static str,
    }
    let cases = vec![
        Case { what: "@required refuses a blank name", body: json!({ "name": "   " }), path: "name", code: "required", param: None, diag: GUARD },
        Case { what: "@length(min=3) refuses a short name", body: json!({ "name": "ab" }), path: "name", code: "min_length", param: Some(("min", json!(3))), diag: GUARD },
        Case { what: "@length(max=12) refuses a long code", body: json!({ "code": "ABCDEFGHIJKLMNOP" }), path: "code", code: "max_length", param: Some(("max", json!(12))), diag: GUARD },
        Case { what: "@pattern refuses a lower-case code", body: json!({ "code": "lower" }), path: "code", code: "pattern", param: None, diag: GUARD },
        Case { what: "@max(500) on text refuses a long summary", body: json!({ "summary": "x".repeat(501) }), path: "summary", code: "max_length", param: Some(("max", json!(500))), diag: GUARD },
        Case { what: "the email type refuses a malformed address", body: json!({ "contactEmail": "not-an-email" }), path: "contact_email", code: "email", param: None, diag: GUARD },
        Case { what: "the url type refuses a malformed url", body: json!({ "website": "not a url" }), path: "website", code: "url", param: None, diag: GUARD },
        Case { what: "the @email attribute on a string field refuses a malformed address", body: json!({ "backupEmail": "not-an-email" }), path: "backup_email", code: "email", param: None, diag: "metaphor-plugin-schema/src/rules/fields.rs:308-313 derives the email/url checks from the field type only; the @email attribute is parsed (parser/yaml_parser/helpers.rs) and dropped" },
        Case { what: "the @url attribute on a string field refuses a malformed url", body: json!({ "docsUrl": "not a url" }), path: "docs_url", code: "url", param: None, diag: "metaphor-plugin-schema/src/rules/fields.rs:308-313 derives the email/url checks from the field type only; the @url attribute is dropped" },
        Case { what: "@non_negative refuses a negative budget", body: json!({ "budget": -1 }), path: "budget", code: "non_negative", param: None, diag: GUARD },
        Case { what: "@positive refuses a zero fee", body: json!({ "fee": 0 }), path: "fee", code: "positive", param: None, diag: GUARD },
        Case { what: "@range(1,5) refuses priority 9", body: json!({ "priority": 9 }), path: "priority", code: "range", param: Some(("max", json!(5))), diag: GUARD },
        Case { what: "@min(1) refuses headcount 0 (int64)", body: json!({ "headcount": 0 }), path: "headcount", code: "min", param: Some(("min", json!(1))), diag: GUARD },
        Case { what: "@max(100000) refuses headcount 200000 (int64)", body: json!({ "headcount": 200000 }), path: "headcount", code: "max", param: Some(("max", json!(100000))), diag: GUARD },
    ];
    for case in cases {
        let resp = c.project(ta, c.org.company_a, case.body.clone()).await;
        let v = resp.violations().into_iter().find(|v| v["path"].as_str() == Some(case.path) && v["code"].as_str() == Some(case.code));
        let param_ok = match (&case.param, &v) {
            (Some((k, want)), Some(v)) => num(&v["params"][*k]) == num(want),
            (None, _) => true,
            _ => false,
        };
        r.check("B", "Field attributes", case.what, resp.status == 422 && v.is_some() && param_ok, &resp.request, &format!("422 {{path: {}, code: {}{}}}", case.path, case.code, case.param.as_ref().map(|(k, v)| format!(", params.{k}: {v}")).unwrap_or_default()), &resp.got(), case.diag);
    }
    let missing = app.post("/projects", ta, json!({ "companyId": c.org.company_a, "code": uniq("M") })).await;
    r.check("B", "Field attributes", "a missing required key answers 422 required on its path", missing.status == 422 && missing.violation("name", "required"), &missing.request, "422 {path: name, code: required}", &missing.got(), "backbone-core/src/extractors.rs (missing field)");
    let multi = c.project(ta, c.org.company_a, json!({ "name": "ab", "priority": 9, "budget": -1 })).await;
    r.check("B", "Field attributes", "every broken rule is reported at once", multi.status == 422 && multi.violations().len() >= 3, &multi.request, "422 with >= 3 violations", &multi.got(), "backbone-core/src/service.rs (guard collects all)");

    // defaults
    let d = c.project(ta, c.org.company_a, json!({})).await;
    let dd = d.data();
    let defaults_ok = d.status == 201 && dd["priority"] == json!(3) && dd["billable"] == json!(true) && dd["kind"] == json!("internal") && num(&dd["budget"]) == Some(0.0) && Uuid::parse_str(dd["id"].as_str().unwrap_or("")).is_ok();
    r.check("B", "Defaults", "@default fills omitted fields (literal int/bool/enum/decimal, uuid id)", defaults_ok, &d.request, "priority 3, billable true, kind internal, budget 0, uuid id", &d.got(), "metaphor-plugin-schema/src/generators/dto.rs:1254 (create defaults)");
    let ts_ok = dd["metadata"]["createdAt"].is_string() || dd["metadata"]["created_at"].is_string() || dd["createdAt"].is_string();
    r.check("B", "Defaults", "@default(now) and audit metadata stamp created_at", ts_ok, &d.request, "metadata.createdAt set", &d.got(), "audit metadata trigger (generators/sql.rs:1082-1124)");
    let t = c.ticket(ta, c.org.company_a, &d.id(), json!({})).await;
    r.check("B", "Defaults", "@default(uuid) on a non-id field fills it", t.status == 201 && Uuid::parse_str(t.data()["reference"].as_str().unwrap_or("")).is_ok(), &t.request, "reference is a uuid", &t.got(), "generators/dto.rs (uuid default)");
    r.check("B", "Defaults", "a state field is born at its initial state", t.data()["status"] == json!("open"), &t.request, "status open", &t.got(), "generators/dto.rs");

    // write-protected fields
    let p = c.project(ta, c.org.company_a, json!({ "externalRef": "EXT-1", "apiSecret": "s3cret" })).await;
    let pid = p.id();
    r.check("B", "Immutable", "an @immutable field is accepted on create", p.status == 201 && p.data()["externalRef"] == json!("EXT-1"), &p.request, "201, externalRef EXT-1", &p.got(), "generators/dto.rs");
    let im = app.patch(&format!("/projects/{pid}"), ta, json!({ "externalRef": "EXT-2" })).await;
    r.check("B", "Immutable", "PATCH changing an @immutable field answers 422 field_not_writable", im.status == 422 && im.violation("external_ref", "field_not_writable"), &im.request, "422 {path: external_ref, code: field_not_writable}", &im.got(), "backbone-core/src/service.rs:999 (refuse_protected_changes)");
    let same = app.patch(&format!("/projects/{pid}"), ta, json!({ "externalRef": "EXT-1", "name": "Same ref kept" })).await;
    r.check("B", "Immutable", "PATCH carrying an @immutable field unchanged is accepted", same.status == 200, &same.request, "200", &same.got(), "backbone-core/src/service.rs:999");
    let im_put = app.put(&format!("/projects/{pid}"), ta, json!({ "companyId": c.org.company_a, "code": p.data()["code"], "name": "Put immutable", "externalRef": "EXT-3", "budget": 0, "priority": 3, "billable": true, "kind": "internal" })).await;
    r.check("B", "Immutable", "PUT changing an @immutable field is refused", im_put.status == 422 || (im_put.status == 200 && im_put.data()["externalRef"] == json!("EXT-1")), &im_put.request, "422 field_not_writable, or 200 with externalRef unchanged", &im_put.got(), "generators/dto.rs:1250 (immutable left out of the Update DTO)");

    let secret_hidden = |r: &Resp| !r.raw.contains("s3cret") && r.data().get("apiSecret").map(|v| v.is_null()).unwrap_or(true);
    r.check("B", "Private field", "an @private field never appears in a create response", p.status == 201 && secret_hidden(&p), &p.request, "no apiSecret value", &p.got(), "backbone-core/src/http.rs:474-578 (private field stripping)");
    let g = app.get(&format!("/projects/{pid}"), ta).await;
    r.check("B", "Private field", "an @private field never appears in a GET response", secret_hidden(&g), &g.request, "no apiSecret value", &g.got(), "backbone-core/src/http.rs:474-578 (private field stripping)");
    let l = app.get(&format!("/projects?id={pid}"), ta).await;
    r.check("B", "Private field", "an @private field never appears in a list response", secret_hidden(&l), &l.request, "no apiSecret value", &l.got(), "backbone-core/src/http.rs:474-578 (private field stripping)");

    let ex = c.project(ta, c.org.company_a, json!({ "internalScore": 7 })).await;
    r.check("B", "Excluded from DTO", "POST carrying an @exclude_from_dto field answers 422 unknown_field", ex.status == 422 && ex.has_code("unknown_field"), &ex.request, "422 unknown_field", &ex.got(), "generators/dto.rs:1236");
    let exp = app.patch(&format!("/projects/{pid}"), ta, json!({ "internalScore": 7 })).await;
    let stored: Option<i32> = sqlx::query_scalar("SELECT internal_score FROM conformance.projects WHERE id = $1").bind(Uuid::parse_str(&pid).unwrap()).fetch_one(&c.owner).await.unwrap_or(None);
    r.check("B", "Excluded from DTO", "PATCH cannot set an @exclude_from_dto field", exp.status == 422 && stored.is_none(), &exp.request, "422 and the column stays NULL", &format!("{}; stored {stored:?}", exp.got()), "backbone-core/src/service.rs:1034 merge_patch accepts any entity field; Model::write_protected_fields (metaphor-plugin-schema/src/ast/model.rs:173) leaves @exclude_from_dto out");

    // lifecycle shapes
    let tid = t.id();
    let st = app.patch(&format!("/tickets/{tid}"), ta, json!({ "status": "resolved" })).await;
    r.check("B", "State field", "a generic PATCH of a machine's state field answers 422 state_field_verbs_only", st.status == 422 && st.violation("status", "state_field_verbs_only"), &st.request, "422 {path: status, code: state_field_verbs_only}", &st.got(), "generated TicketRowRules (rules/compile.rs:647-653)");
    let pj = app.patch(&format!("/tickets/{tid}"), ta, json!({ "stage": "triage" })).await;
    r.check("B", "Lifecycle projection", "a generic PATCH of a projection field answers 422 field_not_writable", pj.status == 422 && pj.violation("stage", "field_not_writable"), &pj.request, "422 {path: stage, code: field_not_writable}", &pj.got(), "backbone-core/src/service.rs:999");
    let pjc = c.ticket(ta, c.org.company_a, &pid, json!({ "stage": "triage" })).await;
    r.check("B", "Lifecycle projection", "POST carrying a projection field answers 422 unknown_field (born values stay out of the create DTO)", pjc.status == 422 && pjc.has_code("unknown_field"), &pjc.request, "422 unknown_field", &pjc.got(), "generators/dto.rs:1362");
    let hs = app.post("/approvals", ta, json!({ "companyId": c.org.company_a, "subject": "x" })).await;
    r.check("B", "Lifecycle hand_set", "an entity with a hand_set field mounts no generic create (405)", hs.status == 405, &hs.request, "405", &hs.got(), "metaphor-plugin-schema/src/ast/model.rs:148 (mounts_writes)");

    // database constraints, through the HTTP path
    let fk = c.ticket(ta, c.org.company_a, &Uuid::new_v4().to_string(), json!({})).await;
    r.check("B", "DB constraints", "a dangling foreign key answers 422 reference_missing on project_id", fk.status == 422 && fk.violation("project_id", "reference_missing"), &fk.request, "422 {path: project_id, code: reference_missing}", &fk.got(), "backbone-core/src/persistence/traits.rs:271 key_column reads the column from the DETAIL `Key (col)=`, which Postgres leaves out when the table has RLS enabled; the constraint name fk_<table>_<col> (generators/sql.rs) is not parsed, so the path comes back empty on every fenced table");
    let parent = c.project(ta, c.org.company_a, json!({})).await.id();
    let _child = c.ticket(ta, c.org.company_a, &parent, json!({})).await;
    app.delete(&format!("/projects/{parent}"), ta).await;
    let sr = app.delete(&format!("/projects/trash/{parent}"), ta).await;
    r.check("B", "DB constraints", "permanently deleting a referenced row answers 422 still_referenced", sr.status == 422 && sr.has_code("still_referenced"), &sr.request, "422 still_referenced", &sr.got(), "backbone-core/src/persistence/traits.rs:150 (23503)");
    app.post(&format!("/projects/{parent}/restore"), ta, json!({})).await;

    let mail = format!("{}@example.com", uniq("u").to_lowercase());
    let u1 = c.project(ta, c.org.company_a, json!({ "contactEmail": mail })).await;
    let u2 = c.project(ta, c.org.company_a, json!({ "contactEmail": mail })).await;
    let raw_leak = u2.raw.contains("idx_projects") || u2.raw.contains("duplicate key");
    r.check("B", "DB constraints", "a duplicate @unique value answers 422 naming contact_email, without the raw index name", u1.status == 201 && u2.status == 422 && u2.violations().iter().any(|v| v["path"] == json!("contact_email")) && !raw_leak, &u2.request, "422, violation path contact_email, no index name in the body", &u2.got(), "the index is uq_projects_contact_email__unique (generators/sql.rs unique_index_name); Postgres reports no key on a table with RLS, so backbone-core/src/persistence/traits.rs must read the path out of the name (convention: generators/sql.rs constraint_name)");
    let title = format!("Same title {}", uniq(""));
    let c1 = c.ticket(ta, c.org.company_a, &parent, json!({ "title": title })).await;
    let c2 = c.ticket(ta, c.org.company_a, &parent, json!({ "title": title })).await;
    let raw_leak = c2.raw.contains("idx_tickets") || c2.raw.contains("duplicate key");
    r.check("B", "DB constraints", "a duplicate composite unique (project_id, title) answers 422 with a field path, without the raw index name", c1.status == 201 && c2.status == 422 && !c2.violations().is_empty() && !raw_leak, &c2.request, "422 with a violation, no index name in the body", &c2.got(), "the index is uq_tickets_title__unique (generators/sql.rs unique_index_name); backbone-core/src/persistence/traits.rs reads the code from it and the path from the DETAIL or the name");

    let nn = app.patch(&format!("/tickets/{}", c1.id()), ta, json!({ "title": null })).await;
    r.check("B", "DB constraints", "PATCH nulling a NOT NULL column answers 422 required on its path", nn.status == 422 && nn.violation("title", "required"), &nn.request, "422 {path: title, code: required}", &nn.got(), "backbone-core/src/service.rs:1034 merge_patch / persistence/traits.rs (23502)");

    // the database's own refusal, mapped by the framework, for a writer the guard never saw
    let scoped = |sql: &'static str, company: Uuid| {
        let pool = c.pool.clone();
        async move {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("SELECT set_config('app.company_id', $1, true), set_config('app.scope_unit_ids', $1, true)").bind(company.to_string()).execute(&mut *tx).await.unwrap();
            let res = sqlx::query(sql).execute(&mut *tx).await;
            let _ = tx.rollback().await;
            res
        }
    };
    for (what, sql, path, code) in [
        ("a CHECK from @non_negative refuses a raw write and maps to non_negative on budget", "UPDATE conformance.projects SET budget = -5", "budget", "non_negative"),
        ("a CHECK from a declared rule refuses a raw write and maps to its code, on the path the guard reports (kind)", "UPDATE conformance.projects SET kind = 'client', website = NULL", "kind", "website_required_for_client"),
        ("a CHECK from @required refuses a blank title and maps to required", "UPDATE conformance.tickets SET title = '  '", "title", "required"),
    ] {
        let res = scoped(sql, c.org.company_a).await;
        match res {
            Err(e) => {
                let mapped = backbone_core::persistence::RepositoryError::from(e);
                let rendered = format!("{mapped:?}");
                let (ok, got) = match &mapped {
                    backbone_core::persistence::RepositoryError::Violations(vs) => (
                        vs.iter().any(|v| v.path == path && v.code == code && !v.message.contains("ck_")),
                        rendered.clone(),
                    ),
                    _ => (false, rendered.clone()),
                };
                r.check("B", "DB constraints", what, ok, sql, &format!("Violation {{path: {path}, code: {code}}}, no ck_ name in the message"), &got, "a row rule's CHECK is ck_<table>_<field>__<code> (generators/sql.rs constraint_name); backbone-core/src/persistence/traits.rs check_column must read that form as well as a field check's ck_<table>_<field>_<code>__<code>");
            }
            Ok(done) => {
                r.check("B", "DB constraints", what, false, sql, "the database refuses", &format!("{} row(s) updated", done.rows_affected()), "the add_declared_checks migration did not add the constraint");
            }
        }
    }
}

// =============================================================================
// C. Custom validation
// =============================================================================

async fn area_c(c: &Ctx) {
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const GUARD: &str = "generated <Entity>RowRules (metaphor-plugin-schema/src/rules/compile.rs:101)";
    let pid = c.project(ta, c.org.company_a, json!({})).await.id();

    let t = c.ticket(ta, c.org.company_a, &pid, json!({ "amount": 500, "budgetCap": 100 })).await;
    r.check("C", "Declared rule", "declared row rule refuses: amount over budget_cap answers 422 over_budget", t.status == 422 && t.has_code("over_budget"), &t.request, "422 over_budget", &t.got(), GUARD);
    let ok = c.ticket(ta, c.org.company_a, &pid, json!({ "amount": 50, "budgetCap": 100 })).await;
    r.check("C", "Declared rule", "declared row rule lets a valid row through (amount 50 <= budget_cap 100, both decimal)", ok.status == 201, &ok.request, "201", &ok.got(), "backbone-core/src/row_rule.rs:195-197 order() compares two Val::Text lexicographically, and a rust_decimal field serializes as a JSON string, so the guard compares \"50\" <= \"100\" as text (false); the generator emits r::field for decimals without a numeric cast (metaphor-plugin-schema/src/rules/compile.rs)");
    let fresh = c.ticket(ta, c.org.company_a, &pid, json!({ "amount": 7 })).await.id();
    let up = app.patch(&format!("/tickets/{fresh}"), ta, json!({ "budgetCap": 5 })).await;
    r.check("C", "Declared rule", "declared row rule refuses on update too", up.status == 422 && up.has_code("over_budget"), &up.request, "422 over_budget", &up.got(), GUARD);
    let t = c.ticket(ta, c.org.company_a, &pid, json!({ "discount": 5 })).await;
    r.check("C", "Declared rule", "declared row rule refuses: a discount without a reason answers 422 discount_reason_required", t.status == 422 && t.has_code("discount_reason_required"), &t.request, "422 discount_reason_required", &t.got(), GUARD);
    let p = c.project(ta, c.org.company_a, json!({ "kind": "client" })).await;
    r.check("C", "Declared rule", "declared row rule refuses: a client project without a website answers 422 website_required_for_client", p.status == 422 && p.has_code("website_required_for_client"), &p.request, "422 website_required_for_client", &p.got(), GUARD);

    let code = uniq("Q");
    let first = app.post("/projects", ta, c.project_body(c.org.company_a, &code, "Unique one")).await;
    let dup = app.post("/projects", ta, c.project_body(c.org.company_a, &code, "Unique two")).await;
    r.check("C", "Declared uniqueness", "declared uniqueness rule refuses a duplicate code in the company: 422 code_taken on path code", first.status == 201 && dup.status == 422 && dup.violation("code", "code_taken"), &dup.request, "422 {path: code, code: code_taken}", &dup.got(), "the index is uq_projects_company_id_code__code_taken (generators/sql.rs unique_index_name); Postgres omits the DETAIL key on a table with RLS, so backbone-core/src/persistence/traits.rs must read the path out of the name, stripping the leading tenant keys");
    let other = app.post("/projects", &c.tb, c.project_body(c.org.company_b, &code, "Other company")).await;
    r.check("C", "Declared uniqueness", "declared uniqueness rule is per company: the same code in another company is accepted", other.status == 201, &other.request, "201", &other.got(), "generators/sql.rs:2414-2445");
    app.delete(&format!("/projects/{}", first.id()), ta).await;
    let after_delete = app.post("/projects", ta, c.project_body(c.org.company_a, &code, "Reused after delete")).await;
    r.check("C", "Declared uniqueness", "declared uniqueness rule ignores soft-deleted rows (its filter)", after_delete.status == 201, &after_delete.request, "201", &after_delete.got(), "generators/sql.rs:2414-2445 (partial index WHERE)");

    let sh = c.ticket(ta, c.org.company_a, &pid, json!({ "note": "shadow" })).await;
    r.check("C", "Shadow rule", "shadow rule lets the write through", sh.status == 201, &sh.request, "201", &sh.got(), "rules/compile.rs:142 (shadow)");
    let un = c.ticket(ta, c.org.company_a, &pid, json!({ "title": "zz" })).await;
    r.check("C", "Unenforced rule", "unenforced rule lets the write through", un.status == 201, &un.request, "201", &un.got(), "rules register (UNENFORCED)");

    let hk = app.post("/hooked-projects", ta, c.project_body(c.org.company_a, &uniq("H"), "A forbidden name")).await;
    r.check("C", "Extension check", "the hand-written check refuses through the extension's service: 422 name_forbidden", hk.status == 422 && hk.violation("name", "name_forbidden"), &hk.request, "422 {path: name, code: name_forbidden}", &hk.got(), "project_service.ext.rs (ProjectCustomRules)");
    let both = app.post("/hooked-projects", ta, json!({ "companyId": c.org.company_a, "code": "bad code", "name": "forbidden" })).await;
    r.check("C", "Extension check", "the schema guard and the hand-written check report together", both.status == 422 && both.has_code("name_forbidden") && both.has_code("pattern"), &both.request, "422 with name_forbidden and pattern", &both.got(), "project_service.ext.rs (ProjectGuards)");
    let plain = app.post("/projects", ta, c.project_body(c.org.company_a, &uniq("H"), "A forbidden name")).await;
    r.check("C", "Extension check", "the generated service (no extension) does not run the hand-written check", plain.status == 201, &plain.request, "201", &plain.got(), "lib.rs build_project_service");

    // the VERB rule: held by the hand-written verb only
    let v = c.ticket(ta, c.org.company_a, &pid, json!({})).await.id();
    for verb in ["start", "resolve"] {
        app.post(&format!("/tickets/{v}/transitions/{verb}"), ta, json!({})).await;
    }
    let blank = app.post(&format!("/tickets/{v}/archive-with-reason"), ta, json!({ "reason": " " })).await;
    r.check("C", "Verb rule", "rule held by a hand-written verb refuses a blank reason: 422 archive_reason_required", blank.status == 422 && blank.violation("archive_reason", "archive_reason_required"), &blank.request, "422 {path: archive_reason, code: archive_reason_required}", &blank.got(), "ticket_service.ext.rs::archive_with_reason");
    let gen = app.post(&format!("/tickets/{v}/transitions/archive"), ta, json!({})).await;
    r.check("C", "Verb rule", "rule held by a hand-written verb is not held by the generated archive verb (VERB holds on its own path only, as documented)", gen.status == 200 && gen.data()["archiveReason"].is_null(), &gen.request, "200, archived with no reason", &gen.got(), "docs/handbook/rule-enforcement-map.md (VERB)");
}

// =============================================================================
// D. Hooks and events
// =============================================================================

async fn area_d(c: &Ctx) {
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    let log = c.module.hook_log();
    const EXT: &str = "backbone-core/src/service.rs (ServiceLifecycle / CrudEventPublisher calls)";
    let count = |entries: &[String], prefix: &str| entries.iter().filter(|e| e.starts_with(prefix)).count();

    log.clear();
    let code = uniq("D");
    let cr = app.post("/hooked-projects", ta, c.project_body(c.org.company_a, &code, "Hooked one")).await;
    let id = cr.id();
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "create fires before_create, after_create and a created event exactly once each", cr.status == 201 && count(&e, &format!("before_create:{code}")) == 1 && count(&e, &format!("after_create:{code}:{id}")) == 1 && count(&e, &format!("event:created:{code}")) == 1 && e.len() == 3, &cr.request, "[before_create, after_create, event:created], once each", &format!("{e:?}"), EXT);
    r.check("D", "Lifecycle hooks", "hooks run in order: before, after, event", e.first().is_some_and(|x| x.starts_with("before_create")) && e.last().is_some_and(|x| x.starts_with("event:created")), &cr.request, "before_create first, event last", &format!("{e:?}"), EXT);

    log.clear();
    let pa = app.patch(&format!("/hooked-projects/{id}"), ta, json!({ "name": "Hooked two" })).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "PATCH fires before_update/after_update once and an event carrying before and after", pa.status == 200 && count(&e, "before_update:") == 1 && count(&e, "after_update:") == 1 && e.iter().any(|x| x == "event:patched:Hooked one->Hooked two" || x == "event:updated:Hooked one->Hooked two") && e.len() == 3, &pa.request, "[before_update, after_update, event:(patched|updated):Hooked one->Hooked two]", &format!("{e:?}"), EXT);
    r.check("D", "Lifecycle hooks", "before_update sees the row as it will be stored", e.iter().any(|x| x.ends_with(":Hooked two") && x.starts_with("before_update")), &pa.request, "before_update:<code>:Hooked two", &format!("{e:?}"), EXT);

    log.clear();
    let put = app.put(&format!("/hooked-projects/{id}"), ta, json!({ "companyId": c.org.company_a, "code": code, "name": "Hooked three", "budget": 0, "priority": 3, "billable": true, "kind": "internal" })).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "PUT fires the update hooks once and an updated event with before and after", put.status == 200 && count(&e, "before_update:") == 1 && count(&e, "after_update:") == 1 && e.iter().any(|x| x == "event:updated:Hooked two->Hooked three") && e.len() == 3, &put.request, "[before_update, after_update, event:updated:Hooked two->Hooked three]", &format!("{e:?}"), EXT);

    log.clear();
    let bad = app.patch(&format!("/hooked-projects/{id}"), ta, json!({ "priority": 9 })).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "a refused write fires no hook and no event", bad.status == 422 && e.is_empty(), &bad.request, "422, nothing recorded", &format!("{} {e:?}", bad.status), EXT);

    log.clear();
    let del = app.delete(&format!("/hooked-projects/{id}"), ta).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "soft delete fires before_delete, after_delete and a soft_deleted event once each", del.status == 200 && count(&e, "before_delete:") == 1 && count(&e, &format!("after_delete:{id}")) == 1 && count(&e, "event:soft_deleted:") == 1 && e.len() == 3, &del.request, "[before_delete, after_delete, event:soft_deleted]", &format!("{e:?}"), EXT);

    log.clear();
    let rs = app.post(&format!("/hooked-projects/{id}/restore"), ta, json!({})).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "restore fires before_restore and a restored event once each", rs.status == 200 && count(&e, "before_restore:") == 1 && count(&e, "event:restored:") == 1 && e.len() == 2, &rs.request, "[before_restore, event:restored]", &format!("{e:?}"), EXT);

    app.delete(&format!("/hooked-projects/{id}"), ta).await;
    log.clear();
    let hd = app.delete(&format!("/hooked-projects/trash/{id}"), ta).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "permanent delete fires before_hard_delete and a hard_deleted event once each", hd.status == 204 && count(&e, "before_hard_delete:") == 1 && count(&e, &format!("event:hard_deleted:{id}")) == 1 && e.len() == 2, &hd.request, "[before_hard_delete, event:hard_deleted]", &format!("{e:?}"), EXT);

    log.clear();
    let bc = app.post("/hooked-projects/bulk", ta, json!([c.project_body(c.org.company_a, &uniq("D"), "Bulk hook one"), c.project_body(c.org.company_a, &uniq("D"), "Bulk hook two")])).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "bulk create fires before_create and after_create once per item", bc.status == 201 && count(&e, "before_create:") == 2 && count(&e, "after_create:") == 2, &bc.request, "2 before_create, 2 after_create", &format!("{e:?}"), "backbone-core/src/service.rs:655 (bulk_create)");
    let bids: Vec<String> = bc.data()["items"].as_array().cloned().unwrap_or_default().iter().filter_map(|i| i["id"].as_str().map(str::to_string)).collect();
    log.clear();
    let bp = app.patch("/hooked-projects/bulk", ta, json!({ "ids": bids, "patch": { "priority": 4 } })).await;
    let e = log.entries();
    r.check("D", "Lifecycle hooks", "bulk patch fires the update hooks once per item", bp.status == 200 && count(&e, "before_update:") == 2 && count(&e, "after_update:") == 2, &bp.request, "2 before_update, 2 after_update", &format!("{e:?}"), "backbone-core/src/service.rs:882 (bulk_partial_update)");

    area_d_events(c).await;
    area_d_triggers(c).await;
}

// =============================================================================
// D (continued). Declared events: staged in the write's transaction
// =============================================================================

/// One staged outbox row, as the probes read it.
#[derive(Debug, Clone)]
struct Staged {
    event_type: String,
    correlation_id: Option<String>,
    version: i32,
    payload: Value,
    published: bool,
}

async fn staged(c: &Ctx, aggregate_id: &str) -> Vec<Staged> {
    let rows: Vec<(String, Option<String>, i32, Value, bool)> = sqlx::query_as(
        "SELECT event_type, correlation_id, version, payload, published_at IS NOT NULL FROM conformance.outbox_events \
         WHERE aggregate_id = $1 ORDER BY created_at, event_type",
    )
    .bind(aggregate_id)
    .fetch_all(&c.owner)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(event_type, correlation_id, version, payload, published)| Staged { event_type, correlation_id, version, payload, published })
        .collect()
}

async fn staged_total(c: &Ctx) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM conformance.outbox_events").fetch_one(&c.owner).await.unwrap()
}

fn kinds(rows: &[Staged]) -> Vec<&str> {
    rows.iter().map(|r| r.event_type.as_str()).collect()
}

async fn ticket_count(c: &Ctx, title: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM conformance.tickets WHERE title = $1").bind(title).fetch_one(&c.owner).await.unwrap()
}

/// The `backbone::trigger` log lines the probes capture, as field maps.
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<std::collections::BTreeMap<String, String>>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogCapture {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != "backbone::trigger" {
            return;
        }
        struct Fields(std::collections::BTreeMap<String, String>);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.insert(field.name().to_string(), format!("{value:?}"));
            }
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                self.0.insert(field.name().to_string(), value.to_string());
            }
        }
        let mut fields = Fields(Default::default());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
}

/// D. Hook triggers: they run in the write's transaction, around the write, in file order.
async fn area_d_triggers(c: &Ctx) {
    use tracing_subscriber::layer::SubscriberExt;
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const TRIG: &str = "generated incident_write_hooks.rs and incident_triggers.rs (generators/triggers.rs), incident.hook.yaml triggers:, the hand half in incident_triggers.ext.rs";
    let capture = LogCapture::default();
    let installed = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(capture.clone())).is_ok();
    let company = c.org.company_a;
    let body = |title: &str, severity: i64| json!({ "companyId": company, "title": title, "severity": severity });
    let kinds_of = |rows: &[Staged], kind: &str| rows.iter().filter(|x| x.event_type == kind).count();

    // before_create `set:` is persisted; a label with `timing:` and `condition:` runs after it.
    let t1 = uniq("INC");
    let made = app.post("/incidents", ta, body(&t1, 1)).await;
    let id = made.id();
    let stored: Option<(Option<String>, Option<String>)> = sqlx::query_as("SELECT sla, handled_by FROM conformance.incidents WHERE id = $1").bind(Uuid::parse_str(&id).unwrap()).fetch_optional(&c.owner).await.unwrap();
    r.check("D", "Hook triggers", "a before_create `set:` is persisted (sla, handled_by)", made.status == 201 && stored == Some((Some("standard".into()), Some("unassigned".into()))) && made.data()["sla"] == json!("standard"), &made.request, "201; sla standard, handled_by unassigned, stored and answered", &format!("{} {stored:?}", made.got()), TRIG);
    let urgent = app.post("/incidents", ta, body(&uniq("INC"), 4)).await;
    r.check("D", "Hook triggers", "a trigger keyed by a label with `timing: before_create` and a `condition:` runs after the one before it, and only when it holds", urgent.status == 201 && urgent.data()["sla"] == json!("urgent") && made.data()["sla"] == json!("standard"), &urgent.request, "severity 4 -> urgent; severity 1 -> standard", &format!("{} / {}", urgent.data()["sla"], made.data()["sla"]), TRIG);

    // after_create: the inline actions stage in the write's transaction, with the request's correlation id.
    let cid = format!("trg-{}", &Uuid::new_v4().simple().to_string()[..10]);
    let t2 = uniq("INC");
    let cr = app.send("POST", "/incidents", Some(ta), Some(body(&t2, 2).to_string()), &[("x-correlation-id", cid.as_str())]).await;
    let id2 = cr.id();
    let rows = staged(c, &id2).await;
    r.check("D", "Hook triggers", "after_create stages what its compiled actions say: the emitted event and a request per notify and send_email", cr.status == 201 && kinds(&rows) == ["IncidentOpened", "NotificationRequested", "NotificationRequested"], &cr.request, "201; IncidentOpened, NotificationRequested x2", &format!("{} {:?}", cr.status, kinds(&rows)), TRIG);
    r.check("D", "Hook triggers", "every row a trigger staged carries the request's correlation id", rows.iter().all(|x| x.correlation_id.as_deref() == Some(cid.as_str())), &cr.request, &format!("correlation_id {cid} on every row"), &format!("{:?}", rows.iter().map(|x| x.correlation_id.clone()).collect::<Vec<_>>()), TRIG);
    r.check("D", "Hook triggers", "`emit:` builds the declared payload from its arguments", rows.first().is_some_and(|x| x.payload["incident_id"] == json!(id2) && x.payload["severity"] == json!(2)), &cr.request, "incident_id and severity", &format!("{:?}", rows.first().map(|x| x.payload.clone())), TRIG);
    let notify = rows.iter().find(|x| x.payload["channel"] == json!("notify"));
    let email = rows.iter().find(|x| x.payload["channel"] == json!("email"));
    r.check("D", "Hook triggers", "`notify` stages a NotificationRequested with its text interpolated, its recipients and variables", notify.is_some_and(|x| x.payload["body"] == json!(format!("Incident {t2} opened")) && x.payload["to"] == json!([id2]) && x.payload["data"]["severity"] == json!(2) && x.payload["trigger"] == json!("Incident.after_create")), &cr.request, "channel notify; body 'Incident <title> opened'; to [id]", &format!("{:?}", notify.map(|x| x.payload.clone())), TRIG);
    r.check("D", "Hook triggers", "`send_email` stages a NotificationRequested naming its template and recipient", email.is_some_and(|x| x.payload["template"] == json!("incident_opened") && x.payload["to"] == json!("unassigned") && x.payload["data"]["title"] == json!(t2)), &cr.request, "channel email; template incident_opened; to the value `set:` filled in before the write", &format!("{:?}", email.map(|x| x.payload.clone())), TRIG);

    // log() carries the correlation id (and the audit tag), and leaves out a field the hook cannot know.
    if installed {
        let lines = capture.0.lock().unwrap().clone();
        let line = lines.iter().find(|l| l.get("log_event").map(String::as_str) == Some("incident.opened") && l.get("correlation_id").map(String::as_str) == Some(cid.as_str()));
        r.check("D", "Hook triggers", "`log()` writes a line carrying the correlation id, the trigger it came from and its fields", line.is_some_and(|l| l.get("trigger").map(String::as_str) == Some("Incident.after_create") && l.get("incident_id").is_some_and(|v| v.contains(&id2)) && l.get("severity").map(String::as_str) == Some("2")), &cr.request, "a backbone::trigger line: correlation_id, trigger Incident.after_create, incident_id, severity", &format!("{line:?}"), TRIG);
        r.check("D", "Hook triggers", "a `log()` field the write cannot know ($context.ip_address) is left out, not invented", line.is_some_and(|l| !l.contains_key("ip")), &cr.request, "no `ip` field", &format!("{line:?}"), TRIG);
    } else {
        r.unsupported("D", "Hook triggers", "`log()` writes a line carrying the correlation id", "the probe process already had a global tracing subscriber");
    }

    // after_update_<field>: fires only when the field changed.
    let t3 = uniq("INC");
    let id3 = app.post("/incidents", ta, body(&t3, 1)).await.id();
    let before = staged(c, &id3).await.len();
    let sev = app.patch(&format!("/incidents/{id3}"), ta, json!({ "severity": 2 })).await;
    let unchanged = staged(c, &id3).await;
    r.check("D", "Hook triggers", "a write that leaves status and title alone fires neither after_update_status nor after_update_title", sev.status == 200 && unchanged.len() == before && kinds_of(&unchanged, "IncidentStatusChanged") == 0 && kinds_of(&unchanged, "IncidentRetitled") == 0, &sev.request, "no new outbox row", &format!("{} {before} -> {}", sev.status, unchanged.len()), TRIG);
    let retitle = app.patch(&format!("/incidents/{id3}"), ta, json!({ "title": format!("{t3}-new") })).await;
    let after_title = staged(c, &id3).await;
    r.check("D", "Hook triggers", "after_update_title fires when a generic write changes the title (its `if:` over $old holds)", retitle.status == 200 && kinds_of(&after_title, "IncidentRetitled") == 1 && after_title.iter().find(|x| x.event_type == "IncidentRetitled").is_some_and(|x| x.payload["title"] == json!(format!("{t3}-new"))), &retitle.request, "one IncidentRetitled", &format!("{} {:?}", retitle.status, kinds(&after_title)), TRIG);

    // after_update_status, with and without a value suffix; on_enter and on_exit of a state.
    let ack = app.post(&format!("/incidents/{id3}/transitions/acknowledge"), ta, json!({})).await;
    let rows = staged(c, &id3).await;
    let changed = rows.iter().find(|x| x.event_type == "IncidentStatusChanged");
    r.check("D", "Hook triggers", "after_update_status fires when a transition changes the state, and carries the state it left ($old)", ack.status == 200 && changed.is_some_and(|x| x.payload["status"] == json!("acknowledged") && x.payload["was"] == json!("open")), &ack.request, "IncidentStatusChanged status acknowledged, was open", &format!("{} {:?}", ack.status, changed.map(|x| x.payload.clone())), TRIG);
    r.check("D", "Hook triggers", "on_enter emits on a transition into the state, on_exit on a transition out of it", kinds_of(&rows, "IncidentAcknowledged") == 1 && kinds_of(&rows, "IncidentLeftOpen") == 1, &ack.request, "IncidentAcknowledged and IncidentLeftOpen, once each", &format!("{:?}", kinds(&rows)), TRIG);
    r.check("D", "Hook triggers", "after_update_status_resolved does not fire for a change to another state", kinds_of(&rows, "IncidentResolved") == 0, &ack.request, "no IncidentResolved after acknowledge", &format!("{:?}", kinds(&rows)), TRIG);
    let res = app.post(&format!("/incidents/{id3}/transitions/resolve"), ta, json!({})).await;
    let rows = staged(c, &id3).await;
    r.check("D", "Hook triggers", "after_update_status_resolved fires when the state becomes resolved, beside after_update_status", res.status == 200 && kinds_of(&rows, "IncidentResolved") == 1 && kinds_of(&rows, "IncidentStatusChanged") == 2, &res.request, "IncidentResolved once; IncidentStatusChanged twice in all", &format!("{} {:?}", res.status, kinds(&rows)), TRIG);

    // after_<verb> of the machine runs its hand method, in the transaction.
    let reopen = app.post(&format!("/incidents/{id3}/transitions/reopen"), ta, json!({})).await;
    let rows = staged(c, &id3).await;
    let noted = rows.iter().find(|x| x.event_type == "ReopenNoted");
    r.check("D", "Hook triggers", "an after_<verb> trigger runs its hand-written method on that transition and no other", reopen.status == 200 && kinds_of(&rows, "ReopenNoted") == 1 && noted.is_some_and(|x| x.payload["was"] == json!("resolved") && x.payload["is"] == json!("open")), &reopen.request, "one ReopenNoted: was resolved, is open", &format!("{} {:?}", reopen.status, noted.map(|x| x.payload.clone())), TRIG);

    // A trigger that returns an error rolls back the write and everything it staged.
    let veto_title = format!("{} veto", uniq("INC"));
    let before_total = staged_total(c).await;
    let veto = app.post("/incidents", ta, body(&veto_title, 1)).await;
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.incidents WHERE title = $1").bind(&veto_title).fetch_one(&c.owner).await.unwrap();
    r.check("D", "Hook triggers", "a hand method returning an error refuses the write with its code (422) and leaves no row", veto.status == 422 && veto.violation("title", "trigger_refused") && kept == 0, &veto.request, "422 {path: title, code: trigger_refused}; no incident row", &format!("{} rows {kept}", veto.got()), TRIG);
    r.check("D", "Hook triggers", "and takes down what the hooks staged before it failed (the event and both requests)", staged_total(c).await == before_total, &veto.request, "the outbox row count is unchanged", &format!("{before_total} -> {}", staged_total(c).await), TRIG);

    // Nesting: triggers that write through the service nest up to the cap.
    let chain_ok = app.post("/incidents", ta, body("chain:1/3", 1)).await;
    let committed: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.incidents WHERE title LIKE 'chain:%/3'").fetch_one(&c.owner).await.unwrap();
    r.check("D", "Hook triggers", "triggers that write through the generated service join the transaction: a chain of three commits all three rows", chain_ok.status == 201 && committed == 3, &chain_ok.request, "201; three incidents", &format!("{} rows {committed}", chain_ok.status), TRIG);
    let before_total = staged_total(c).await;
    let chain_deep = app.post("/incidents", ta, body("chain:1/50", 1)).await;
    let leaked: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.incidents WHERE title LIKE 'chain:%/50'").fetch_one(&c.owner).await.unwrap();
    r.check("D", "Hook triggers", "nesting beyond depth 8 is refused with the code trigger_depth_exceeded", chain_deep.status == 422 && chain_deep.has_code("trigger_depth_exceeded"), &chain_deep.request, "422 trigger_depth_exceeded", &chain_deep.got(), TRIG);
    r.check("D", "Hook triggers", "and the refusal rolls the whole chain back: no row, no staged request", leaked == 0 && staged_total(c).await == before_total, &chain_deep.request, "no chain row; the outbox row count is unchanged", &format!("rows {leaked}, outbox {before_total} -> {}", staged_total(c).await), TRIG);

    // A trigger no write fires is fired by hand code, in its transaction.
    let page = app.send("POST", &format!("/incidents/{id2}/page"), Some(ta), Some("{}".into()), &[("x-correlation-id", cid.as_str())]).await;
    let rows = staged(c, &id2).await;
    r.check("D", "Hook triggers", "a hand-fired trigger (after_page) runs when hand code calls fire_after_page: its emit and its hand method stage in one transaction", page.status == 200 && kinds_of(&rows, "IncidentPaged") == 1 && kinds_of(&rows, "OnCallPaged") == 1, &page.request, "200; IncidentPaged and OnCallPaged staged", &format!("{} {:?}", page.status, kinds(&rows)), TRIG);

    // A delete fires before_delete (a log line) and after_delete (an event).
    let del = app.delete(&format!("/incidents/{id2}"), ta).await;
    let rows = staged(c, &id2).await;
    r.check("D", "Hook triggers", "a soft delete fires after_delete", del.status == 200 && kinds_of(&rows, "IncidentDeleted") == 1, &del.request, "one IncidentDeleted", &format!("{} {:?}", del.status, kinds(&rows)), TRIG);
    if installed {
        let lines = capture.0.lock().unwrap().clone();
        r.check("D", "Hook triggers", "and before_delete, which runs before the row goes", lines.iter().any(|l| l.get("log_event").map(String::as_str) == Some("incident.deleting") && l.get("incident_id").is_some_and(|v| v.contains(&id2))), &del.request, "a before_delete log line for the incident", &format!("{} lines", lines.len()), TRIG);
    }

    // A schedule: entry is a registered job whose method the scheduler runs.
    use backbone_module_skeleton::application::triggers::{run_incident_scheduled, sweeps_run, IncidentTriggersImpl, INCIDENT_SCHEDULED};
    let registered = INCIDENT_SCHEDULED.iter().any(|s| s.name == "sweep_stale" && s.schedule == "every 5.minutes");
    let runs = sweeps_run();
    let ran = run_incident_scheduled(&IncidentTriggersImpl::default(), "sweep_stale").await;
    let unknown = run_incident_scheduled(&IncidentTriggersImpl::default(), "nope").await;
    r.check("D", "Hook triggers", "a `schedule:` entry registers a scheduled job (its name and schedule) and the generated runner calls its required method", registered && matches!(ran, Ok(true)) && sweeps_run() == runs + 1 && matches!(unknown, Ok(false)), "run_incident_scheduled(\"sweep_stale\")", "registered; the method ran once; an unknown name is not run", &format!("registered {registered}, ran {ran:?}, sweeps {runs} -> {}, unknown {unknown:?}", sweeps_run()), TRIG);
}

async fn area_d_events(c: &Ctx) {
    use backbone_outbox::{inbox, outbox, relay, OutboxError};
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const HOOKS: &str = "generated ticket_write_hooks.rs (generators/declared_events.rs) over backbone-core write_hooks.rs and service.rs";
    let unit = c.org.company_a;
    let pid = c.project(ta, unit, json!({})).await.id();
    let new_ticket = |title: &str, extra: Value| {
        let mut body = json!({ "orgUnitId": unit, "projectId": pid, "title": title });
        if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        body
    };

    // 1. a create stages exactly one TicketOpened, under the request's correlation id
    let cid = format!("conf-{}", &Uuid::new_v4().simple().to_string()[..10]);
    let title = uniq("EV");
    let cr = app.send("POST", "/tickets", Some(ta), Some(new_ticket(&title, json!({})).to_string()), &[("x-correlation-id", cid.as_str())]).await;
    let tid = cr.id();
    let rows = staged(c, &tid).await;
    r.check("D", "Declared events", "a create stages exactly one TicketOpened row", cr.status == 201 && kinds(&rows) == ["TicketOpened"], &cr.request, "201; one outbox row, TicketOpened", &format!("{} {:?}", cr.status, kinds(&rows)), HOOKS);
    r.check("D", "Declared events", "the staged row carries the request's correlation id", rows.first().and_then(|x| x.correlation_id.as_deref()) == Some(cid.as_str()), &cr.request, &format!("correlation_id {cid}"), &format!("{:?}", rows.first().map(|x| x.correlation_id.clone())), "backbone-core/src/write_hooks.rs (WriteCtx::current) and the org request scope's audit attribution");
    r.check("D", "Declared events", "the staged row carries the declared version and the payload read from the stored row", rows.first().is_some_and(|x| x.version == 2 && x.payload["id"] == json!(tid) && x.payload["project_id"] == json!(pid) && x.payload.as_object().map(|o| o.len()) == Some(2)), &cr.request, "version 2, payload {id, project_id}", &format!("{:?}", rows.first().map(|x| (x.version, x.payload.clone()))), HOOKS);
    let minted = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({}))).await;
    let minted_rows = staged(c, &minted.id()).await;
    r.check("D", "Declared events", "a create with no correlation header still stages one, under an id minted for the request", minted_rows.first().and_then(|x| x.correlation_id.as_deref()).is_some_and(|x| Uuid::parse_str(x).is_ok()), &minted.request, "a uuid correlation id", &format!("{:?}", minted_rows.first().map(|x| x.correlation_id.clone())), "backbone-auth/src/org.rs (the audit attribution mints one)");

    // 2. a create the guard or the database refuses stages nothing
    let before = staged_total(c).await;
    let by_guard = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({ "amount": -5 }))).await;
    let dup = app.post("/tickets", ta, new_ticket(&title, json!({}))).await;
    let after = staged_total(c).await;
    r.check("D", "Declared events", "a create the guard refuses stages no event", by_guard.status == 422 && after == before, &by_guard.request, "422; the outbox row count is unchanged", &format!("{} {before}->{after}", by_guard.status), HOOKS);
    r.check("D", "Declared events", "a create the database refuses (a duplicate title) stages no event", dup.status == 422 && after == before, &dup.request, "422; the outbox row count is unchanged", &format!("{} {before}->{after}", dup.status), HOOKS);

    // 3. an event that cannot be staged takes the write with it
    sqlx::raw_sql("ALTER TABLE conformance.outbox_events ADD CONSTRAINT ck_conf_poison CHECK (event_type <> 'TicketOpened') NOT VALID").execute(&c.owner).await.unwrap();
    let poisoned_title = uniq("EV");
    let before = staged_total(c).await;
    let poisoned = app.post("/tickets", ta, new_ticket(&poisoned_title, json!({}))).await;
    let kept = ticket_count(c, &poisoned_title).await;
    sqlx::raw_sql("ALTER TABLE conformance.outbox_events DROP CONSTRAINT ck_conf_poison").execute(&c.owner).await.unwrap();
    r.check("D", "Declared events", "when staging the event fails the create fails and no ticket row exists", poisoned.status == 500 && kept == 0 && staged_total(c).await == before, &poisoned.request, "500; no ticket row; no outbox row", &format!("{} tickets {kept}", poisoned.got()), "backbone-core/src/write_hooks.rs (WriteCtx::stage) and service.rs (atomically)");
    r.check("D", "Declared events", "the failed staging does not leak the outbox error to the client", !poisoned.raw.contains("ck_conf_poison") && !poisoned.raw.contains("outbox"), &poisoned.request, "a generic internal error", &poisoned.got(), "backbone-core/src/http.rs (a 500 answers INTERNAL_ERROR_MESSAGE)");

    // 4. transitions: staged when they happen, nothing when refused
    let big = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({ "amount": 500 }))).await.id();
    let small = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({ "amount": 50 }))).await.id();
    let over = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({ "amount": 2000 }))).await.id();
    for id in [&big, &small, &over] {
        app.post(&format!("/tickets/{id}/transitions/start"), ta, json!({})).await;
    }
    let verb = |id: &str, v: &str| format!("/tickets/{id}/transitions/{v}");
    let tcid = format!("conf-{}", &Uuid::new_v4().simple().to_string()[..10]);
    let res = app.send("POST", &verb(&big, "resolve"), Some(ta), Some("{}".into()), &[("x-correlation-id", tcid.as_str())]).await;
    let rows = staged(c, &big).await;
    r.check("D", "Declared events", "a transition stages its event (resolve stages TicketResolved)", res.status == 200 && kinds(&rows) == ["TicketOpened", "TicketResolved"], &res.request, "200; TicketOpened, TicketResolved", &format!("{} {:?}", res.status, kinds(&rows)), HOOKS);
    r.check("D", "Declared events", "a transition's event carries the request's correlation id and the stored row's values", rows.iter().find(|x| x.event_type == "TicketResolved").is_some_and(|x| x.correlation_id.as_deref() == Some(tcid.as_str()) && num(&x.payload["amount"]) == Some(500.0)), &res.request, "correlation id and amount 500", &format!("{:?}", rows.iter().find(|x| x.event_type == "TicketResolved")), HOOKS);
    let res = app.post(&verb(&small, "resolve"), ta, json!({})).await;
    r.check("D", "Declared events", "an event whose `if:` does not hold is not staged (amount 50 < 100)", res.status == 200 && kinds(&staged(c, &small).await) == ["TicketOpened"], &res.request, "200; only TicketOpened", &format!("{} {:?}", res.status, kinds(&staged(c, &small).await)), HOOKS);
    let before = staged_total(c).await;
    let guard_refused = app.post(&verb(&over, "resolve"), ta, json!({})).await;
    let wrong_state = app.post(&verb(&over, "archive"), ta, json!({})).await;
    r.check("D", "Declared events", "a refused transition stages nothing (a failing guard, a wrong state)", guard_refused.status == 422 && (400..500).contains(&wrong_state.status) && staged_total(c).await == before, &guard_refused.request, "422 and 4xx; the outbox row count is unchanged", &format!("{} {} {before}->{}", guard_refused.status, wrong_state.status, staged_total(c).await), HOOKS);
    let ar = app.post(&verb(&big, "archive"), ta, json!({})).await;
    let rows = staged(c, &big).await;
    r.check("D", "Declared events", "entering a state stages its event (archive enters archived: TicketArchived)", ar.status == 200 && kinds(&rows) == ["TicketOpened", "TicketResolved", "TicketArchived"], &ar.request, "200; TicketOpened, TicketResolved, TicketArchived", &format!("{} {:?}", ar.status, kinds(&rows)), HOOKS);

    // 5. update(title) fires only when the title changed
    let up = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({}))).await;
    let (uid, utitle) = (up.id(), up.data()["title"].as_str().unwrap_or_default().to_string());
    let same = app.patch(&format!("/tickets/{uid}"), ta, json!({ "title": utitle })).await;
    let other = app.patch(&format!("/tickets/{uid}"), ta, json!({ "note": "unrelated" })).await;
    let n_before = kinds(&staged(c, &uid).await).iter().filter(|k| **k == "TicketRetitled").count();
    let changed = app.patch(&format!("/tickets/{uid}"), ta, json!({ "title": uniq("NEW") })).await;
    let n_after = kinds(&staged(c, &uid).await).iter().filter(|k| **k == "TicketRetitled").count();
    r.check("D", "Declared events", "update(title) fires only when the title changed", same.status == 200 && other.status == 200 && n_before == 0 && changed.status == 200 && n_after == 1, &changed.request, "no TicketRetitled for the same title or another field; one for a new title", &format!("{} {} {} before {n_before} after {n_after}", same.status, other.status, changed.status), HOOKS);

    // 6. delete
    let del = app.delete(&format!("/tickets/{uid}"), ta).await;
    r.check("D", "Declared events", "a delete stages its event in the same transaction", del.status == 200 && kinds(&staged(c, &uid).await).contains(&"TicketDeleted"), &del.request, "200; TicketDeleted staged", &format!("{} {:?}", del.status, kinds(&staged(c, &uid).await)), HOOKS);

    // 7. a hand-staged event joins the flow's transaction
    let esc_ticket = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({}))).await.id();
    let esc = app.post(&format!("/tickets/{esc_ticket}/escalate"), ta, json!({ "note": "waiting" })).await;
    let rows = staged(c, &esc_ticket).await;
    r.check("D", "Hand-staged events", "a hand-staged declared event is staged by hand code inside the flow's transaction (TicketEscalated)", esc.status == 200 && kinds(&rows) == ["TicketOpened", "TicketEscalated"], &esc.request, "200; TicketOpened, TicketEscalated", &format!("{} {:?}", esc.status, kinds(&rows)), "ticket_handler.ext.rs (escalate) with backbone-orm in_write_tx_result");
    let before = staged_total(c).await;
    let missing = app.post(&format!("/tickets/{}/escalate", Uuid::new_v4()), ta, json!({ "note": "x" })).await;
    r.check("D", "Hand-staged events", "an escalation of an unknown ticket stages nothing", missing.status == 404 && staged_total(c).await == before, &missing.request, "404; the outbox row count is unchanged", &format!("{} {before}->{}", missing.status, staged_total(c).await), "ticket_handler.ext.rs (escalate)");

    // 8. a write hook that fails takes the write and what it staged down together
    let before = staged_total(c).await;
    let boom = app.post("/hooked-projects", ta, c.project_body(unit, &uniq("B"), "boom")).await;
    let boom_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.projects WHERE name = 'boom'").fetch_one(&c.owner).await.unwrap();
    r.check("D", "Write hooks", "a hook that fails after staging rolls back the write and its event", boom.status == 422 && boom_rows == 0 && staged_total(c).await == before, &boom.request, "422; no project row; no outbox row", &format!("{} projects {boom_rows} outbox {before}->{}", boom.status, staged_total(c).await), "backbone-core/src/service.rs (atomically over the hooks)");
    let calm = app.send("POST", "/hooked-projects", Some(ta), Some(c.project_body(unit, &uniq("B"), "Calm").to_string()), &[("x-correlation-id", cid.as_str())]).await;
    let calm_rows = staged(c, &calm.data()["id"].as_str().unwrap_or_default().to_string()).await;
    r.check("D", "Write hooks", "a hand-written WriteHooks stages its event with the write, under the correlation id", calm.status == 201 && kinds(&calm_rows) == ["ProjectNoted"] && calm_rows[0].correlation_id.as_deref() == Some(cid.as_str()), &calm.request, "201; ProjectNoted with the correlation id", &format!("{} {:?}", calm.status, calm_rows.iter().map(|x| (&x.event_type, &x.correlation_id)).collect::<Vec<_>>()), "backbone-core/src/write_hooks.rs");

    // 9. bulk: the rows and their events are all-or-nothing
    let dup_title = uniq("BK");
    let before = staged_total(c).await;
    let refused = app.post("/tickets/bulk", ta, json!([new_ticket(&dup_title, json!({})), new_ticket(&uniq("BK"), json!({})), new_ticket(&dup_title, json!({}))])).await;
    r.check("D", "Declared events", "a bulk create refused partway commits no row and no event", (400..500).contains(&refused.status) && ticket_count(c, &dup_title).await == 0 && staged_total(c).await == before, &refused.request, "4xx; no ticket; the outbox row count is unchanged", &format!("{} tickets {} outbox {before}->{}", refused.status, ticket_count(c, &dup_title).await, staged_total(c).await), "backbone-core/src/service.rs (bulk_create inside the write transaction)");
    let ok = app.post("/tickets/bulk", ta, json!([new_ticket(&uniq("BK"), json!({})), new_ticket(&uniq("BK"), json!({})), new_ticket(&uniq("BK"), json!({}))])).await;
    let bulk_ids: Vec<String> = ok.data()["items"].as_array().cloned().unwrap_or_default().iter().filter_map(|i| i["id"].as_str().map(str::to_string)).collect();
    let opened: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.outbox_events WHERE event_type = 'TicketOpened' AND aggregate_id = ANY($1)").bind(&bulk_ids).fetch_one(&c.owner).await.unwrap();
    r.check("D", "Declared events", "a bulk create stages one event per row with the rows", ok.status == 201 && bulk_ids.len() == 3 && opened == 3, &ok.request, "201; three tickets, three TicketOpened", &format!("{} ids {} events {opened}", ok.status, bulk_ids.len()), HOOKS);
    // two tickets of one project cannot share a title: the bulk patch fails on the second, whole
    let (a, b) = (bulk_ids[0].clone(), bulk_ids[1].clone());
    let same_project = app.patch("/tickets/bulk", ta, json!({ "ids": [a, b], "patch": { "title": uniq("SAME") } })).await;
    let retitled: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.outbox_events WHERE event_type = 'TicketRetitled' AND aggregate_id = ANY($1)").bind(&bulk_ids).fetch_one(&c.owner).await.unwrap();
    r.check("D", "Declared events", "a bulk patch refused partway changes no row and stages no event", (400..500).contains(&same_project.status) && retitled == 0, &same_project.request, "4xx; no TicketRetitled", &format!("{} retitled {retitled}", same_project.status), "backbone-core/src/service.rs (bulk_partial_update inside the write transaction)");
    let p2 = c.project(ta, unit, json!({})).await.id();
    let t2 = app.post("/tickets", ta, json!({ "orgUnitId": unit, "projectId": p2, "title": uniq("BK") })).await.id();
    let shared = uniq("SHARED");
    let ok = app.patch("/tickets/bulk", ta, json!({ "ids": [bulk_ids[0].clone(), t2.clone()], "patch": { "title": shared } })).await;
    let retitled: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.outbox_events WHERE event_type = 'TicketRetitled' AND aggregate_id = ANY($1)").bind(vec![bulk_ids[0].clone(), t2.clone()]).fetch_one(&c.owner).await.unwrap();
    r.check("D", "Declared events", "a bulk patch stages one event per changed row", ok.status == 200 && retitled == 2, &ok.request, "200; two TicketRetitled", &format!("{} retitled {retitled}", ok.got()), HOOKS);

    // 10. delivery: the relay drains, dead-letters and re-emits; a consumer dedupes a replay
    let drain_ticket = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({}))).await.id();
    let id: Uuid = sqlx::query_scalar("SELECT id FROM conformance.outbox_events WHERE aggregate_id = $1 AND event_type = 'TicketOpened'").bind(&drain_ticket).fetch_one(&c.owner).await.unwrap();
    let published = relay::drain_once(&c.owner, "conformance", 1000, |_rec| async { Ok(()) }).await;
    r.check("D", "Relay", "a relay drain marks the staged row published", published.is_ok() && staged(c, &drain_ticket).await.iter().all(|x| x.published) && outbox::pending_count(&c.owner, "conformance").await.unwrap() == 0, "relay::drain_once", "published, nothing pending", &format!("{published:?}"), "backbone-outbox/src/relay.rs");
    let retry_ticket = app.post("/tickets", ta, new_ticket(&uniq("EV"), json!({}))).await.id();
    let retry_id: Uuid = sqlx::query_scalar("SELECT id FROM conformance.outbox_events WHERE aggregate_id = $1 AND event_type = 'TicketOpened'").bind(&retry_ticket).fetch_one(&c.owner).await.unwrap();
    let failed = relay::drain_once(&c.owner, "conformance", 1000, |_rec| async { Err(OutboxError::Publish("the bus is down".into())) }).await;
    let still_pending = !staged(c, &retry_ticket).await[0].published && outbox::dead_count(&c.owner, "conformance").await.unwrap() == 0;
    let dead = relay::drain_once(&c.owner, "conformance", 1000, |_rec| async { Err(OutboxError::Exhausted("gave up".into())) }).await;
    let dead_count = outbox::dead_count(&c.owner, "conformance").await.unwrap();
    r.check("D", "Relay", "a failing publish leaves the row pending, an exhausted one makes it dead", failed.is_ok() && still_pending && dead.is_ok() && dead_count == 1, "relay::drain_once with a failing sink", "pending, then dead (dead_count 1)", &format!("pending-after-failure {still_pending}, dead_count {dead_count}"), "backbone-outbox/src/relay.rs");
    let revived = relay::reemit(&c.owner, "conformance", retry_id).await;
    let redelivered = relay::drain_once(&c.owner, "conformance", 1000, |_rec| async { Ok(()) }).await;
    r.check("D", "Relay", "re-emitting a dead row delivers it again", revived.as_ref().is_ok_and(|b| *b) && redelivered.is_ok() && staged(c, &retry_ticket).await[0].published && outbox::dead_count(&c.owner, "conformance").await.unwrap() == 0, "relay::reemit then drain_once", "revived, published, no dead rows", &format!("{revived:?} {redelivered:?}"), "backbone-outbox/src/relay.rs");
    let first = inbox::once(&c.owner, "conformance", "conf-consumer", id).await;
    let replay = inbox::once(&c.owner, "conformance", "conf-consumer", id).await;
    r.check("D", "Relay", "a consumer's inbox::once dedupes a replay of the same event", first.as_ref().is_ok_and(|b| *b) && replay.as_ref().is_ok_and(|b| !*b), "inbox::once twice for one event id", "true then false", &format!("{first:?} {replay:?}"), "backbone-outbox/src/inbox.rs");
}

// =============================================================================
// E. State machine
// =============================================================================

async fn area_e(c: &Ctx) {
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const H: &str = "generated presentation/http/ticket_handler.rs (metaphor-plugin-schema/src/generators/handler.rs:926-1230)";
    let pid = c.project(ta, c.org.company_a, json!({})).await.id();
    let tid = c.ticket(ta, c.org.company_a, &pid, json!({ "amount": 2000 })).await.id();
    let tr = |verb: &str| format!("/tickets/{tid}/transitions/{verb}");

    let s = app.post(&tr("start"), ta, json!({})).await;
    r.check("E", "Transitions", "an allowed transition (start: open → in_progress) answers 200 and moves the state", s.status == 200 && s.data()["status"] == json!("in_progress"), &s.request, "200, status in_progress", &s.got(), H);
    let bad = app.post(&tr("archive"), ta, json!({})).await;
    r.check("E", "Transitions", "a disallowed transition (archive from in_progress) is refused with 4xx", (400..500).contains(&bad.status), &bad.request, "4xx", &bad.got(), H);
    r.check("E", "Transitions", "a disallowed transition names a stable code", bad.has_code("transition_not_allowed"), &bad.request, "a transition_not_allowed violation", &bad.got(), "metaphor-plugin-schema/src/generators/handler.rs (the 400 'Transition not allowed from current state' is ApiResponse::error with no code)");
    let g = app.post(&tr("resolve"), ta, json!({})).await;
    r.check("E", "Transition guard", "a transition whose guard does not hold (amount <= 1000) is refused 422", g.status == 422, &g.request, "422", &g.got(), H);
    r.check("E", "Transition guard", "a refused guard names a stable code", g.has_code("transition_guard_failed"), &g.request, "a transition_guard_failed violation", &g.got(), "metaphor-plugin-schema/src/generators/handler.rs (guard refusal is ApiResponse::error with no code)");
    app.patch(&format!("/tickets/{tid}"), ta, json!({ "amount": 500 })).await;
    let g = app.post(&tr("resolve"), ta, json!({})).await;
    r.check("E", "Transition guard", "the same transition passes once its guard holds", g.status == 200 && g.data()["status"] == json!("resolved"), &g.request, "200, resolved", &g.got(), H);
    let a = app.post(&tr("archive"), ta, json!({})).await;
    r.check("E", "Final state", "archive enters the final state", a.status == 200 && a.data()["status"] == json!("archived"), &a.request, "200, archived", &a.got(), H);
    let leave = app.post(&tr("start"), ta, json!({})).await;
    r.check("E", "Final state", "a final state cannot be left by an ordinary edge", (400..500).contains(&leave.status), &leave.request, "4xx", &leave.got(), H);
    let ro = app.post(&tr("reopen"), ta, json!({})).await;
    r.check("E", "Final state", "the from: \"*\" edge leaves the final state", ro.status == 200 && ro.data()["status"] == json!("open"), &ro.request, "200, open", &ro.got(), "generated state machine can_transition (wildcard edge)");
    let gp = app.patch(&format!("/tickets/{tid}"), ta, json!({ "status": "in_progress" })).await;
    r.check("E", "Generic writes", "a generic PATCH of the state field is refused: 422 state_field_verbs_only", gp.status == 422 && gp.violation("status", "state_field_verbs_only"), &gp.request, "422 state_field_verbs_only", &gp.got(), "generated TicketRowRules");
    let gput = app.put(&format!("/tickets/{tid}"), ta, json!({ "orgUnitId": c.org.company_a, "projectId": pid, "title": "Put state", "amount": 500, "reference": Uuid::new_v4(), "status": "resolved" })).await;
    let still: String = sqlx::query_scalar("SELECT status::text FROM conformance.tickets WHERE id = $1").bind(Uuid::parse_str(&tid).unwrap()).fetch_one(&c.owner).await.unwrap_or_default();
    r.check("E", "Generic writes", "a generic PUT cannot move the state field", still == "open", &gput.request, "status stays open (422 or the key refused)", &format!("{}; stored {still}", gput.got()), "generated TicketRowRules / Update DTO");
    let nop = app.post(&tr("start"), &c.clerk, json!({})).await;
    r.check("E", "Permissions", "a caller without <model>:transition:<verb> is refused 403", nop.status == 403, &nop.request, "403", &nop.got(), H);
    let other = app.post(&tr("start"), &c.tb, json!({})).await;
    r.check("E", "Scoping", "another company's caller cannot transition the ticket (404)", other.status == 404, &other.request, "404", &other.got(), H);

    // the database trigger refuses an undeclared move from any writer
    let mut tx = c.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.scope_unit_ids', $1, true)").bind(format!("{},{}", c.org.company_a, c.org.root)).execute(&mut *tx).await.unwrap();
    let raw = sqlx::query("UPDATE conformance.tickets SET status = 'archived' WHERE id = $1").bind(Uuid::parse_str(&tid).unwrap()).execute(&mut *tx).await;
    let _ = tx.rollback().await;
    match raw {
        Err(e) => {
            let db_code = e.as_database_error().and_then(|d| d.code()).map(|c| c.to_string()).unwrap_or_default();
            let mapped = backbone_core::persistence::RepositoryError::from(e);
            let ok = matches!(&mapped, backbone_core::persistence::RepositoryError::Violations(vs) if vs.iter().any(|v| v.path == "status" && v.code == "state_transition_not_declared" && !v.message.contains("tg_")));
            r.check("E", "DB trigger", "raw UPDATE refused by the transition trigger and mapped to state_transition_not_declared on status", ok && db_code == "23514", "UPDATE conformance.tickets SET status = 'archived' (open → archived)", "23514; Violation {path: status, code: state_transition_not_declared}", &format!("{db_code} {mapped:?}"), "generators/state_transition_check.rs:108-165 / backbone-core/src/persistence/traits.rs:150");
        }
        Ok(_) => {
            r.check("E", "DB trigger", "raw UPDATE refused by the transition trigger and mapped to state_transition_not_declared on status", false, "UPDATE conformance.tickets SET status = 'archived'", "refused", "the update went through", "generators/state_transition_check.rs");
        }
    }

    // the hand_set field bound to a machine: verbs move it, generic writes cannot reach it
    let aid: Uuid = sqlx::query_scalar("INSERT INTO conformance.approvals (company_id, subject) VALUES ($1, 'Seeded') RETURNING id").bind(c.org.company_a).fetch_one(&c.owner).await.unwrap();
    let ap = |verb: &str| format!("/approvals/{aid}/transitions/{verb}");
    let gp = app.patch(&format!("/approvals/{aid}"), ta, json!({ "state": "approved" })).await;
    r.check("E", "hand_set + machine", "a generic PATCH of the hand_set machine field is refused (no generic write route: 405)", gp.status == 405, &gp.request, "405", &gp.got(), "ast/model.rs:148 (mounts_writes)");
    let sb = app.post(&ap("submit"), ta, json!({})).await;
    r.check("E", "hand_set + machine", "the generated verb moves the hand_set field (submit: draft → submitted)", sb.status == 200 && sb.data()["state"] == json!("submitted"), &sb.request, "200, submitted", &sb.got(), "generated approval_handler.rs + as_transition (backbone-core/src/service.rs:999 lets state fields through inside a transition)");
    let again = app.post(&ap("submit"), ta, json!({})).await;
    r.check("E", "hand_set + machine", "a verb from the wrong state is refused", (400..500).contains(&again.status), &again.request, "4xx", &again.got(), "generated approval_handler.rs");
    let ok = app.post(&ap("approve"), ta, json!({})).await;
    r.check("E", "hand_set + machine", "the final verb (approve) completes", ok.status == 200 && ok.data()["state"] == json!("approved"), &ok.request, "200, approved", &ok.got(), "generated approval_handler.rs");
    let g = app.get(&format!("/approvals/{aid}"), ta).await;
    r.check("E", "hand_set + machine", "the read routes of a hand_set entity are mounted", g.status == 200, &g.request, "200", &g.got(), "generated create_approval_read_routes");
}

// =============================================================================
// F. Extension
// =============================================================================

async fn area_f(c: &Ctx) {
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const X: &str = "ticket_handler.ext.rs (custom routes) / lib.ext.rs (mounting)";
    let pid = c.project(ta, c.org.company_a, json!({})).await.id();
    let tid = c.ticket(ta, c.org.company_a, &pid, json!({})).await.id();

    let e = app.post(&format!("/tickets/{tid}/escalate"), ta, json!({ "note": "customer waiting" })).await;
    let note = e.data()["note"].as_str().unwrap_or("").to_string();
    r.check("F", "Custom route", "the custom escalate route is reachable through the stack and moves the ticket by its verb", e.status == 200 && e.data()["status"] == json!("in_progress"), &e.request, "200, in_progress", &e.got(), X);
    r.check("F", "Custom route", "the custom route reads the org scope the stack bound (the acting unit stamped in the note)", note.contains(&c.org.company_a.to_string()), &e.request, "note names company A", &note, X);
    let n = app.send("POST", &format!("/tickets/{tid}/escalate"), None, Some(json!({ "note": "x" }).to_string()), &[]).await;
    r.check("F", "Custom route", "the custom route is guarded: no token answers 401", n.status == 401, &n.request, "401", &n.got(), X);
    let b = app.post(&format!("/tickets/{tid}/escalate"), &c.tb, json!({ "note": "x" })).await;
    r.check("F", "Custom route", "the custom route is fenced: another company's caller gets 404", b.status == 404, &b.request, "404", &b.got(), X);
    let g = app.post(&format!("/tickets/{tid}/escalate"), ta, json!({ "note": "x".repeat(10) })).await;
    r.check("F", "Custom route", "the custom route's write still meets the database's transition trigger (in_progress → in_progress is not a move; archived → in_progress is refused)", g.status == 200, &g.request, "200 (no state change)", &g.got(), X);
    for verb in ["resolve", "archive"] {
        app.post(&format!("/tickets/{tid}/transitions/{verb}"), ta, json!({})).await;
    }
    let refused = app.post(&format!("/tickets/{tid}/escalate"), ta, json!({ "note": "late" })).await;
    r.check("F", "Custom route", "escalating an archived ticket is refused by the transition trigger: 422 state_transition_not_declared", refused.status == 422 && refused.violation("status", "state_transition_not_declared"), &refused.request, "422 {path: status, code: state_transition_not_declared}", &refused.got(), X);
    let budget = app.post(&format!("/tickets/{}/escalate", c.ticket(ta, c.org.company_a, &pid, json!({ "budgetCap": 1 })).await.id()), ta, json!({ "note": "y" })).await;
    r.check("F", "Custom route", "the custom route's write passes the schema guard (a valid row is accepted)", budget.status == 200, &budget.request, "200", &budget.got(), X);

    let t2 = c.ticket(ta, c.org.company_a, &pid, json!({})).await.id();
    for verb in ["start", "resolve"] {
        app.post(&format!("/tickets/{t2}/transitions/{verb}"), ta, json!({})).await;
    }
    let ar = app.post(&format!("/tickets/{t2}/archive-with-reason"), ta, json!({ "reason": "duplicate" })).await;
    r.check("F", "Custom service method", "the custom service method archives with a reason", ar.status == 200 && ar.data()["status"] == json!("archived") && ar.data()["archiveReason"] == json!("duplicate"), &ar.request, "200, archived, archiveReason duplicate", &ar.got(), "ticket_service.ext.rs::archive_with_reason");
    let t3 = c.ticket(ta, c.org.company_a, &pid, json!({})).await.id();
    let early = app.post(&format!("/tickets/{t3}/archive-with-reason"), ta, json!({ "reason": "too early" })).await;
    r.check("F", "Custom service method", "the custom service method consults the generated machine: archiving an open ticket answers 422 transition_not_allowed", early.status == 422 && early.has_code("transition_not_allowed"), &early.request, "422 transition_not_allowed", &early.got(), "ticket_service.ext.rs::archive_with_reason");
    let hp = app.send("GET", "/hooked-projects", None, None, &[]).await;
    r.check("F", "Extension routes", "the extension's generic routes sit behind the stack (no token answers 401)", hp.status == 401, &hp.request, "401", &hp.got(), X);
    let hl = app.get(&format!("/hooked-projects?id={pid}"), &c.tb).await;
    r.check("F", "Extension routes", "the extension's generic routes are fenced (company B sees none of A's projects)", hl.status == 200 && hl.items().is_empty(), &hl.request, "200, empty", &hl.got(), X);
}

// =============================================================================
// F. Workflows run by the engine
// =============================================================================

/// One staged outbox row of an aggregate, as the engine's relay would hand it to the engine.
async fn outbox_record(c: &Ctx, aggregate_id: &str, event_type: &str) -> Option<backbone_core::write_hooks::OutboxRecord> {
    let row: Option<(Uuid, String, String, String, Uuid, Value, chrono::DateTime<chrono::Utc>, Option<String>, Option<String>, i32)> = sqlx::query_as(
        "SELECT id, event_type, aggregate_type, aggregate_id, company_id, payload, occurred_at, correlation_id, causation_id, version \
         FROM conformance.outbox_events WHERE aggregate_id = $1 AND event_type = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(aggregate_id)
    .bind(event_type)
    .fetch_optional(&c.owner)
    .await
    .unwrap();
    row.map(|(id, event_type, aggregate_type, aggregate_id, company_id, payload, occurred_at, correlation_id, causation_id, version)| {
        backbone_core::write_hooks::OutboxRecord { id, event_type, aggregate_type, aggregate_id, company_id, payload, occurred_at, correlation_id, causation_id, version }
    })
}

/// A request event for the engine, as another module's publisher would stage it.
fn request_event(kind: &str, company: Uuid, payload: Value, correlation: &str) -> backbone_core::write_hooks::OutboxRecord {
    let aggregate = payload["incident_id"].as_str().unwrap_or("-").to_string();
    backbone_core::write_hooks::OutboxRecord::new(kind, "Incident", aggregate, company, payload, chrono::Utc::now()).with_correlation_id(correlation)
}

#[derive(Debug, Clone)]
struct Inst {
    id: Uuid,
    status: String,
    step: Option<String>,
    attempts: i32,
    context: Value,
    correlation: Option<String>,
    trigger: Uuid,
    due: bool,
}

async fn instance_of(c: &Ctx, workflow: &str, trigger: Uuid) -> Option<Inst> {
    let row: Option<(Uuid, String, Option<String>, i32, Value, Option<String>, Uuid, bool)> = sqlx::query_as(
        "SELECT id, status, current_step, attempts, context, correlation_id, trigger_event_id, wait_due_at IS NOT NULL \
         FROM conformance.bb_workflow_instances WHERE workflow = $1 AND trigger_event_id = $2",
    )
    .bind(workflow)
    .bind(trigger)
    .fetch_optional(&c.owner)
    .await
    .unwrap();
    row.map(|(id, status, step, attempts, context, correlation, trigger, due)| Inst { id, status, step, attempts, context, correlation, trigger, due })
}

async fn steps_of(c: &Ctx, id: Uuid) -> Vec<(String, String)> {
    sqlx::query_as("SELECT step, outcome FROM conformance.bb_workflow_steps WHERE instance_id = $1 ORDER BY seq")
        .bind(id)
        .fetch_all(&c.owner)
        .await
        .unwrap()
}

async fn force_due(c: &Ctx, id: Uuid) {
    sqlx::query("UPDATE conformance.bb_workflow_instances SET wait_due_at = now() - interval '1 second' WHERE id = $1").bind(id).execute(&c.owner).await.unwrap();
}

async fn area_f_workflows(c: &Ctx) {
    use backbone_core::write_hooks::OutboxRecord;
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    const WF: &str = "workflows/incident_escalation.workflow.yaml and incident_checklist.workflow.yaml (executor: engine): the generated *_workflow.rs, backbone-core's workflow engine";
    let tasks = c.module.background_tasks();
    let engine = tasks.engine().clone();
    let company = c.org.company_a;
    let new_incident = |title: String, severity: i64| {
        let body = json!({ "companyId": company, "title": title, "severity": severity });
        async move { app.post("/incidents", ta, body).await }
    };
    let escalation = |incident: &str, corr: &str| request_event("IncidentEscalationRequested", company, json!({ "incident_id": incident }), corr);

    // 1. An event starts one instance, and the same event delivered twice still makes one.
    let low = new_incident(uniq("WFL"), 1).await.id();
    let corr = format!("wf-{}", &Uuid::new_v4().simple().to_string()[..8]);
    let start = escalation(&low, &corr);
    let first = tasks.deliver(start.clone()).await;
    let again = tasks.deliver(start.clone()).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.bb_workflow_instances WHERE workflow = 'IncidentEscalation' AND trigger_event_id = $1").bind(start.id).fetch_one(&c.owner).await.unwrap();
    let inst = instance_of(c, "IncidentEscalation", start.id).await;
    r.check("F", "Workflow engine", "an event starts one instance, and the same event delivered twice still makes one", first.is_ok() && again.is_ok() && count == 1, "deliver IncidentEscalationRequested twice", "ok, ok; one instance", &format!("{first:?} {again:?} instances {count}"), WF);
    let inst = inst.expect("the instance exists");
    let steps = steps_of(c, inst.id).await;
    r.check("F", "Workflow engine", "the instance runs its steps to the wait and stops there: waiting on await_ack, each step logged, the start event's correlation id kept", inst.status == "waiting" && inst.step.as_deref() == Some("await_ack") && steps.iter().map(|s| s.0.as_str()).collect::<Vec<_>>() == ["fetch_incident", "choose", "await_ack"] && inst.correlation.as_deref() == Some(corr.as_str()), "SELECT the instance and its step log", "waiting, [fetch_incident, choose, await_ack], correlation kept", &format!("{} {:?} {:?} {:?}", inst.status, inst.step, steps, inst.correlation), WF);
    r.check("F", "Workflow engine", "a step's `set:` and the trigger's `extract:` land in the instance context (severity read from the incident through the generated service)", inst.context["incident_id"] == json!(low) && inst.context["severity"] == json!(1), "SELECT context", "incident_id and severity 1", &inst.context.to_string(), WF);
    let paged_low = outbox_record(c, &low, "ResponderPaged").await;
    r.check("F", "Workflow engine", "a condition step takes its branch: a severity 1 incident is not paged", paged_low.is_none() && !steps.iter().any(|s| s.0 == "page_now"), "look for the page_now step and a ResponderPaged row", "none", &format!("{paged_low:?}"), WF);

    // 2. A bespoke verb runs inside the step; a declared function is called by an expression.
    let high = new_incident(uniq("WFH"), 4).await.id();
    let hstart = escalation(&high, "wf-high");
    tasks.deliver(hstart.clone()).await.unwrap();
    let hinst = instance_of(c, "IncidentEscalation", hstart.id).await.unwrap();
    let paged = outbox_record(c, &high, "ResponderPaged").await;
    r.check("F", "Workflow engine", "a bespoke verb runs through the Actions method of the extension, inside the step's transaction, and a declared function evaluates in an expression (who = on_call_name(severity))", paged.as_ref().is_some_and(|p| p.payload["who"] == json!("responder-sev4")) && steps_of(c, hinst.id).await.iter().any(|s| s.0 == "page_now" && s.1 == "ok"), "start the workflow for a severity 4 incident", "a ResponderPaged row naming responder-sev4; page_now ok", &format!("{:?}", paged.map(|p| p.payload)), WF);
    let requested = outbox_record(c, &hinst.id.to_string(), "NotificationRequested").await;
    r.check("F", "Workflow engine", "`send_email` stages a NotificationRequested on the step's transaction (the effect is the relay's, never the step's): channel, template, recipient, variables and the step that asked", requested.as_ref().is_some_and(|n| n.payload["channel"] == json!("email") && n.payload["template"] == json!("incident_paged") && n.payload["to"] == json!("responder-sev4") && n.payload["data"]["incident_id"] == json!(high) && n.payload["data"]["subject"] == json!("Incident paged") && n.payload["trigger"] == json!("IncidentEscalation.tell_oncall") && n.correlation_id.as_deref() == Some("wf-high")), "the NotificationRequested row of the instance", "email, incident_paged, responder-sev4, trigger IncidentEscalation.tell_oncall", &format!("{:?}", requested.map(|n| n.payload)), WF);
    r.check("F", "Workflow engine", "a row a step stages carries the instance's correlation id and the starting event as its cause", {
        let p = outbox_record(c, &high, "ResponderPaged").await;
        p.is_some_and(|p| p.correlation_id.as_deref() == Some("wf-high") && p.causation_id.as_deref() == Some(hstart.id.to_string().as_str()))
    }, "read the staged row", "correlation wf-high, causation the start event's id", "see the staged row", WF);

    // 3. A wait resumes on the matching event only. The event is the one the Incident write hooks stage
    // when the machine enters `acknowledged`, taken from the outbox as the relay would.
    let other = new_incident(uniq("WFO"), 1).await.id();
    let ack_other = app.post(&format!("/incidents/{other}/transitions/acknowledge"), ta, json!({})).await;
    let other_event = outbox_record(c, &other, "IncidentAcknowledged").await;
    let ignored = match &other_event {
        Some(e) => tasks.deliver(e.clone()).await.is_ok(),
        None => false,
    };
    let still = instance_of(c, "IncidentEscalation", start.id).await.unwrap();
    r.check("F", "Workflow engine", "a wait ignores an event whose key does not match: another incident's acknowledgement leaves the instance waiting", ack_other.status == 200 && ignored && still.status == "waiting", "acknowledge a different incident and deliver its IncidentAcknowledged", "the first instance is still waiting", &format!("{} {}", ack_other.got(), still.status), WF);
    let ack = app.post(&format!("/incidents/{low}/transitions/acknowledge"), ta, json!({})).await;
    let ack_event = outbox_record(c, &low, "IncidentAcknowledged").await.expect("the write hooks staged IncidentAcknowledged");
    let delivered = tasks.deliver(ack_event.clone()).await;
    let done = instance_of(c, "IncidentEscalation", start.id).await.unwrap();
    let done_steps = steps_of(c, done.id).await;
    r.check("F", "Workflow engine", "a wait resumes on the matching event: acknowledging the incident (the real transition, the real staged event) completes the instance", ack.status == 200 && delivered.is_ok() && done.status == "completed" && done.context["outcome"] == json!("resolved"), "acknowledge, then deliver the staged IncidentAcknowledged", "completed, outcome resolved", &format!("{} {:?} {}", ack.got(), done.status, done.context["outcome"]), WF);
    let stored: Option<String> = sqlx::query_scalar("SELECT status::text FROM conformance.incidents WHERE id = $1").bind(Uuid::parse_str(&low).unwrap()).fetch_optional(&c.owner).await.unwrap();
    r.check("F", "Workflow engine", "a transition step moves the record through its state machine: the incident is `resolved`", stored.as_deref() == Some("resolved"), "SELECT status", "resolved", &format!("{stored:?}"), WF);
    let resolved_events = staged(c, &low).await;
    r.check("F", "Workflow engine", "an entity action goes through the generated service, so the entity's own triggers and declared events apply: the machine's `after_update_status_resolved` staged IncidentResolved", kinds(&resolved_events).contains(&"IncidentResolved"), "the events staged for the incident", "IncidentResolved among them", &format!("{:?}", kinds(&resolved_events)), WF);
    r.check("F", "Workflow engine", "the step log shows each stage: waiting, resumed, then the transition, the event and the terminal", done_steps.iter().map(|s| (s.0.as_str(), s.1.as_str())).collect::<Vec<_>>().ends_with(&[("await_ack", "waiting"), ("await_ack", "resumed"), ("resolve_now", "ok"), ("announce", "ok"), ("done", "terminal")]), "SELECT the step log", "... await_ack waiting, await_ack resumed, resolve_now ok, announce ok, done terminal", &format!("{done_steps:?}"), WF);
    // The event is staged under the workflow instance (its aggregate), carrying the incident in its payload.
    let handled = outbox_record(c, &done.id.to_string(), "IncidentEscalationHandled").await;
    r.check("F", "Workflow engine", "`emit_event` stages a declared event on the step's transaction with the instance's correlation and causation", handled.as_ref().is_some_and(|h| h.payload["outcome"] == json!("resolved") && h.correlation_id.as_deref() == Some(corr.as_str()) && h.causation_id.as_deref() == Some(start.id.to_string().as_str())), "the IncidentEscalationHandled row", "outcome resolved; correlation and causation of the instance", &format!("{:?}", handled.map(|h| (h.payload, h.correlation_id, h.causation_id))), WF);
    let replay = tasks.deliver(ack_event).await;
    let after = steps_of(c, done.id).await;
    r.check("F", "Workflow engine", "a resume event delivered again changes nothing (the relay delivers at least once)", replay.is_ok() && after.len() == done_steps.len(), "deliver the same IncidentAcknowledged again", "no new step", &format!("{} -> {} steps", done_steps.len(), after.len()), WF);

    // 4. A wait that times out takes its branch through the sweep; the refused transition takes the failure branch.
    let quiet = new_incident(uniq("WFQ"), 1).await.id();
    let qstart = escalation(&quiet, "wf-quiet");
    tasks.deliver(qstart.clone()).await.unwrap();
    let qinst = instance_of(c, "IncidentEscalation", qstart.id).await.unwrap();
    let idle = engine.sweep().await.unwrap();
    force_due(c, qinst.id).await;
    let swept = engine.sweep().await.unwrap();
    let qdone = instance_of(c, "IncidentEscalation", qstart.id).await.unwrap();
    let qsteps = steps_of(c, qinst.id).await;
    let handled_by: Option<Option<String>> = sqlx::query_scalar("SELECT handled_by FROM conformance.incidents WHERE id = $1").bind(Uuid::parse_str(&quiet).unwrap()).fetch_optional(&c.owner).await.unwrap();
    r.check("F", "Workflow engine", "a wait expires through workflow_timer_sweep when its due time is in the past (and not before): the on_timeout branch runs", idle.timed_out.is_empty() && swept.timed_out == vec![qinst.id] && qsteps.iter().any(|s| s == &("await_ack".to_string(), "timed_out".to_string())) && handled_by == Some(Some("escalated".into())), "sweep, force wait_due_at into the past, sweep", "the first sweep finds nothing; the second times the wait out and runs reassign", &format!("{:?} {:?} {handled_by:?}", idle.timed_out, swept.timed_out), WF);
    r.check("F", "Workflow engine", "a transition the machine refuses (resolve from open) is a step failure: the failure branch runs and the error reaches the expression", qdone.status == "completed" && qsteps.iter().any(|s| s == &("try_resolve".to_string(), "failed".to_string())) && qsteps.iter().any(|s| s == &("refused".to_string(), "ok".to_string())) && qdone.context["outcome"] == json!("escalated") && qdone.context["error"].as_str().is_some_and(|e| e.contains("not allowed") || e.contains("transition")), "let the wait time out; try_resolve asks the machine for resolve from open", "try_resolve failed, refused ok, outcome escalated", &format!("{:?} {qsteps:?} {}", qdone.status, qdone.context["error"]), WF);
    let still_open: Option<String> = sqlx::query_scalar("SELECT status::text FROM conformance.incidents WHERE id = $1").bind(Uuid::parse_str(&quiet).unwrap()).fetch_optional(&c.owner).await.unwrap();
    r.check("F", "Workflow engine", "the refused transition changed nothing: the incident is still open", still_open.as_deref() == Some("open"), "SELECT status", "open", &format!("{still_open:?}"), WF);

    // 5. An instance past its deadline expires.
    let slow = new_incident(uniq("WFD"), 1).await.id();
    let sstart = escalation(&slow, "wf-slow");
    tasks.deliver(sstart.clone()).await.unwrap();
    let sinst = instance_of(c, "IncidentEscalation", sstart.id).await.unwrap();
    sqlx::query("UPDATE conformance.bb_workflow_instances SET wait_due_at = NULL, deadline_at = now() - interval '1 second' WHERE id = $1").bind(sinst.id).execute(&c.owner).await.unwrap();
    let report = engine.sweep().await.unwrap();
    let expired = instance_of(c, "IncidentEscalation", sstart.id).await.unwrap();
    r.check("F", "Workflow engine", "an instance past the workflow's deadline (config.timeout) ends `expired`", report.expired.contains(&sinst.id) && expired.status == "expired", "force deadline_at into the past and sweep", "expired", &format!("{:?} {}", report.expired, expired.status), WF);

    // 6. Atomic steps, retries with backoff, compensation in reverse, idempotent compensation.
    let before_probe: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.outbox_events WHERE event_type = 'WorkflowProbe'").fetch_one(&c.owner).await.unwrap();
    let boom_title = uniq("boom");
    let cstart = request_event("IncidentChecklistRequested", company, json!({ "company_id": company, "title": boom_title, "incident_id": "-" }), "wf-boom");
    tasks.deliver(cstart.clone()).await.unwrap();
    let cinst = instance_of(c, "IncidentChecklist", cstart.id).await.unwrap();
    let probes: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.outbox_events WHERE event_type = 'WorkflowProbe'").fetch_one(&c.owner).await.unwrap();
    let followups = |title: String| async move { sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conformance.incidents WHERE title = $1 AND metadata->>'deleted_at' IS NULL").bind(title).fetch_one(&c.owner).await.unwrap() };
    r.check("F", "Workflow engine", "a step that fails after writing leaves none of its writes: the row it staged is gone, the earlier step's follow-up stayed, and the failure is logged", probes == before_probe && cinst.status == "running" && cinst.attempts == 1 && cinst.step.as_deref() == Some("risky") && followups(boom_title.clone()).await == 1 && steps_of(c, cinst.id).await.contains(&("risky".to_string(), "failed".to_string())), "start the checklist for a boom... title", "no WorkflowProbe row; running on risky, attempt 1; the follow-up incident exists", &format!("probe rows {before_probe}->{probes}; {} {:?} attempts {}", cinst.status, cinst.step, cinst.attempts), WF);
    r.check("F", "Workflow engine", "a transient failure stores its backoff and waits: nothing is retried before it passes", cinst.due && engine.sweep().await.unwrap().retried.is_empty(), "sweep right after the failure", "nothing due", "see the instance", WF);
    force_due(c, cinst.id).await;
    let retry1 = engine.sweep().await.unwrap();
    let second = instance_of(c, "IncidentChecklist", cstart.id).await.unwrap();
    force_due(c, cinst.id).await;
    let retry2 = engine.sweep().await.unwrap();
    let ended = instance_of(c, "IncidentChecklist", cstart.id).await.unwrap();
    let cs = steps_of(c, cinst.id).await;
    r.check("F", "Workflow engine", "a failing step is tried three times (backoff observable between them), then the instance fails and compensates: compensated", retry1.retried == vec![cinst.id] && second.attempts == 2 && retry2.retried == vec![cinst.id] && ended.status == "compensated" && cs.iter().filter(|s| s.0 == "risky" && s.1 == "failed").count() == 3, "force the backoff due twice", "three failed attempts, then compensated", &format!("attempts {} -> {}; {} {cs:?}", cinst.attempts, second.attempts, ended.status), WF);
    r.check("F", "Workflow engine", "compensation runs through the generated service: the follow-up is soft-deleted and its IncidentDeleted event staged", followups(boom_title.clone()).await == 0 && cs.iter().any(|s| s == &("compensate:retire_followup".to_string(), "compensated".to_string())) && {
        let follow_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM conformance.incidents WHERE title = $1").bind(&boom_title).fetch_optional(&c.owner).await.unwrap();
        match follow_id { Some(id) => kinds(&staged(c, &id.to_string()).await).contains(&"IncidentDeleted"), None => false }
    }, "SELECT the follow-up and its events", "metadata.deleted_at set; IncidentDeleted staged; the log shows compensate:retire_followup", &format!("{cs:?}"), WF);
    engine.compensate(company, cinst.id).await.unwrap();
    let cs_again = steps_of(c, cinst.id).await;
    r.check("F", "Workflow engine", "compensation is idempotent: running it again repeats nothing and changes nothing", cs_again == cs && instance_of(c, "IncidentChecklist", cstart.id).await.unwrap().status == "compensated", "engine.compensate again", "the step log is unchanged", &format!("{} -> {} rows", cs.len(), cs_again.len()), WF);

    // A refusal the caller can fix is not retried.
    let refuse_title = uniq("refuse");
    let rstart = request_event("IncidentChecklistRequested", company, json!({ "company_id": company, "title": refuse_title, "incident_id": "-" }), "wf-refuse");
    tasks.deliver(rstart.clone()).await.unwrap();
    let rinst = instance_of(c, "IncidentChecklist", rstart.id).await.unwrap();
    let rsteps = steps_of(c, rinst.id).await;
    r.check("F", "Workflow engine", "a business failure (the verb refuses with a validation error) is not retried: one failed attempt, then compensation", rinst.status == "compensated" && rsteps.iter().filter(|s| s.1 == "failed").count() == 1, "start the checklist for a refuse... title", "compensated after one failed attempt", &format!("{} {rsteps:?}", rinst.status), WF);

    // 7. A step cut short leaves the instance on it; the sweep resumes without repeating a completed step.
    let linger_title = uniq("linger");
    let lstart = request_event("IncidentChecklistRequested", company, json!({ "company_id": company, "title": linger_title, "incident_id": "-" }), "wf-linger");
    {
        let run = tasks.deliver(lstart.clone());
        tokio::select! {
            _ = run => {}
            _ = backbone_module_skeleton::application::workflows::lingering().notified() => {}
        }
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    let linst = instance_of(c, "IncidentChecklist", lstart.id).await.unwrap();
    let lprobe: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.outbox_events WHERE event_type = 'WorkflowProbe' AND aggregate_id = $1").bind(&linger_title).fetch_one(&c.owner).await.unwrap();
    r.check("F", "Workflow engine", "a run cut inside a step's transaction (a crash) leaves the instance running on that step, with the step's row rolled back and the completed step logged once", linst.status == "running" && linst.step.as_deref() == Some("risky") && lprobe == 0 && steps_of(c, linst.id).await.iter().map(|s| s.0.as_str()).collect::<Vec<_>>() == ["open_followup"], "cut the run while the linger step is inside its transaction", "running on risky; no WorkflowProbe row; one logged step", &format!("{} {:?} probe rows {lprobe}", linst.status, linst.step), WF);
    sqlx::query("UPDATE conformance.bb_workflow_instances SET context = jsonb_set(context, '{title}', to_jsonb($2::text)), updated_at = now() - interval '1 hour' WHERE id = $1")
        .bind(linst.id)
        .bind(&linger_title.replacen("linger", "fine", 1))
        .execute(&c.owner)
        .await
        .unwrap();
    let recovered = engine.sweep().await.unwrap();
    let lend = instance_of(c, "IncidentChecklist", lstart.id).await.unwrap();
    r.check("F", "Workflow engine", "the sweep drives an instance nobody is running and the completed step is not repeated (one follow-up incident, never two)", recovered.recovered.contains(&linst.id) && lend.status == "completed" && followups(linger_title.clone()).await == 1, "age the instance and sweep", "completed; one follow-up", &format!("{:?} {}; follow-ups {}", recovered.recovered, lend.status, followups(linger_title.clone()).await), WF);

    // 8. Tenancy: instances are fenced by company.
    let b_event = request_event("IncidentAcknowledged", c.org.company_b, json!({ "incident_id": high }), "wf-b");
    let b_report = engine.on_event(&b_event).await.unwrap();
    let hstill = instance_of(c, "IncidentEscalation", hstart.id).await.unwrap();
    r.check("F", "Workflow engine", "an event of company B for company A's key never resumes company A's instance", b_report.resumed.is_empty() && hstill.status == "waiting", "deliver IncidentAcknowledged under company B with A's incident id", "A's instance still waiting", &format!("{:?} {}", b_report.resumed, hstill.status), WF);
    let seen_by_b = engine.describe(c.org.company_b, hinst.id).await.unwrap();
    let seen_by_a = engine.describe(company, hinst.id).await.unwrap();
    r.check("F", "Workflow engine", "another company's scope cannot read the instance or its step log; its own company can", seen_by_b.is_none() && seen_by_a.as_ref().is_some_and(|(i, s)| i.status == "waiting" && !s.is_empty()), "describe the instance under company B and under company A", "none; found", &format!("{:?} / {:?}", seen_by_b.as_ref().map(|(i, _)| i.status.clone()), seen_by_a.as_ref().map(|(i, _)| i.status.clone())), WF);
    let unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.bb_workflow_instances").fetch_one(&c.pool).await.unwrap();
    let steps_unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.bb_workflow_steps").fetch_one(&c.pool).await.unwrap();
    r.check("F", "Workflow engine", "the fence is the database: the app role with no company scope sees no instance and no step", unscoped == 0 && steps_unscoped == 0, "SELECT count(*) as the app role, unscoped", "0, 0", &format!("{unscoped}, {steps_unscoped}"), WF);
    force_due(c, hinst.id).await;
    let a_sweep = engine.sweep().await.unwrap();
    r.check("F", "Workflow engine", "the sweep still serves every company: company A's due wait runs under company A's scope", a_sweep.timed_out.contains(&hinst.id) && instance_of(c, "IncidentEscalation", hstart.id).await.unwrap().status == "completed", "force A's wait due and sweep", "timed out and completed", &format!("{:?}", a_sweep.timed_out), WF);
    let owned: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.bb_workflow_instances WHERE company_id <> $1 AND workflow = 'IncidentEscalation'").bind(company).fetch_one(&c.owner).await.unwrap();
    r.check("F", "Workflow engine", "every instance carries its event's company", owned == 0, "SELECT instances of another company", "0", &owned.to_string(), WF);

    // 9. What the module hands the composing service.
    let events = tasks.subscribed_events();
    r.check("F", "Workflow engine", "the module's background tasks list the events the engine reacts to: each trigger and each awaited event", ["IncidentEscalationRequested", "IncidentAcknowledged", "IncidentChecklistRequested"].iter().all(|e| events.iter().any(|x| x == e)) && events.len() == 3, "background_tasks().subscribed_events()", "the two triggers and the awaited event", &format!("{events:?}"), WF);
    let unrelated = tasks.deliver(OutboxRecord::new("SomethingElse", "X", "x", company, json!({}), chrono::Utc::now())).await;
    r.check("F", "Workflow engine", "deliver accepts an event nobody subscribed to and does nothing (the relay's sink can be shared)", unrelated.is_ok(), "deliver SomethingElse", "ok", &format!("{unrelated:?}"), WF);
}

// =============================================================================
// G. Scoping
// =============================================================================

async fn area_g(c: &Ctx) {
    let (r, app, ta, tb) = (&c.rec, &c.app, c.ta.as_str(), c.tb.as_str());
    const RLS: &str = "RLS policy (generators/sql.rs:1170/1238) + org_auth scope (backbone-auth/src/org.rs)";
    let code = uniq("G");
    let a = app.post("/projects", ta, json!({ "companyId": c.org.company_a, "code": code, "name": "Company A secret project", "budget": 777 })).await;
    let aid = a.id();
    let at = c.ticket(ta, c.org.branch_a, &aid, json!({ "title": "Branch ticket of A" })).await;
    r.check("G", "Setup", "company A creates a project and a branch ticket", a.status == 201 && at.status == 201, &at.request, "201, 201", &format!("{} / {}", a.got(), at.got()), RLS);
    let atid = at.id();
    let b = c.project(tb, c.org.company_b, json!({})).await;
    let bid = b.id();

    let l = app.get("/projects?limit=100", tb).await;
    r.check("G", "Read fence", "list: B sees none of A's projects", l.status == 200 && !l.ids().contains(&aid) && l.ids().contains(&bid), &l.request, "B's rows only", &l.got(), RLS);
    let g = app.get(&format!("/projects/{aid}"), tb).await;
    r.check("G", "Read fence", "get: B gets 404 for A's project", g.status == 404, &g.request, "404", &g.got(), RLS);
    let gt = app.get(&format!("/tickets/{atid}"), tb).await;
    r.check("G", "Read fence", "get: B gets 404 for A's org-fenced ticket", gt.status == 404, &gt.request, "404", &gt.got(), RLS);
    let ga = app.get(&format!("/tickets/{atid}"), ta).await;
    r.check("G", "Read fence", "the company's session sees its branch's ticket (subtree union)", ga.status == 200, &ga.request, "200", &ga.got(), RLS);
    let p = app.patch(&format!("/projects/{aid}"), tb, json!({ "name": "hijacked" })).await;
    r.check("G", "Write fence", "update: B's PATCH of A's project answers 404", p.status == 404, &p.request, "404", &p.got(), RLS);
    let pu = app.put(&format!("/projects/{aid}"), tb, json!({ "companyId": c.org.company_b, "code": code, "name": "hijacked", "budget": 0, "priority": 3, "billable": true, "kind": "internal" })).await;
    r.check("G", "Write fence", "update: B's PUT of A's project answers 404", pu.status == 404, &pu.request, "404", &pu.got(), RLS);
    let d = app.delete(&format!("/projects/{aid}"), tb).await;
    r.check("G", "Write fence", "delete: B's DELETE of A's project answers 404", d.status == 404, &d.request, "404", &d.got(), RLS);
    let bp = app.patch("/projects/bulk", tb, json!({ "ids": [aid], "patch": { "name": "hijacked" } })).await;
    let bd = app.post("/projects/delete/bulk", tb, json!({ "ids": [aid] })).await;
    let still = app.get(&format!("/projects/{aid}"), ta).await;
    r.check("G", "Write fence", "bulk: B's bulk patch and bulk delete of A's ids change nothing", still.status == 200 && still.data()["name"] == json!("Company A secret project") && bd.data()["soft_deleted"].as_i64().unwrap_or(0) == 0, &format!("{} ; {}", bp.request, bd.request), "A's row unchanged, soft_deleted 0", &format!("{} / {} / A sees {}", bp.got(), bd.got(), still.got()), RLS);
    let bh = app.delete_body("/projects/trash/bulk", tb, json!({ "ids": [aid] })).await;
    let still = app.get(&format!("/projects/{aid}"), ta).await;
    r.check("G", "Write fence", "bulk: B's bulk permanent delete of A's id removes nothing", still.status == 200, &bh.request, "A's row still there", &bh.got(), RLS);

    let bt = c.ticket(tb, c.org.company_b, &aid, json!({ "title": "B ticket on A project" })).await;
    r.check("G", "Write fence", "B cannot attach its ticket to a project it cannot see (foreign keys check rows RLS hides)", bt.status != 201, &bt.request, "refused (422 reference_missing)", &bt.got(), "backbone-core write path (PersistentEntity::references, emitted by generators/rust.rs from generators/sql.rs relation_foreign_key): a written key must name a row the writer can see; Postgres's own key check reads rows RLS hides");

    // The read fence over include=, proven on a cross-company link the API now refuses: seeded as
    // the owner, whom RLS does not apply to, so B holds a ticket naming A's project.
    let seeded = Uuid::new_v4();
    let seed = sqlx::query("INSERT INTO conformance.tickets (id, org_unit_id, project_id, title) VALUES ($1, $2, $3, 'B ticket seeded on A project')")
        .bind(seeded)
        .bind(c.org.company_b)
        .bind(Uuid::parse_str(&aid).unwrap_or_default())
        .execute(&c.owner)
        .await;
    const INC: &str = "backbone-core/src/http.rs (include runs through the fenced connection)";
    let leaks = |resp: &Resp| resp.raw.contains("Company A secret project") || resp.raw.contains(&code);
    let inc = app.get(&format!("/tickets?id={seeded}&include=project"), tb).await;
    let sees_own = inc.items().iter().any(|i| i["id"] == json!(seeded.to_string()));
    r.check("G", "Read fence", "include=: B cannot read A's project through a relation (list)", seed.is_ok() && inc.status == 200 && sees_own && !leaks(&inc), &inc.request, "B's seeded ticket listed, A's project fields absent", &format!("seed {:?}; {}", seed.as_ref().err(), inc.got()), INC);
    let one = app.get(&format!("/tickets/{seeded}?include=project"), tb).await;
    r.check("G", "Read fence", "include=: B cannot read A's project through a relation (get by id)", seed.is_ok() && one.status == 200 && !leaks(&one), &one.request, "200, A's project fields absent", &one.got(), INC);
    let own = app.get(&format!("/tickets?id={atid}&include=project"), ta).await;
    let hydrated = own.items().first().map(|i| i["project"]["id"] == json!(aid)).unwrap_or(false);
    r.check("G", "Read fence", "include=: A's own ticket still hydrates A's project", own.status == 200 && hydrated, &own.request, "items[0].project.id == A's project", &own.got(), INC);
    let ag = app.get(&format!("/projects/aggregate?code={code}&group_by=kind&sum=budget"), tb).await;
    r.check("G", "Read fence", "aggregate: B's aggregate sees none of A's rows", ag.status == 200 && !ag.raw.contains("777"), &ag.request, "no group with A's budget", &ag.got(), RLS);
    let cn = app.get(&format!("/projects/count?code={code}"), tb).await;
    r.check("G", "Read fence", "count: B's count of A's code is 0", cn.status == 200 && cn.data()["count"] == json!(0), &cn.request, "count 0", &cn.got(), RLS);
    let or = app.get(&format!("/projects?code=nothing&code[or]={code}"), tb).await;
    r.check("G", "Read fence", "an OR-filter attempt (?code[or]=…) cannot widen past the fence", or.status == 200 && or.items().is_empty(), &or.request, "empty", &or.got(), RLS);
    let base = app.get(&format!("/projects?code={code}&__base_condition=1%3D1"), tb).await;
    r.check("G", "Read fence", "?__base_condition= has no effect", base.status == 200 && base.items().is_empty(), &base.request, "empty", &base.got(), "backbone-orm/src/filter/parser.rs:81 (is_internal_key)");
    let x = c.project(ta, c.org.company_a, json!({})).await.id();
    app.delete(&format!("/projects/{x}"), ta).await;
    let base_a = app.get(&format!("/projects?id={x}&__base_condition=1%3D1"), ta).await;
    r.check("G", "Read fence", "?__base_condition= cannot surface soft-deleted rows", base_a.status == 200 && base_a.items().is_empty(), &base_a.request, "empty", &base_a.got(), "backbone-orm/src/filter/parser.rs:81");
    let tr = app.get(&format!("/projects/trash?id={x}"), tb).await;
    r.check("G", "Read fence", "trash: B's trash list does not show A's deleted rows", tr.status == 200 && tr.items().is_empty(), &tr.request, "empty", &tr.got(), RLS);
    let ra = app.post("/projects/restore/all", tb, json!({})).await;
    let still_trashed = app.get(&format!("/projects/{x}"), ta).await.status == 404;
    r.check("G", "Write fence", "restore-all: B cannot restore A's trash", still_trashed, &ra.request, "A's row stays trashed", &ra.got(), RLS);
    let et = app.delete("/projects/empty", tb).await;
    let still_there = app.get(&format!("/projects/{x}/deleted"), ta).await.status == 200;
    r.check("G", "Write fence", "empty-trash: B cannot purge A's trash", still_there, &et.request, "A's trashed row still there", &et.got(), RLS);

    let forged = app.post("/projects", ta, c.project_body(c.org.company_b, &uniq("F"), "Forged company")).await;
    let landed: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.projects WHERE name = 'Forged company'").fetch_one(&c.owner).await.unwrap_or(-1);
    r.check("G", "Write fence", "a create naming another company is refused (RLS WITH CHECK) and writes nothing", (400..500).contains(&forged.status) && landed == 0, &forged.request, "4xx, no row", &format!("{}; rows {landed}", forged.got()), "Postgres raises 42501 for a WITH CHECK breach; backbone-core/src/persistence/traits.rs:108-150 has no mapping for it, so it surfaces as a 500 DatabaseError");
    let ft = c.ticket(ta, c.org.company_b, &aid, json!({ "title": "Forged unit" })).await;
    r.check("G", "Write fence", "a ticket naming another company's org unit is refused and writes nothing", (400..500).contains(&ft.status), &ft.request, "4xx", &ft.got(), "42501 unmapped in backbone-core/src/persistence/traits.rs:108-150");
    let root = c.ticket(ta, c.org.root, &aid, json!({ "title": "Root unit" })).await;
    r.check("G", "Write fence", "the org kind guard refuses a ticket on the root node with a 4xx naming org_unit_id", (400..500).contains(&root.status) && root.violations().iter().any(|v| v["path"] == json!("org_unit_id") && v["code"] == json!("org_kind_refused")), &root.request, "422 org_kind_refused on org_unit_id", &root.got(), "the kind guard (generators/sql.rs org_kind_guard_function_sql) raises check_violation with COLUMN org_unit_id and CONSTRAINT tg_<table>__org_kind_refused; backbone-core/src/persistence/traits.rs maps it");

    // tokens
    let other_tenant = OrgIssuer::hs256(SECRET).for_audience("globex").issue_access("admin-a", c.org.company_a, &[], Some(c.org.company_a), Duration::from_secs(3600)).unwrap();
    let ot = app.get("/projects", &other_tenant).await;
    r.check("G", "Tokens", "a token minted for another tenant (aud) is refused 401", ot.status == 401, &ot.request, "401", &ot.got(), "backbone-auth/src/org.rs (audience binding)");
    let unbound = OrgIssuer::hs256(SECRET).issue_access("admin-a", c.org.company_a, &[], Some(c.org.company_a), Duration::from_secs(3600)).unwrap();
    let ub = app.get("/projects", &unbound).await;
    r.check("G", "Tokens", "a token naming no tenant is refused 401 on a tenant's surface", ub.status == 401, &ub.request, "401", &ub.got(), "backbone-auth/src/audience.rs");
    let none = app.send("GET", "/projects", None, None, &[]).await;
    r.check("G", "Tokens", "no token is refused 401", none.status == 401, &none.request, "401", &none.got(), "backbone-auth/src/org.rs");
    let expired = jsonwebtoken::encode(&jsonwebtoken::Header::default(), &OrgClaims { sub: "admin-a".into(), exp: 1_000_000_000, org_unit_id: Some(c.org.company_a), entitled_units: vec![], company_id: Some(c.org.company_a), typ: Some("access".into()), aud: vec![TENANT.into()] }, &jsonwebtoken::EncodingKey::from_secret(SECRET)).unwrap();
    let ex = app.get("/projects", &expired).await;
    r.check("G", "Tokens", "an expired token is refused 401", ex.status == 401, &ex.request, "401", &ex.got(), "backbone-auth/src/org.rs");
    let refresh = c.issuer.issue_refresh("admin-a", c.org.company_a, &[], Some(c.org.company_a), Duration::from_secs(3600)).unwrap();
    let rf = app.get("/projects", &refresh).await;
    r.check("G", "Tokens", "a refresh token is refused 401 on a guarded route", rf.status == 401, &rf.request, "401", &rf.got(), "backbone-auth/src/org.rs (typ)");
    let stranger = c.issuer.issue_access("admin-x", Uuid::new_v4(), &[], None, Duration::from_secs(3600)).unwrap();
    let st = app.get("/projects", &stranger).await;
    r.check("G", "Tokens", "a token whose org unit is not in this tenant's tree is refused 403", st.status == 403, &st.request, "403", &st.got(), "backbone-auth/src/org.rs (UnknownActingUnit)");
    let unknown_host = app.router.clone().oneshot(Request::builder().uri("/projects").header("host", "nobody.conf.test").header("authorization", format!("Bearer {ta}")).body(Body::empty()).unwrap()).await.map(|r| r.status().as_u16()).unwrap_or(0);
    r.check("G", "Tokens", "a host that names an unknown tenant is refused before auth (503/404)", unknown_host == 503 || unknown_host == 404, "GET /projects Host: nobody.conf.test", "404 or 503", &unknown_host.to_string(), "backbone-tenant/src/axum_router.rs");

    // RLS for the app role, outside any request
    let unscoped_p: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.projects").fetch_one(&c.pool).await.unwrap_or(-1);
    let unscoped_t: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.tickets").fetch_one(&c.pool).await.unwrap_or(-1);
    let owner_p: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.projects").fetch_one(&c.owner).await.unwrap_or(-1);
    r.check("G", "RLS", "the app role without a scope reads zero rows from fenced tables (fail-closed)", unscoped_p == 0 && unscoped_t == 0 && owner_p > 0, "SELECT count(*) FROM projects / tickets as the app role, no app.* settings", "0 and 0 (the owner sees > 0)", &format!("projects {unscoped_p}, tickets {unscoped_t}, owner sees {owner_p}"), RLS);
    let mut tx = c.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.company_id', $1, true)").bind(c.org.company_b.to_string()).execute(&mut *tx).await.unwrap();
    let as_b: i64 = sqlx::query_scalar("SELECT count(*) FROM conformance.projects WHERE company_id = $1").bind(c.org.company_a).fetch_one(&mut *tx).await.unwrap_or(-1);
    let _ = tx.rollback().await;
    r.check("G", "RLS", "the app role scoped to B reads none of A's rows even when asking for them", as_b == 0, "set app.company_id = B; SELECT ... WHERE company_id = A", "0", &as_b.to_string(), RLS);
    let forced: Vec<(String, bool, bool)> = sqlx::query_as("SELECT relname::text, relrowsecurity, relforcerowsecurity FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'conformance' AND relname IN ('projects','tickets','approvals') ORDER BY 1").fetch_all(&c.owner).await.unwrap_or_default();
    r.check("G", "RLS", "every scoped table has RLS enabled and forced", forced.len() == 3 && forced.iter().all(|(_, e, f)| *e && *f), "pg_class relrowsecurity, relforcerowsecurity", "3 tables, all true", &format!("{forced:?}"), "generators/sql.rs:1170/1238");
}

// =============================================================================
// I. Permissions
// =============================================================================

/// The identity tables a composing service has from the identity module, in the columns the
/// framework's grant source and the generated seed read: a scratch copy for the harness.
const IDENTITY_DDL: &[&str] = &[
    "CREATE SCHEMA IF NOT EXISTS sapiens",
    "CREATE TABLE public.roles (id uuid PRIMARY KEY DEFAULT gen_random_uuid(), name text NOT NULL, description text, is_default boolean NOT NULL DEFAULT false, metadata jsonb NOT NULL DEFAULT '{}')",
    "CREATE TABLE public.permissions (id uuid PRIMARY KEY DEFAULT gen_random_uuid(), name text NOT NULL, description text, resource text NOT NULL, action text NOT NULL, metadata jsonb NOT NULL DEFAULT '{}')",
    "CREATE TABLE public.role_permissions (id uuid PRIMARY KEY DEFAULT gen_random_uuid(), role_id uuid NOT NULL REFERENCES public.roles(id), permission_id uuid NOT NULL REFERENCES public.permissions(id), granted_at timestamptz NOT NULL DEFAULT now(), metadata jsonb NOT NULL DEFAULT '{}')",
    "CREATE UNIQUE INDEX idx_role_permissions_role_id_permission_id ON public.role_permissions (role_id, permission_id)",
    "CREATE TABLE public.user_roles (user_id uuid NOT NULL, role_id uuid NOT NULL, metadata jsonb NOT NULL DEFAULT '{}')",
    "CREATE TABLE public.user_permissions (user_id uuid NOT NULL, permission_id uuid NOT NULL, status text NOT NULL DEFAULT 'active', revoked_at timestamptz, expires_at timestamptz, resource_id uuid, metadata jsonb NOT NULL DEFAULT '{}')",
    "CREATE TABLE sapiens.organization_users (user_id uuid NOT NULL, org_unit_id uuid NOT NULL, role_id uuid, status text NOT NULL DEFAULT 'active', metadata jsonb NOT NULL DEFAULT '{}')",
    "CREATE TABLE sapiens.direct_permission_grants (user_id uuid NOT NULL, permission_id uuid NOT NULL, status text NOT NULL DEFAULT 'active', revoked_at timestamptz, expires_at timestamptz, scope jsonb, conditions jsonb, metadata jsonb NOT NULL DEFAULT '{}')",
];

async fn permission_row(owner: &PgPool, name: &str) -> Uuid {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM public.permissions WHERE name = $1 AND (metadata->>'deleted_at') IS NULL").bind(name).fetch_optional(owner).await.unwrap() {
        return id;
    }
    let (resource, action) = name.split_once(|c| c == ':' || c == '.').unwrap_or((name, "*"));
    sqlx::query_scalar("INSERT INTO public.permissions (name, resource, action) VALUES ($1, $2, $3) RETURNING id")
        .bind(name)
        .bind(resource)
        .bind(action)
        .fetch_one(owner)
        .await
        .unwrap()
}

/// A tenant role created as rows: the role, and a grant row for each permission.
async fn tenant_role(owner: &PgPool, name: &str, permissions: &[&str]) -> Uuid {
    let role: Uuid = sqlx::query_scalar("INSERT INTO public.roles (name) VALUES ($1) RETURNING id").bind(name).fetch_one(owner).await.unwrap();
    for p in permissions {
        let pid = permission_row(owner, p).await;
        sqlx::query("INSERT INTO public.role_permissions (role_id, permission_id) VALUES ($1, $2)").bind(role).bind(pid).execute(owner).await.unwrap();
    }
    role
}

async fn assign_role(owner: &PgPool, user: Uuid, role: Uuid) {
    sqlx::query("INSERT INTO public.user_roles (user_id, role_id) VALUES ($1, $2)").bind(user).bind(role).execute(owner).await.unwrap();
}

async fn assign_named_role(owner: &PgPool, user: Uuid, role: &str) {
    let id: Uuid = sqlx::query_scalar("SELECT id FROM public.roles WHERE name = $1 AND (metadata->>'deleted_at') IS NULL").bind(role).fetch_one(owner).await.unwrap();
    assign_role(owner, user, id).await;
}

async fn direct_grant(owner: &PgPool, user: Uuid, permission: &str) {
    let pid = permission_row(owner, permission).await;
    sqlx::query("INSERT INTO public.user_permissions (user_id, permission_id) VALUES ($1, $2)").bind(user).bind(pid).execute(owner).await.unwrap();
}

/// A 403 naming the permission the route requires, in the framework's shape.
fn denied(resp: &Resp, permission: &str) -> bool {
    resp.status == 403
        && resp.body["success"] == json!(false)
        && resp.violations().iter().any(|v| v["code"] == json!("permission_required") && v["params"]["permission"] == json!(permission))
}

async fn area_i(c: &Ctx) {
    let (r, app, rapp) = (&c.rec, &c.app, &c.rapp);
    const P: &str = "backbone-core/src/permission.rs (the route layer), backbone-auth/src/grants.rs (matching), the generated <entity>_permissions() in presentation/http/*_handler.rs";
    let ta = c.ta.as_str();
    let hour = Duration::from_secs(3600);
    let token = |user: Uuid| c.issuer.issue_access(&user.to_string(), c.org.company_a, &[], Some(c.org.company_a), hour).unwrap();

    // Records to act on, made by the admin of the default stack.
    let pid = c.project(ta, c.org.company_a, json!({})).await.id();
    let tid = c.ticket(ta, c.org.company_a, &pid, json!({})).await.id();
    let gone = c.ticket(ta, c.org.company_a, &pid, json!({})).await.id();
    app.delete(&format!("/tickets/{gone}"), ta).await;

    // ---- seeds -------------------------------------------------------------------------
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM public.permissions WHERE (metadata->>'deleted_at') IS NULL ORDER BY name").fetch_all(&c.owner).await.unwrap();
    let want = [
        "ticket:read", "ticket:create", "ticket:update", "ticket:delete", "ticket:restore", "ticket:purge",
        "project:read", "approval:read", "ticket:start", "ticket:resolve", "approval:submit", "ticket:escalate", "ticket:archive_with_reason", "*:*",
    ];
    let missing: Vec<&str> = want.iter().copied().filter(|w| !names.iter().any(|n| n == w)).collect();
    r.check("I", "Seeds", "the seed migration writes every permission the routes require, the declared custom verbs and the system roles' grants", missing.is_empty(), "SELECT name FROM public.permissions", "ticket:{read,create,update,delete,restore,purge}, project:read, approval:read, the verbs, ticket:escalate, *:*", &format!("missing {missing:?} of {} rows", names.len()), "generators/sql.rs (the seed migration), rbac.rs (seed)");
    let viewer_grants: Vec<String> = sqlx::query_scalar(
        "SELECT p.name FROM public.role_permissions rp JOIN public.roles r ON r.id = rp.role_id JOIN public.permissions p ON p.id = rp.permission_id WHERE r.name = 'viewer' AND (rp.metadata->>'deleted_at') IS NULL ORDER BY p.name",
    ).fetch_all(&c.owner).await.unwrap();
    r.check("I", "Seeds", "a declared system role is seeded with its baseline grants (the dotted spelling normalised: projects.read -> project:read)", viewer_grants == vec!["project:read", "ticket:read"], "SELECT the grants of role viewer", "[project:read, ticket:read]", &format!("{viewer_grants:?}"), "rbac.rs (system_roles, canonical)");
    let admin_god: i64 = sqlx::query_scalar("SELECT count(*) FROM public.role_permissions rp JOIN public.roles r ON r.id = rp.role_id JOIN public.permissions p ON p.id = rp.permission_id WHERE r.name = 'ADMIN' AND p.name = '*:*'").fetch_one(&c.owner).await.unwrap();
    r.check("I", "Seeds", "the ADMIN system role is declared with *:* in the seed", admin_god == 1, "SELECT the *:* grant of role ADMIN", "1", &admin_god.to_string(), "the module index's authorization: roles:");

    // ---- the identity of the people the probes use -------------------------------------
    let (viewer, nobody, starred, god, reader_all, legacy, dotted, boss) =
        (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    assign_named_role(&c.owner, viewer, "viewer").await;
    assign_named_role(&c.owner, boss, "ADMIN").await;
    direct_grant(&c.owner, starred, "ticket:*").await;
    direct_grant(&c.owner, god, "*:*").await;
    direct_grant(&c.owner, reader_all, "*:read").await;
    direct_grant(&c.owner, legacy, "*:transition:*").await;
    direct_grant(&c.owner, dotted, "tickets.read").await;
    let (tv, tn, ts, tg, tr_, tl, td, tb_) = (token(viewer), token(nobody), token(starred), token(god), token(reader_all), token(legacy), token(dotted), token(boss));

    // ---- generic routes: each verb needs its permission --------------------------------
    // The viewer holds ticket:read and project:read, nothing else.
    for (what, path) in [("list", "/tickets".to_string()), ("get", format!("/tickets/{tid}")), ("count", "/tickets/count".to_string()), ("trash", "/tickets/trash".to_string()), ("trash count", "/tickets/trash/count".to_string())] {
        let g = rapp.get(&path, &tv).await;
        r.check("I", "Generic routes", &format!("a holder of ticket:read passes the permission layer on {what}"), g.status == 200, &g.request, "200", &g.got(), P);
    }
    let agg = rapp.get("/tickets/aggregate?group_by=status&agg=count", &tv).await;
    r.check("I", "Generic routes", "a holder of ticket:read passes the permission layer on aggregate", agg.status != 403 && agg.status != 401, &agg.request, "not 401/403", &agg.got(), P);
    let hist = rapp.get(&format!("/tickets/{tid}/history"), &tv).await;
    r.check("I", "Generic routes", "a holder of ticket:read passes the permission layer on history", hist.status != 403 && hist.status != 401, &hist.request, "not 401/403", &hist.got(), P);
    for (what, path) in [("list", "/tickets".to_string()), ("get", format!("/tickets/{tid}")), ("count", "/tickets/count".to_string()), ("aggregate", "/tickets/aggregate?group_by=status&agg=count".to_string()), ("history", format!("/tickets/{tid}/history")), ("trash", "/tickets/trash".to_string()), ("trash count", "/tickets/trash/count".to_string()), ("deleted get", format!("/tickets/{gone}/deleted"))] {
        let g = rapp.get(&path, &tn).await;
        r.check("I", "Generic routes", &format!("a caller without ticket:read is refused on {what}: 403 permission_required naming ticket:read"), denied(&g, "ticket:read"), &g.request, "403, params.permission ticket:read", &g.got(), P);
    }
    let body = json!({ "orgUnitId": c.org.company_a, "projectId": pid, "title": "Denied" });
    let writes: Vec<(&str, &str, String, Option<Value>, &str)> = vec![
        ("create", "POST", "/tickets".into(), Some(body.clone()), "ticket:create"),
        ("bulk create", "POST", "/tickets/bulk".into(), Some(json!([body.clone()])), "ticket:create"),
        ("upsert", "POST", "/tickets/upsert".into(), Some(body.clone()), "ticket:create"),
        ("put", "PUT", format!("/tickets/{tid}"), Some(body.clone()), "ticket:update"),
        ("patch", "PATCH", format!("/tickets/{tid}"), Some(json!({ "title": "x" })), "ticket:update"),
        ("bulk put", "PUT", "/tickets/bulk".into(), Some(json!([{ "id": tid }])), "ticket:update"),
        ("bulk patch", "PATCH", "/tickets/bulk".into(), Some(json!({ "ids": [tid], "changes": { "title": "x" } })), "ticket:update"),
        ("bulk preview", "POST", "/tickets/bulk/preview".into(), Some(json!({ "ids": [tid], "changes": { "title": "x" } })), "ticket:update"),
        ("delete", "DELETE", format!("/tickets/{tid}"), None, "ticket:delete"),
        ("bulk delete", "POST", "/tickets/delete/bulk".into(), Some(json!({ "ids": [tid] })), "ticket:delete"),
        ("restore", "POST", format!("/tickets/{gone}/restore"), None, "ticket:restore"),
        ("bulk restore", "POST", "/tickets/restore/bulk".into(), Some(json!({ "ids": [gone] })), "ticket:restore"),
        ("restore all", "POST", "/tickets/restore/all".into(), None, "ticket:restore"),
        ("purge", "DELETE", format!("/tickets/trash/{gone}"), None, "ticket:purge"),
        ("bulk purge", "DELETE", "/tickets/trash/bulk".into(), Some(json!({ "ids": [gone] })), "ticket:purge"),
        ("empty trash", "DELETE", "/tickets/empty".into(), None, "ticket:purge"),
    ];
    for (what, method, path, payload, perm) in &writes {
        let w = rapp.send(method, path, Some(&tv), payload.as_ref().map(|b| b.to_string()), &[]).await;
        r.check("I", "Generic routes", &format!("{what} needs {perm}: a holder of read only is refused 403 naming it"), denied(&w, perm), &w.request, &format!("403, params.permission {perm}"), &w.got(), P);
    }
    let still = app.get(&format!("/tickets/{tid}"), ta).await;
    r.check("I", "Generic routes", "no refused write changed anything", still.status == 200 && still.data()["title"] != json!("Denied") && still.data()["title"] != json!("x"), &still.request, "the record is untouched", &still.got(), P);

    // The refusal comes before the body is read and before the record is looked up.
    let unknown = Uuid::new_v4();
    let blind = rapp.patch(&format!("/tickets/{unknown}"), &tv, json!({ "title": "x" })).await;
    r.check("I", "Generic routes", "a caller without ticket:update learns nothing about an unknown id (403, not 404)", denied(&blind, "ticket:update"), &blind.request, "403", &blind.got(), P);
    let bad_body = rapp.send("POST", "/tickets", Some(&tv), Some("{ not json".into()), &[]).await;
    r.check("I", "Generic routes", "a malformed body from a caller without ticket:create is still 403, not 400", denied(&bad_body, "ticket:create"), &bad_body.request, "403", &bad_body.got(), P);

    // Another entity needs its own permission: ticket permissions do not carry over.
    let prj = rapp.get("/projects", &tv).await;
    r.check("I", "Generic routes", "project:read (a seeded grant of the viewer role) opens the project list", prj.status == 200, &prj.request, "200", &prj.got(), P);
    let apr = rapp.get("/approvals", &tv).await;
    r.check("I", "Generic routes", "an entity the caller holds nothing for is refused: approval:read", denied(&apr, "approval:read"), &apr.request, "403 approval:read", &apr.got(), P);
    let apr_w = rapp.post("/approvals", &tv, json!({})).await;
    r.check("I", "Generic routes", "a read-only entity mounts no write route (405), whatever the caller holds", apr_w.status == 405, &apr_w.request, "405", &apr_w.got(), P);

    // ---- transitions and custom verbs --------------------------------------------------
    let tr = |verb: &str| format!("/tickets/{tid}/transitions/{verb}");
    let t = rapp.post(&tr("start"), &tv, json!({})).await;
    r.check("I", "Transitions", "a transition needs <model>:<verb>: 403 naming ticket:start", denied(&t, "ticket:start"), &t.request, "403, params.permission ticket:start", &t.got(), P);
    let blind = rapp.post(&format!("/tickets/{unknown}/transitions/start"), &tv, json!({})).await;
    r.check("I", "Transitions", "the permission is checked before the record is read: an unknown id is 403 for a caller without it", denied(&blind, "ticket:start"), &blind.request, "403", &blind.got(), P);
    // The viewer never reached the record: the state is unchanged.
    let st = app.get(&format!("/tickets/{tid}"), ta).await;
    r.check("I", "Transitions", "a refused transition changed nothing", st.data()["status"] == json!("open"), &st.request, "status open", &st.got(), P);
    let e = rapp.post(&format!("/tickets/{tid}/escalate"), &tv, json!({ "note": "x" })).await;
    r.check("I", "Custom verbs", "a hand-written verb guarded by guarded(): 403 naming ticket:escalate", denied(&e, "ticket:escalate"), &e.request, "403, params.permission ticket:escalate", &e.got(), P);
    let a = rapp.post(&format!("/tickets/{tid}/archive-with-reason"), &tv, json!({ "reason": "x" })).await;
    r.check("I", "Custom verbs", "a second guarded verb names its own permission: ticket:archive_with_reason", denied(&a, "ticket:archive_with_reason"), &a.request, "403", &a.got(), P);
    let hp = rapp.get("/hooked-projects", &tn).await;
    r.check("I", "Custom verbs", "the extension's generic routes are guarded too: project:read", denied(&hp, "project:read"), &hp.request, "403 project:read", &hp.got(), P);

    // ---- wildcards, authority by name ---------------------------------------------------
    let fresh = || json!({ "orgUnitId": c.org.company_a, "projectId": pid, "title": format!("Permitted {}", uniq("")) });
    let w = rapp.post("/tickets", &ts, fresh()).await;
    r.check("I", "Wildcards", "ticket:* grants every action of the entity (create)", w.status == 201, &w.request, "201", &w.got(), P);
    let ws = rapp.post(&format!("/tickets/{}/transitions/start", w.id()), &ts, json!({})).await;
    r.check("I", "Wildcards", "ticket:* grants the entity's transitions", ws.status == 200, &ws.request, "200", &ws.got(), P);
    let wp = rapp.get("/projects", &ts).await;
    r.check("I", "Wildcards", "ticket:* does not reach another entity", denied(&wp, "project:read"), &wp.request, "403 project:read", &wp.got(), P);
    let ra = rapp.get("/projects", &tr_).await;
    let rc = rapp.post("/tickets", &tr_, body.clone()).await;
    r.check("I", "Wildcards", "*:read grants every read and no write", ra.status == 200 && denied(&rc, "ticket:create"), &rc.request, "200 on read, 403 ticket:create on write", &format!("{} / {}", ra.got(), rc.got()), P);
    let gd = rapp.post("/tickets", &tg, fresh()).await;
    let gp = rapp.delete(&format!("/tickets/trash/{gone}"), &tg).await;
    r.check("I", "Wildcards", "*:* grants everything (create and purge)", gd.status == 201 && gp.status != 403, &gp.request, "201 and not 403", &format!("{} / {}", gd.got(), gp.got()), P);
    let bd = rapp.post("/tickets", &tb_, fresh()).await;
    r.check("I", "Wildcards", "the ADMIN role grants everything by name, with no permission row of its own for the person", bd.status == 201, &bd.request, "201", &bd.got(), P);

    // ---- legacy spellings --------------------------------------------------------------
    let lv = rapp.post(&format!("/tickets/{}/transitions/start", c.ticket(ta, c.org.company_a, &pid, json!({})).await.id()), &tl, json!({})).await;
    let lr = rapp.get("/tickets", &tl).await;
    let lw = rapp.patch(&format!("/tickets/{tid}"), &tl, json!({ "title": "x" })).await;
    r.check("I", "Legacy spellings", "*:transition:* still grants a verb, and opens no generic read or write", lv.status == 200 && denied(&lr, "ticket:read") && denied(&lw, "ticket:update"), &lr.request, "200 on the verb, 403 on read and update", &format!("{} / {} / {}", lv.got(), lr.got(), lw.got()), "backbone-auth/src/grants.rs (read_held: a transition name covers verbs only)");
    let dr = rapp.get("/tickets", &td).await;
    r.check("I", "Legacy spellings", "the dotted spelling tickets.read still grants the read (normalised)", dr.status == 200, &dr.request, "200", &dr.got(), "backbone-auth/src/grants.rs");
    let seen = backbone_auth::grants::legacy_spellings_seen();
    r.check("I", "Legacy spellings", "a legacy spelling that grants access raises the deprecation signal with the canonical form to rewrite to", seen.iter().any(|(k, _)| k == "tickets.read") && seen.iter().any(|(k, v)| k == "*:transition:*" && v == "*:<each verb>"), "legacy_spellings_seen()", "tickets.read and *:transition:* listed", &format!("{seen:?}"), "backbone-auth/src/grants.rs (note_legacy)");

    // ---- a tenant role is data -----------------------------------------------------------
    let triager = Uuid::new_v4();
    let tt = token(triager);
    let before = rapp.patch(&format!("/tickets/{tid}"), &tt, json!({ "title": "Triaged" })).await;
    r.check("I", "Tenant roles as data", "before the role exists the person is refused", denied(&before, "ticket:update"), &before.request, "403", &before.got(), P);
    let role = tenant_role(&c.owner, "triager", &["ticket:read", "ticket:update"]).await;
    assign_role(&c.owner, triager, role).await;
    let after = rapp.patch(&format!("/tickets/{tid}"), &tt, json!({ "title": "Triaged" })).await;
    r.check("I", "Tenant roles as data", "a role created as rows grants access at the next request, with no regeneration and no restart", after.status == 200 && after.data()["title"] == json!("Triaged"), &after.request, "200", &after.got(), "backbone-auth/src/resolver.rs (IdentityGrantSource)");
    let del = rapp.delete(&format!("/tickets/{tid}"), &tt).await;
    r.check("I", "Tenant roles as data", "the role grants only what its rows say: ticket:delete is still refused", denied(&del, "ticket:delete"), &del.request, "403 ticket:delete", &del.got(), P);
    let up = rapp.post("/tickets/upsert", &tt, body.clone()).await;
    r.check("I", "Tenant roles as data", "an upsert needs create and update: a holder of update only is refused on create", denied(&up, "ticket:create"), &up.request, "403 ticket:create", &up.got(), P);
    sqlx::query("UPDATE public.user_roles SET metadata = '{\"deleted_at\":\"2026-01-01\"}' WHERE user_id = $1").bind(triager).execute(&c.owner).await.unwrap();
    let revoked = rapp.patch(&format!("/tickets/{tid}"), &tt, json!({ "title": "Again" })).await;
    r.check("I", "Tenant roles as data", "removing the assignment removes the access at the next request", denied(&revoked, "ticket:update"), &revoked.request, "403", &revoked.got(), P);

    // ---- the resolver is the source: the token's claims are not -----------------------
    let admin_in_resolver_mode = rapp.get("/tickets", ta).await;
    r.check("I", "Resolver mode", "with a resolver installed the token is not the source: a subject the identity tables do not know holds nothing", denied(&admin_in_resolver_mode, "ticket:read"), &admin_in_resolver_mode.request, "403", &admin_in_resolver_mode.got(), "backbone-auth/src/resolver.rs");
    sqlx::query("ALTER TABLE public.user_permissions RENAME TO user_permissions_away").execute(&c.owner).await.unwrap();
    let broken = rapp.get("/tickets", &tv).await;
    sqlx::query("ALTER TABLE public.user_permissions_away RENAME TO user_permissions").execute(&c.owner).await.unwrap();
    r.check("I", "Resolver mode", "a grant store that cannot be read refuses the request (503 grants_unavailable), never reads as no grants and carries on", broken.status == 503 && broken.has_code("grants_unavailable") && !broken.raw.contains("user_permissions"), &broken.request, "503 grants_unavailable, no table name", &broken.got(), "backbone-auth/src/resolver.rs");

    // ---- fail closed with no resolver installed -------------------------------------------
    // The default stack (the host's own AuthContext, no resolver): `admin-*` holds *:*, `clerk-*`
    // holds nothing, `reader-*` holds ticket:read.
    let reader = c.issuer.issue_access("reader-a", c.org.company_a, &[], Some(c.org.company_a), hour).unwrap();
    for (what, method, path, payload, perm) in [
        ("list", "GET", "/tickets".to_string(), None, "ticket:read"),
        ("create", "POST", "/tickets".to_string(), Some(body.clone()), "ticket:create"),
        ("patch", "PATCH", format!("/tickets/{tid}"), Some(json!({ "title": "x" })), "ticket:update"),
        ("delete", "DELETE", format!("/tickets/{tid}"), None, "ticket:delete"),
        ("a transition", "POST", format!("/tickets/{tid}/transitions/start"), Some(json!({})), "ticket:start"),
        ("a custom verb", "POST", format!("/tickets/{tid}/escalate"), Some(json!({ "note": "x" })), "ticket:escalate"),
        ("the extension's generic list", "GET", "/hooked-projects".to_string(), None, "project:read"),
    ] {
        let d = app.send(method, &path, Some(&c.clerk), payload.as_ref().map(|b| b.to_string()), &[]).await;
        r.check("I", "Fail closed", &format!("no resolver and no permission on the AuthContext: {what} is 403 naming {perm}"), denied(&d, perm), &d.request, &format!("403, params.permission {perm}"), &d.got(), P);
    }
    // Every other generic verb, in the default stack too: no resolver, nothing held, refused.
    for (what, path) in [
        ("get", format!("/tickets/{tid}")),
        ("count", "/tickets/count".to_string()),
        ("aggregate", "/tickets/aggregate?group_by=status&agg=count".to_string()),
        ("history", format!("/tickets/{tid}/history")),
        ("trash", "/tickets/trash".to_string()),
        ("trash count", "/tickets/trash/count".to_string()),
        ("deleted get", format!("/tickets/{gone}/deleted")),
    ] {
        let d = app.get(&path, &c.clerk).await;
        r.check("I", "Fail closed", &format!("no resolver and no permission on the AuthContext: {what} is 403 naming ticket:read"), denied(&d, "ticket:read"), &d.request, "403, params.permission ticket:read", &d.got(), P);
    }
    for (what, method, path, payload, perm) in &writes {
        let d = app.send(method, path, Some(&c.clerk), payload.as_ref().map(|b| b.to_string()), &[]).await;
        r.check("I", "Fail closed", &format!("no resolver and no permission on the AuthContext: {what} is 403 naming {perm}"), denied(&d, perm), &d.request, &format!("403, params.permission {perm}"), &d.got(), P);
    }
    let rd = app.get("/tickets", &reader).await;
    let rw = app.post("/tickets", &reader, body.clone()).await;
    r.check("I", "Fail closed", "no resolver: the permissions on the AuthContext are what counts (ticket:read reads, and only reads)", rd.status == 200 && denied(&rw, "ticket:create"), &rw.request, "200 on read, 403 ticket:create on write", &format!("{} / {}", rd.got(), rw.got()), P);
    let bare = c.bare.get("/tickets", ta).await;
    r.check("I", "Fail closed", "no AuthContext on the request at all is 401 authentication_required, never allowed", bare.status == 401 && bare.has_code("authentication_required"), &bare.request, "401 authentication_required", &bare.got(), P);
    let bare_tr = c.bare.post(&format!("/tickets/{tid}/transitions/start"), ta, json!({})).await;
    r.check("I", "Fail closed", "a transition with no AuthContext is 401 too", bare_tr.status == 401, &bare_tr.request, "401", &bare_tr.got(), P);
    let waived = c.waived.get("/tickets", &c.clerk).await;
    let waived_tr = c.waived.post(&format!("/tickets/{}/transitions/start", c.ticket(ta, c.org.company_a, &pid, json!({})).await.id()), &c.clerk, json!({})).await;
    r.check("I", "Fail closed", "a request an outer gate admitted on a narrower rule (PermissionWaiver) passes the layer, and only that request", waived.status == 200 && waived_tr.status == 200, &waived.request, "200 on a read and a transition", &format!("{} / {}", waived.got(), waived_tr.got()), "backbone-auth/src/grants.rs (PermissionWaiver)");

    // ---- seeds are idempotent and never undo a tenant's edits -----------------------------
    let seed_files: Vec<std::path::PathBuf> = std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains("_seed_conformance_permissions_") && n.ends_with(".up.sql")))
        .collect();
    r.check("I", "Seeds", "the generator wrote one seed migration, named by a digest of its content", seed_files.len() == 1, "ls migrations/*_seed_conformance_permissions_*.up.sql", "1 file", &format!("{seed_files:?}"), "generators/sql.rs");
    let counts = |owner: PgPool| async move {
        let p: i64 = sqlx::query_scalar("SELECT count(*) FROM public.permissions").fetch_one(&owner).await.unwrap();
        let ro: i64 = sqlx::query_scalar("SELECT count(*) FROM public.roles WHERE name IN ('ADMIN','viewer')").fetch_one(&owner).await.unwrap();
        let g: i64 = sqlx::query_scalar("SELECT count(*) FROM public.role_permissions rp JOIN public.roles r ON r.id = rp.role_id WHERE r.name IN ('ADMIN','viewer')").fetch_one(&owner).await.unwrap();
        (p, ro, g)
    };
    let before = counts(c.owner.clone()).await;
    let sql = std::fs::read_to_string(&seed_files[0]).unwrap();
    let rerun = sqlx::raw_sql(&sql).execute(&c.owner).await;
    let after = counts(c.owner.clone()).await;
    r.check("I", "Seeds", "re-running the seed migration changes nothing", rerun.is_ok() && before == after, "run the seed migration again as the owner", &format!("{before:?}"), &format!("{after:?} {:?}", rerun.err().map(|e| e.to_string())), "generators/sql.rs (every statement inserts what is missing)");
    // A tenant edits the baseline: revokes ticket:read from viewer, grants ticket:create instead.
    let viewer_role: Uuid = sqlx::query_scalar("SELECT id FROM public.roles WHERE name = 'viewer'").fetch_one(&c.owner).await.unwrap();
    sqlx::query("UPDATE public.role_permissions SET metadata = '{\"deleted_at\":\"2026-01-01\"}' WHERE role_id = $1 AND permission_id = (SELECT id FROM public.permissions WHERE name = 'ticket:read')").bind(viewer_role).execute(&c.owner).await.unwrap();
    let create_id = permission_row(&c.owner, "ticket:create").await;
    sqlx::query("INSERT INTO public.role_permissions (role_id, permission_id) VALUES ($1, $2)").bind(viewer_role).bind(create_id).execute(&c.owner).await.unwrap();
    let rerun = sqlx::raw_sql(&sql).execute(&c.owner).await;
    let live: Vec<String> = sqlx::query_scalar("SELECT p.name FROM public.role_permissions rp JOIN public.permissions p ON p.id = rp.permission_id WHERE rp.role_id = $1 AND (rp.metadata->>'deleted_at') IS NULL ORDER BY p.name").bind(viewer_role).fetch_all(&c.owner).await.unwrap();
    let revoked_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM public.role_permissions rp JOIN public.permissions p ON p.id = rp.permission_id WHERE rp.role_id = $1 AND p.name = 'ticket:read'").bind(viewer_role).fetch_one(&c.owner).await.unwrap();
    r.check("I", "Seeds", "a tenant-edited role grant survives a re-run: the revoked baseline grant stays revoked (no duplicate row) and the added grant stays", rerun.is_ok() && live == vec!["project:read", "ticket:create"] && revoked_rows == 1, "revoke viewer's ticket:read, add ticket:create, re-run the seed", "live grants [project:read, ticket:create]; one (revoked) ticket:read row", &format!("{live:?}; {revoked_rows} ticket:read row(s); {:?}", rerun.err().map(|e| e.to_string())), "generators/sql.rs (NOT EXISTS over every row, soft-deleted included)");
    let tenant_view = rapp.get("/tickets", &tv).await;
    r.check("I", "Seeds", "and the edit is what the next request sees: the viewer lost the read and gained the create", denied(&tenant_view, "ticket:read"), &tenant_view.request, "403 ticket:read", &tenant_view.got(), "backbone-auth/src/resolver.rs");
}

// =============================================================================
// I. Public routes (`public:`)
// =============================================================================

async fn area_i_public(c: &Ctx) {
    let r = &c.rec;
    let (app, open, public) = (&c.app, &c.open, &c.public_only);
    const P: &str = "backbone-core/src/permission.rs (CrudPermissions::public) and src/http.rs (gate), the generated label_permissions() and public_routes()";
    let ta = c.ta.as_str();
    let code = uniq("L");
    let made = app.post("/labels", ta, json!({ "code": code, "name": "Public label" })).await;
    let lid = made.id();
    r.check("I", "Public routes", "setup: an admin creates a label (label:create is still required to write it)", made.status == 201, &made.request, "201", &made.got(), P);

    // The read routes of a `public: true` model answer with no login at all.
    let list = open.send("GET", "/labels", None, None, &[]).await;
    r.check("I", "Public routes", "a public read with no authentication answers 200: the list", list.status == 200 && list.ids().contains(&lid), &list.request, "200 listing the label", &list.got(), P);
    let one = open.send("GET", &format!("/labels/{lid}"), None, None, &[]).await;
    r.check("I", "Public routes", "a public read with no authentication answers 200: one record", one.status == 200 && one.id() == lid, &one.request, "200 with the record", &one.got(), P);
    let count = open.send("GET", "/labels/count", None, None, &[]).await;
    r.check("I", "Public routes", "a public read with no authentication answers 200: count", count.status == 200, &count.request, "200", &count.got(), P);

    // A caller who is logged in but holds nothing reads it too, where an ordinary entity refuses.
    let clerk_labels = app.get("/labels", &c.clerk).await;
    let clerk_tickets = app.get("/tickets", &c.clerk).await;
    r.check("I", "Public routes", "a logged-in caller who holds no permission reads the public model, and is refused a non-public one (ticket:read)", clerk_labels.status == 200 && denied(&clerk_tickets, "ticket:read"), &clerk_labels.request, "200 on /labels, 403 ticket:read on /tickets", &format!("{} / {}", clerk_labels.got(), clerk_tickets.got()), P);

    // The same model's writes are not public.
    let body = json!({ "code": uniq("W"), "name": "Anon" });
    for (what, method, path, payload) in [
        ("create", "POST", "/labels".to_string(), Some(body.clone())),
        ("patch", "PATCH", format!("/labels/{lid}"), Some(json!({ "name": "x" }))),
        ("put", "PUT", format!("/labels/{lid}"), Some(body.clone())),
        ("delete", "DELETE", format!("/labels/{lid}"), None),
        ("bulk create", "POST", "/labels/bulk".to_string(), Some(json!([body.clone()]))),
        ("restore", "POST", format!("/labels/{lid}/restore"), None),
        ("empty trash", "DELETE", "/labels/empty".to_string(), None),
    ] {
        let d = open.send(method, &path, None, payload.as_ref().map(|b| b.to_string()), &[]).await;
        r.check("I", "Public routes", &format!("the same model's {what} is not public: 401 with no authentication"), d.status == 401 && d.has_code("authentication_required"), &d.request, "401 authentication_required", &d.got(), P);
    }
    for (what, method, path, payload, perm) in [
        ("create", "POST", "/labels".to_string(), Some(body.clone()), "label:create"),
        ("patch", "PATCH", format!("/labels/{lid}"), Some(json!({ "name": "x" })), "label:update"),
        ("delete", "DELETE", format!("/labels/{lid}"), None, "label:delete"),
    ] {
        let d = app.send(method, &path, Some(&c.clerk), payload.as_ref().map(|b| b.to_string()), &[]).await;
        r.check("I", "Public routes", &format!("the same model's {what} still needs {perm}: 403 for a caller who holds nothing"), denied(&d, perm), &d.request, &format!("403 naming {perm}"), &d.got(), P);
    }
    let still = open.send("GET", &format!("/labels/{lid}"), None, None, &[]).await;
    r.check("I", "Public routes", "a refused anonymous write changed nothing", still.status == 200 && still.data()["name"] == json!("Public label"), &still.request, "the label unchanged", &still.got(), P);

    // A non-public sibling stays closed, in the same mount.
    for (what, path) in [("project list", "/projects".to_string()), ("ticket list", "/tickets".to_string()), ("approval list", "/approvals".to_string())] {
        let d = open.send("GET", &path, None, None, &[]).await;
        r.check("I", "Public routes", &format!("a non-public sibling is 401 beside it in the same mount: {what}"), d.status == 401 && d.has_code("authentication_required"), &d.request, "401 authentication_required", &d.got(), P);
    }

    // `public_routes()` mounts only the public model's routes.
    let pl = public.send("GET", "/labels", None, None, &[]).await;
    let pp = public.send("GET", "/projects", None, None, &[]).await;
    let pw = public.send("POST", "/labels", None, Some(body.to_string()), &[]).await;
    r.check("I", "Public routes", "`public_routes()` serves the public model's reads with no login, and mounts no other entity", pl.status == 200 && pp.status == 404, &pl.request, "200 on /labels, 404 on /projects", &format!("{} / {}", pl.got(), pp.got()), "src/generators/module.rs (public_routes)");
    r.check("I", "Public routes", "`public_routes()` still refuses an anonymous write to the public model: 401", pw.status == 401, &pw.request, "401", &pw.got(), "src/generators/module.rs (public_routes)");

    // The tenant resolution in front of the layer still applies: an unknown tenant is not served.
    let req = Request::builder().method("GET").uri("/labels").header("host", "nobody.conf.test").body(Body::empty()).unwrap();
    let res = public.router.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let res_body_is_empty_of_labels = !String::from_utf8_lossy(&res.into_body().collect().await.map(|b| b.to_bytes()).unwrap_or_default()).contains(&lid);
    r.check("I", "Public routes", "the tenant fence still applies to a public route: an unknown tenant is refused, not served", status != 200 && res_body_is_empty_of_labels, "GET /labels (host nobody.conf.test)", "not served (the tenant router refuses an unknown tenant)", &status.to_string(), "backbone-tenant (tenant_route)");

    // The vocabulary: a public action is not a permission anything must hold.
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM public.permissions WHERE (metadata->>'deleted_at') IS NULL AND name LIKE 'label:%' ORDER BY name").fetch_all(&c.owner).await.unwrap();
    r.check("I", "Public routes", "the seed holds the permissions the public model's writes require and none for its public reads", names.contains(&"label:create".to_string()) && !names.contains(&"label:read".to_string()), "SELECT name FROM public.permissions WHERE name LIKE 'label:%'", "label:create present, label:read absent", &format!("{names:?}"), "src/rbac.rs (required, seed)");
}

// =============================================================================
// H. Debuggability
// =============================================================================

async fn area_h(c: &Ctx) {
    let (r, app, ta) = (&c.rec, &c.app, c.ta.as_str());
    let refusals = app.refusals.lock().unwrap().clone();

    let with_violations: Vec<&Resp> = refusals.iter().filter(|x| !x.violations().is_empty()).collect();
    let unnamed: Vec<String> = with_violations
        .iter()
        .flat_map(|x| {
            x.violations()
                .into_iter()
                .filter(|v| v["code"].as_str().unwrap_or("").is_empty() || v.get("path").is_none())
                .map(|v| format!("{} → {v}", x.request))
                .collect::<Vec<_>>()
        })
        .collect();
    r.check("H", "Refusals", "every violation the probes received names a code and carries a path key", !with_violations.is_empty() && unnamed.is_empty(), &format!("{} refusals with violations", with_violations.len()), "every violation has code and path", &unnamed.join(" | "), "the refusing layer");
    let validation_like: Vec<&Resp> = refusals.iter().filter(|x| matches!(x.status, 400 | 409 | 422)).collect();
    let coded = |x: &Resp| !x.violations().is_empty() || x.body.get("code").is_some();
    let uncoded: Vec<String> = validation_like.iter().filter(|x| !coded(x)).map(|x| format!("{} → {}", x.request, trunc(&x.raw, 160))).collect();
    r.check("H", "Refusals", "every 400/409/422 refusal carries a stable code (a violation code)", uncoded.is_empty(), &format!("{} refusals", validation_like.len()), "every one coded", &uncoded.join(" | "), "generated transition handlers (generators/handler.rs: ApiResponse::error without a code); backbone-core ServiceError::Validation (400, no violations); RepositoryError::AlreadyExists (409, no violations)");
    let leaks: Vec<String> = refusals
        .iter()
        .filter(|x| ["ck_", "uq_", "idx_", "tg_", "fk_", "duplicate key", "violates"].iter().any(|n| x.raw.contains(n)))
        .map(|x| format!("{} → {}", x.request, trunc(&x.raw, 200)))
        .collect();
    r.check("H", "Refusals", "no refusal leaks a raw constraint, index or trigger name", leaks.is_empty(), &format!("{} refusals scanned", refusals.len()), "no ck_/uq_/idx_/tg_/fk_/duplicate key in a body", &leaks.join(" | "), "backbone-core/src/persistence/traits.rs:125 (AlreadyExists carries the raw pg message)");
    let nf = app.get(&format!("/projects/{}", Uuid::new_v4()), ta).await;
    r.check("H", "Refusals", "a 404 names a stable code", nf.body.get("code").is_some() || !nf.violations().is_empty() || nf.body["error"].as_str().is_some_and(|e| e.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')), &nf.request, "a code such as not_found", &nf.got(), "backbone-core/src/service.rs:1173 (not found is a sentence, no code)");
    r.check("H", "Refusals", "a 404 does not leak the Rust type path", !nf.raw.contains("::"), &nf.request, "no `crate::path::Type` in the body", &nf.got(), "backbone-core/src/service.rs:1173-1174 formats std::any::type_name::<E>()");

    let corr = app.send("GET", "/projects", Some(ta), None, &[("x-correlation-id", "conf-trace-123")]).await;
    r.check("H", "Correlation", "a caller's X-Correlation-ID is echoed on the response", corr.headers.get("x-correlation-id").and_then(|v| v.to_str().ok()) == Some("conf-trace-123"), &corr.request, "x-correlation-id: conf-trace-123", &format!("{:?}", corr.headers.get("x-correlation-id")), "backbone-auth/src/org.rs:588");
    let minted = app.get("/projects/not-a-uuid", ta).await;
    let header = minted.headers.get("x-correlation-id").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    r.check("H", "Correlation", "a refusal carries a minted correlation id header when the caller sent none", Uuid::parse_str(&header).is_ok(), &minted.request, "x-correlation-id: <uuid>", &header, "backbone-auth/src/org.rs");
}
