//! `metaphor schema tenancy` — emit or verify the composition-installed tenancy
//! decorator (ADR-0029).
//!
//! Modules ship no scoping columns. The composing backend-service owns a
//! `tenancy.yaml` descriptor at its root listing the module tables it wants
//! org-scoped, and this command turns that descriptor into a decorator migration
//! chain (ADD COLUMN `org_unit_id` with guarded backfill, the entitlement-union
//! RLS policy, the write-path kind guard, per-unit uniques, and a deny-by-default
//! event trigger over the scoped schemas). The chain is service-owned: it is
//! written under the service's `migrations/` and must be listed under
//! `user_owned:` in `metaphor.codegen.yaml` like any hand-authored migration.
//!
//! Modes: default emits the migration pair; `--preview` prints it; `--check`
//! verifies a live database against the descriptor (coverage doctor).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;
use serde::Deserialize;

use crate::generators::sql::{
    tenancy_decorator_chain, TenancyTableTarget, TenancyUnique,
};

use super::discovery::find_schema_files;
use super::module_loader::build_module_schema;

/// The service-level tenancy descriptor (`tenancy.yaml`, ADR-0029).
#[derive(Debug, Deserialize)]
pub(crate) struct TenancyDescriptor {
    version: u32,
    /// Schemas under which any later-created table is deny-locked until the
    /// descriptor covers it. Every listed table's schema should appear here.
    #[serde(default)]
    scoped_schemas: Vec<String>,
    #[serde(default)]
    tables: Vec<TenancyTableEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TenancyTableEntry {
    /// Workspace project name (`backbone-party`) — resolved against
    /// `metaphor.yaml` to find the module's schema and its Postgres schema.
    module: String,
    /// Bare table name as the module's model collection (`parties`).
    table: String,
    /// Kind-guard allowance for root-node anchoring (tenant-shared rows).
    #[serde(default)]
    allow_root: bool,
    #[serde(default)]
    uniques: Vec<TenancyUniqueEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TenancyUniqueEntry {
    fields: Vec<String>,
    /// Raw SQL predicate for a partial unique, carried verbatim.
    #[serde(default)]
    r#where: Option<String>,
}

pub(super) fn execute_tenancy(
    output: &Path,
    preview: bool,
    check: bool,
    database_url: Option<String>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let descriptor_path = cwd.join("tenancy.yaml");
    anyhow::ensure!(
        descriptor_path.exists(),
        "no tenancy.yaml in {} — the tenancy descriptor lives at the composing \
         service's root (ADR-0029)",
        cwd.display()
    );
    let descriptor = load_descriptor(&descriptor_path)?;

    let targets = resolve_targets(&cwd, &descriptor)?;

    if check {
        #[cfg(feature = "database")]
        {
            let url = database_url.context(
                "--check needs a database URL (--database-url or DATABASE_URL)",
            )?;
            let rt = tokio::runtime::Runtime::new()
                .context("failed to create tokio runtime for --check")?;
            return rt.block_on(check_coverage(&descriptor, &targets, &url));
        }
        #[cfg(not(feature = "database"))]
        {
            let _ = database_url;
            anyhow::bail!(
                "--check requires live introspection; this binary was built \
                 without the `database` feature"
            );
        }
    }

    let (up, down) = tenancy_decorator_chain(&targets, &descriptor.scoped_schemas);

    if preview {
        println!("{up}");
        println!("-- ══ down ══");
        println!("{down}");
        return Ok(());
    }

    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let stem = if descriptor.scoped_schemas.is_empty() {
        "tenancy".to_string()
    } else {
        // Path components cap at 255 bytes on the common filesystems, and the
        // schema enumeration grows with every module a service composes, so an
        // unbounded join eventually fails to write. Cap the enumeration and
        // pin the full list with a short stable digest (FNV-1a over the exact
        // joined string): the same descriptor always emits the same stem, and
        // two descriptors that differ only past the cap still get different
        // names instead of colliding on a shared truncation.
        let joined = descriptor.scoped_schemas.join("_");
        const CAP: usize = 160;
        if joined.len() <= CAP {
            format!("tenancy_{joined}")
        } else {
            let digest = joined
                .bytes()
                .fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
            let mut cut = CAP;
            while !joined.is_char_boundary(cut) {
                cut -= 1;
            }
            format!("tenancy_{}_{digest:08x}", &joined[..cut])
        }
    };
    fs::create_dir_all(output)?;
    let up_path = output.join(format!("{ts}_{stem}.up.sql"));
    let down_path = output.join(format!("{ts}_{stem}.down.sql"));
    fs::write(&up_path, &up)
        .with_context(|| format!("failed to write {}", up_path.display()))?;
    fs::write(&down_path, &down)
        .with_context(|| format!("failed to write {}", down_path.display()))?;

    println!(
        "{} decorator chain for {} table(s) over schema(s) [{}]",
        "Emitted".green().bold(),
        targets.len(),
        descriptor.scoped_schemas.join(", ")
    );
    println!("  up   → {}", up_path.display());
    println!("  down → {}", down_path.display());
    println!(
        "  {} list the pair under `user_owned:` in metaphor.codegen.yaml — it is\n\
              service-owned, not generator-swept",
        "next:".yellow().bold()
    );
    Ok(())
}

fn load_descriptor(path: &Path) -> Result<TenancyDescriptor> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    parse_descriptor(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

/// Parse + vet a tenancy descriptor body. Version and non-empty-tables are
/// checked here (not by serde) so every entry point — file load, tests, a
/// future embedded descriptor — shares the same guard.
fn parse_descriptor(raw: &str) -> Result<TenancyDescriptor> {
    let descriptor: TenancyDescriptor =
        serde_yaml::from_str(raw).context("invalid tenancy.yaml structure")?;
    anyhow::ensure!(
        descriptor.version == 1,
        "unsupported tenancy.yaml version {} (expected 1)",
        descriptor.version
    );
    anyhow::ensure!(
        !descriptor.tables.is_empty(),
        "tenancy.yaml lists no tables — nothing to decorate"
    );
    Ok(descriptor)
}

/// Resolve every descriptor entry to a qualified table target by loading the
/// named module's schema from the workspace. This is where typos die: a module
/// not in `metaphor.yaml`, or a table the module does not define, fails here
/// instead of emitting SQL against a table that doesn't exist.
fn resolve_targets(cwd: &Path, descriptor: &TenancyDescriptor) -> Result<Vec<TenancyTableTarget>> {
    let Some(ws) = crate::commands::workspace::Workspace::from_cwd(cwd) else {
        anyhow::bail!("no metaphor.yaml found — run this from inside a workspace");
    };

    let mut targets = Vec::with_capacity(descriptor.tables.len());
    for entry in &descriptor.tables {
        let project = ws
            .project_by_name(&entry.module)
            .with_context(|| format!("module '{}' is not a project in metaphor.yaml", entry.module))?;
        let schema_dir = ws.project_path(project).join("schema");
        anyhow::ensure!(
            schema_dir.is_dir(),
            "project '{}' has no schema/ directory ({})",
            entry.module,
            schema_dir.display()
        );
        let files = find_schema_files(&schema_dir)?;
        let (module_schema, parse_errors) = build_module_schema(&entry.module, &files)?;
        anyhow::ensure!(
            parse_errors.is_empty(),
            "module '{}' has schema parse errors: {parse_errors:?}",
            entry.module
        );

        let model = module_schema
            .models
            .iter()
            .find(|m| m.collection_name() == entry.table)
            .with_context(|| {
                format!(
                    "module '{}' defines no table '{}' (collections: {})",
                    entry.module,
                    entry.table,
                    module_schema
                        .models
                        .iter()
                        .map(|m| m.collection_name())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        let schema = model
            .schema
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "public".to_string());
        anyhow::ensure!(
            descriptor.scoped_schemas.contains(&schema),
            "table {}.{} resolves to schema '{}' which is not in scoped_schemas — \
             its future sibling tables would escape deny-by-default coverage",
            schema,
            entry.table,
            schema
        );

        targets.push(TenancyTableTarget {
            schema,
            table: entry.table.clone(),
            allow_root: entry.allow_root,
            uniques: entry
                .uniques
                .iter()
                .map(|u| TenancyUnique {
                    fields: u.fields.clone(),
                    where_clause: u.r#where.clone(),
                })
                .collect(),
        });
    }
    Ok(targets)
}

/// Coverage doctor: descriptor vs live introspection. Every listed table must
/// carry the installed column (NOT NULL, acting-unit DEFAULT), the org policy,
/// the kind-guard trigger, and each per-unit unique; the deny event trigger
/// must exist. Exits non-zero on any gap, so it can gate CI.
#[cfg(feature = "database")]
async fn check_coverage(
    descriptor: &TenancyDescriptor,
    targets: &[TenancyTableTarget],
    url: &str,
) -> Result<()> {
    use sqlx::postgres::PgPool;

    let pool = PgPool::connect(url)
        .await
        .context("failed to connect for --check")?;

    let mut gaps: Vec<String> = Vec::new();

    for target in targets {
        let qualified = target.qualified();
        let col: Option<(String, String)> = sqlx::query_as(
            "SELECT is_nullable, coalesce(column_default, '') FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = $2 AND column_name = 'org_unit_id'",
        )
        .bind(&target.schema)
        .bind(&target.table)
        .fetch_optional(&pool)
        .await
        .context("column introspection failed")?;
        match col {
            None => gaps.push(format!("{qualified}: org_unit_id column missing")),
            Some((nullable, default)) => {
                if nullable != "NO" {
                    gaps.push(format!("{qualified}: org_unit_id is nullable"));
                }
                if !default.contains("app.acting_unit_id") {
                    gaps.push(format!(
                        "{qualified}: org_unit_id default is not the acting-unit resolver ({default:?})"
                    ));
                }
            }
        }

        let policy: Option<(String,)> = sqlx::query_as(
            "SELECT policyname FROM pg_policies WHERE schemaname = $1 AND tablename = $2 \
             AND policyname = $3",
        )
        .bind(&target.schema)
        .bind(&target.table)
        .bind(target.policy_name())
        .fetch_optional(&pool)
        .await
        .context("policy introspection failed")?;
        if policy.is_none() {
            gaps.push(format!(
                "{qualified}: policy {} missing",
                target.policy_name()
            ));
        }

        // pg_class carries the RLS flags (pg_tables has no force column) and the
        // kind-guard trigger hangs off the table's own name.
        let rls: Option<(bool, bool)> = sqlx::query_as(
            "SELECT c.relrowsecurity, c.relforcerowsecurity FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2",
        )
        .bind(&target.schema)
        .bind(&target.table)
        .fetch_optional(&pool)
        .await
        .context("rls introspection failed")?;
        match rls {
            None => gaps.push(format!("{qualified}: table missing")),
            Some((enabled, forced)) => {
                if !enabled || !forced {
                    gaps.push(format!(
                        "{qualified}: RLS enabled={enabled} forced={forced} (need both)"
                    ));
                }
            }
        }

        let guard: Option<(String,)> = sqlx::query_as(
            "SELECT tgname FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND t.tgname = $3 AND NOT t.tgisinternal",
        )
        .bind(&target.schema)
        .bind(&target.table)
        .bind(format!("{}_org_unit_kind_guard", target.table))
        .fetch_optional(&pool)
        .await
        .context("trigger introspection failed")?;
        if guard.is_none() {
            gaps.push(format!(
                "{qualified}: kind-guard trigger {}_org_unit_kind_guard missing",
                target.table
            ));
        }

        // The insert-path unit stamp: same introspection as the guard, and its
        // name must sort before the guard's so it stamps NULL ids first.
        let fill: Option<(String,)> = sqlx::query_as(
            "SELECT tgname FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND t.tgname = $3 AND NOT t.tgisinternal",
        )
        .bind(&target.schema)
        .bind(&target.table)
        .bind(format!("{}_org_unit_fill", target.table))
        .fetch_optional(&pool)
        .await
        .context("trigger introspection failed")?;
        if fill.is_none() {
            gaps.push(format!(
                "{qualified}: unit-fill trigger {}_org_unit_fill missing",
                target.table
            ));
        }

        for unique in &target.uniques {
            // Same derivation as the emitter — expression fields (`lower(name)`)
            // sanitize to identifier-safe name parts on both sides.
            let name = target.unique_index_name(unique);
            let idx: Option<(String,)> = sqlx::query_as(
                "SELECT indexname FROM pg_indexes WHERE schemaname = $1 AND tablename = $2 \
                 AND indexname = $3",
            )
            .bind(&target.schema)
            .bind(&target.table)
            .bind(&name)
            .fetch_optional(&pool)
            .await
            .context("index introspection failed")?;
            if idx.is_none() {
                gaps.push(format!("{qualified}: unique index {name} missing"));
            }
        }
    }

    if !descriptor.scoped_schemas.is_empty() {
        let evt: Option<(String,)> =
            sqlx::query_as("SELECT evtname FROM pg_event_trigger WHERE evtname = $1")
                .bind("tenancy_deny_undecorated_table")
                .fetch_optional(&pool)
                .await
                .context("event-trigger introspection failed")?;
        if evt.is_none() {
            gaps.push("deny-by-default event trigger tenancy_deny_undecorated_table missing"
                .to_string());
        }
    }

    pool.close().await;

    if gaps.is_empty() {
        println!(
            "{} every descriptor table is decorated and coverage is armed",
            "Coverage passed:".green().bold()
        );
        Ok(())
    } else {
        for gap in &gaps {
            println!("  {} {gap}", "gap:".red().bold());
        }
        anyhow::bail!("tenancy coverage check failed: {} gap(s)", gaps.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_target() -> TenancyTableTarget {
        TenancyTableTarget {
            schema: "party".into(),
            table: "parties".into(),
            allow_root: false,
            uniques: vec![
                TenancyUnique {
                    fields: vec!["party_code".into()],
                    where_clause: Some("(metadata->>'deleted_at') IS NULL".into()),
                },
                TenancyUnique {
                    fields: vec!["npwp".into()],
                    where_clause: Some(
                        "npwp IS NOT NULL AND (metadata->>'deleted_at') IS NULL".into(),
                    ),
                },
            ],
        }
    }

    #[test]
    fn chain_is_guarded_and_marker_free() {
        let (up, down) = tenancy_decorator_chain(
            &[sample_target()],
            &["party".to_string()],
        );

        // Every statement class the re-run story depends on.
        assert!(up.contains("ADD COLUMN IF NOT EXISTS org_unit_id"));
        assert!(up.contains("SET NOT NULL"));
        assert!(up.contains("app.acting_unit_id"));
        assert!(up.contains("parties_org_unit_isolation"));
        assert!(up.contains("parties_org_unit_kind_guard"));
        assert!(up.contains("parties_org_unit_fill"));
        assert!(up.contains("CREATE TRIGGER parties_org_unit_fill"));
        assert!(up.contains("BEFORE INSERT ON party.parties"));
        // The stamp resolves the acting unit and leaves unbound scopes NULL.
        assert!(up.contains(
            "nullif(current_setting('app.acting_unit_id', true), '')::uuid"
        ));
        assert!(up.contains("CREATE UNIQUE INDEX IF NOT EXISTS uq_parties_org_unit_id_party_code"));
        assert!(up.contains("CREATE UNIQUE INDEX IF NOT EXISTS uq_parties_org_unit_id_npwp"));
        assert!(up.contains("WHERE npwp IS NOT NULL"));
        assert!(up.contains("DROP EVENT TRIGGER IF EXISTS tenancy_deny_undecorated_table"));
        assert!(up.contains("CREATE EVENT TRIGGER tenancy_deny_undecorated_table"));
        assert!(up.contains("schema_name IN ('party')"));

        // The backfill is conditional on company_id existing at runtime, not at
        // emission time — the chain converges pre-strip and post-strip alike.
        assert!(up.contains("AND column_name = 'company_id'"));
        assert!(up.contains("SET org_unit_id = company_id"));

        // The sweep marker must NOT appear — that string is what makes a
        // migration generator-swept, and this chain is service-owned.
        assert!(
            !up.contains("Generated by metaphor-schema"),
            "chain header carries the sweep marker"
        );
        assert!(
            !down.contains("Generated by metaphor-schema"),
            "down header carries the sweep marker"
        );

        // The down reverses in dependency order and drops the column.
        let uniques_pos = down.find("DROP INDEX IF EXISTS uq_parties_org_unit_id_party_code");
        let column_pos = down.find("DROP COLUMN IF EXISTS org_unit_id");
        assert!(uniques_pos.unwrap() < column_pos.unwrap());
        assert!(down.contains("DROP TRIGGER IF EXISTS parties_org_unit_fill"));
        assert!(down.contains("DROP FUNCTION IF EXISTS party.parties_org_unit_fill"));
        assert!(down.contains("DROP EVENT TRIGGER IF EXISTS tenancy_deny_undecorated_table"));
    }

    #[test]
    fn empty_scoped_schemas_omits_event_trigger() {
        let (up, _down) = tenancy_decorator_chain(&[sample_target()], &[]);
        assert!(!up.contains("CREATE EVENT TRIGGER"));
    }

    #[test]
    fn expression_fields_get_identifier_safe_index_names() {
        let target = TenancyTableTarget {
            schema: "blog".into(),
            table: "tags".into(),
            allow_root: false,
            uniques: vec![
                TenancyUnique {
                    fields: vec!["lower(name)".into()],
                    where_clause: None,
                },
                TenancyUnique {
                    fields: vec!["slug".into()],
                    where_clause: None,
                },
            ],
        };
        let (up, down) = tenancy_decorator_chain(&[target], &["blog".to_string()]);

        // The expression rides verbatim into the column list…
        assert!(up.contains(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_tags_org_unit_id_lower_name ON blog.tags (org_unit_id, lower(name));"
        ));
        // …while its punctuation never leaks into the identifier itself.
        assert!(!up.contains("uq_tags_org_unit_id_lower(name)"));
        assert!(down.contains("DROP INDEX IF EXISTS uq_tags_org_unit_id_lower_name;"));
        assert!(down.contains("DROP INDEX IF EXISTS uq_tags_org_unit_id_slug;"));
    }

    #[test]
    fn descriptor_parses_the_documented_shape() {
        let yaml = "\
version: 1
scoped_schemas: [party]
tables:
  - module: backbone-party
    table: parties
    allow_root: false
    uniques:
      - fields: [party_code]
        where: (metadata->>'deleted_at') IS NULL
";
        let d: TenancyDescriptor = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(d.version, 1);
        assert_eq!(d.scoped_schemas, vec!["party".to_string()]);
        assert_eq!(d.tables.len(), 1);
        assert_eq!(d.tables[0].module, "backbone-party");
        assert!(!d.tables[0].allow_root);
        assert_eq!(d.tables[0].uniques[0].fields, vec!["party_code".to_string()]);
        assert_eq!(
            d.tables[0].uniques[0].r#where.as_deref(),
            Some("(metadata->>'deleted_at') IS NULL")
        );
    }

    #[test]
    fn descriptor_rejects_unknown_version() {
        let yaml = "version: 2\ntables:\n  - module: m\n    table: t\n";
        let err = parse_descriptor(yaml).unwrap_err();
        assert!(err.to_string().contains("version"), "got: {err}");
    }

    #[test]
    fn descriptor_rejects_empty_table_list() {
        let err = parse_descriptor("version: 1\nscoped_schemas: [party]\ntables: []\n")
            .unwrap_err();
        assert!(err.to_string().contains("no tables"), "got: {err}");
    }
}
