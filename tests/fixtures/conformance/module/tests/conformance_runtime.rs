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

/// Publishes the `AuthContext` the generated transition handlers read, from the proven
/// `OrgContext` (the composing service's job; see serpa-service's auth_context middleware).
async fn auth_bridge(mut req: Request, next: Next) -> Response {
    if let Some(org) = req.extensions().get::<OrgContext>().cloned() {
        let permissions = if org.user_id.starts_with("admin") { vec!["*".to_string()] } else { Vec::new() };
        req.extensions_mut().insert(backbone_auth::middleware::AuthContext {
            user_id: org.user_id,
            roles: Vec::new(),
            permissions,
        });
    }
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

    // The app role, as scripts/rls_app_role.sql grants it.
    for stmt in [
        format!("GRANT CONNECT ON DATABASE {db} TO {role}"),
        format!("GRANT USAGE ON SCHEMA conformance, organization TO {role}"),
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA conformance TO {role}"),
        format!("GRANT SELECT ON ALL TABLES IN SCHEMA organization TO {role}"),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA conformance TO {role}"),
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
    let tenant_router = TenantRouter::new(HostResolver::new("conf.test"), TenantRegistry::new(Factory { pool: pool.clone() }, 4));
    let verifier = OrgVerifier::hs256(SECRET).bind_audience(tenant_audience);
    let router = module
        .all_crud_routes()
        .merge(module.transition_routes())
        .merge(module.extension_routes())
        .route_layer(from_fn(auth_bridge))
        .route_layer(from_fn_with_state(verifier, org_auth))
        .layer(from_fn_with_state(tenant_router, tenant_route));
    let app = App { router, refusals: Arc::new(Mutex::new(Vec::new())) };

    let issuer = OrgIssuer::hs256(SECRET).for_audience(TENANT);
    let hour = Duration::from_secs(3600);
    let ta = issuer.issue_access("admin-a", org.company_a, &[], Some(org.company_a), hour).unwrap();
    let tb = issuer.issue_access("admin-b", org.company_b, &[], Some(org.company_b), hour).unwrap();
    let clerk = issuer.issue_access("clerk-a", org.company_a, &[], Some(org.company_a), hour).unwrap();

    let ctx = Ctx { app, rec: rec.clone(), org, ta, tb, clerk, owner: owner.clone(), pool: pool.clone(), module: module.clone(), issuer };

    // Each area runs isolated: a panic in one is recorded and the next still runs.
    for (name, fut) in [
        ("A", AssertUnwindSafe(area_a(&ctx)).boxed()),
        ("B", AssertUnwindSafe(area_b(&ctx)).boxed()),
        ("C", AssertUnwindSafe(area_c(&ctx)).boxed()),
        ("D", AssertUnwindSafe(area_d(&ctx)).boxed()),
        ("E", AssertUnwindSafe(area_e(&ctx)).boxed()),
        ("F", AssertUnwindSafe(area_f(&ctx)).boxed()),
        ("G", AssertUnwindSafe(area_g(&ctx)).boxed()),
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
    r.check("B", "DB constraints", "a duplicate @unique value answers 422 naming contact_email, without the raw index name", u1.status == 201 && u2.status == 422 && u2.violations().iter().any(|v| v["path"] == json!("contact_email")) && !raw_leak, &u2.request, "422, violation path contact_email, no index name in the body", &u2.got(), "metaphor-plugin-schema/src/generators/sql.rs:1063 names the @unique index idx_<table>_<field>; backbone-core/src/persistence/traits.rs:125/194 maps only uq_* names, so any other 23505 becomes AlreadyExists(raw pg message) → 409 with the constraint name");
    let title = format!("Same title {}", uniq(""));
    let c1 = c.ticket(ta, c.org.company_a, &parent, json!({ "title": title })).await;
    let c2 = c.ticket(ta, c.org.company_a, &parent, json!({ "title": title })).await;
    let raw_leak = c2.raw.contains("idx_tickets") || c2.raw.contains("duplicate key");
    r.check("B", "DB constraints", "a duplicate composite unique (project_id, title) answers 422 with a field path, without the raw index name", c1.status == 201 && c2.status == 422 && !c2.violations().is_empty() && !raw_leak, &c2.request, "422 with a violation, no index name in the body", &c2.got(), "metaphor-plugin-schema/src/generators/sql.rs:1501 names `indexes: unique` idx_<table>_<cols>; backbone-core/src/persistence/traits.rs:125/194 maps only uq_* names → 409 AlreadyExists with the raw message");

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
                r.check("B", "DB constraints", what, ok, sql, &format!("Violation {{path: {path}, code: {code}}}, no ck_ name in the message"), &got, "backbone-core/src/persistence/traits.rs:250 check_column reads the field out of `ck_<table>_<field>_<code>__<code>`; a hook rule's constraint is named after the rule (`ck_<table>_<rule>__<code>`, generators/sql.rs:2955), so its path comes back empty while the guard reports the rule's first field");
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
    r.check("C", "Declared uniqueness", "declared uniqueness rule refuses a duplicate code in the company: 422 code_taken on path code", first.status == 201 && dup.status == 422 && dup.violation("code", "code_taken"), &dup.request, "422 {path: code, code: code_taken}", &dup.got(), "backbone-core/src/persistence/traits.rs:194 takes the path from the DETAIL `Key (cols)=`, which Postgres omits for a table with RLS enabled (BuildIndexValueDescription), so a declared uniqueness rule on a fenced table answers path \"\"; the rule's own name carries no field to fall back on");
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

    c.rec.unsupported("D", "Declared events", "`events:` in a hook file produce publishing code", "nothing is generated: commands/schema/module_loader.rs:315-331 drops them; schema doctor only warns about a declared event no hand code names (commands/schema/events_check.rs); the generic service publishes CrudEvents through the publisher the module attaches, proven above");
    c.rec.unsupported("D", "Outbox", "an event staged in the write's transaction (outbox)", "the generic service publishes fire-and-forget after the write, outside its transaction (backbone-core/src/service.rs, `let _ = self.event_publisher.publish(..)`); staging is hand code with backbone_outbox::outbox::stage, which the generator never emits");
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
    r.check("E", "Transitions", "a disallowed transition names a stable code", bad.body["violations"].as_array().is_some_and(|v| !v.is_empty()) || bad.body.get("code").is_some(), &bad.request, "a violation or code (e.g. transition_not_allowed)", &bad.got(), "metaphor-plugin-schema/src/generators/handler.rs (the 400 'Transition not allowed from current state' is ApiResponse::error with no code)");
    let g = app.post(&tr("resolve"), ta, json!({})).await;
    r.check("E", "Transition guard", "a transition whose guard does not hold (amount <= 1000) is refused 422", g.status == 422, &g.request, "422", &g.got(), H);
    r.check("E", "Transition guard", "a refused guard names a stable code", !g.violations().is_empty(), &g.request, "a violation with a code", &g.got(), "metaphor-plugin-schema/src/generators/handler.rs (guard refusal is ApiResponse::error with no code)");
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
    if bt.status == 201 {
        let inc = app.get(&format!("/tickets?id={}&include=project", bt.id()), tb).await;
        let leaked = inc.raw.contains("Company A secret project");
        r.check("G", "Read fence", "include=: B cannot read A's project through a relation", inc.status == 200 && !leaked, &inc.request, "project null, A's name absent", &inc.got(), "backbone-core/src/http.rs:631-675 (include runs through the fenced connection)");
        r.check("G", "Write fence", "B cannot attach its ticket to a project it cannot see (foreign keys check rows RLS hides)", false, &bt.request, "refused (422 reference_missing)", &bt.got(), "Postgres referential checks bypass RLS, and nothing in the generated guard checks the referenced row is visible (rules/fields.rs; generators/sql.rs add_declared_foreign_keys): a scoped writer can probe for and reference another company's ids");
    } else {
        r.check("G", "Write fence", "B cannot attach its ticket to a project it cannot see (foreign keys check rows RLS hides)", true, &bt.request, "refused", &bt.got(), "");
        r.unsupported("G", "Read fence", "include=: B cannot read A's project through a relation", "not reachable: B could not create a ticket referencing A's project");
    }
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
    r.check("G", "Write fence", "the org kind guard refuses a ticket on the root node with a 4xx naming org_unit_id", (400..500).contains(&root.status) && root.violations().iter().any(|v| v["path"] == json!("org_unit_id")), &root.request, "422 on org_unit_id", &root.got(), "the kind guard (generators/sql.rs:1238, enable_org_fence) raises a plain RAISE EXCEPTION (P0001) with no ERRCODE/CONSTRAINT, which backbone-core/src/persistence/traits.rs:150 cannot map → 500");

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
