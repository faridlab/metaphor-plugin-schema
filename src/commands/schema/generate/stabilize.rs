//! Stabilize migration filenames against on-disk state so a regen keeps each
//! logical migration at a stable timestamp — no renumbering and no collisions,
//! under both plain `generate` and `--force`.
//!
//! Runs after generation and before [`super::migration_cleanup`] / the write
//! phase. Because the cleanup pass preserves any on-disk migration whose
//! filename appears in the generated set, feeding it already-stabilized names
//! (existing identities reused at their existing on-disk timestamp) makes the
//! cleanup preserve them and delete only genuinely-orphaned generated files.
//!
//! Rules applied to every generated `migrations/<ts>_<suffix>.{up,down}.sql`:
//!
//! - If a file with the same `<suffix>` already exists on disk, reuse its
//!   timestamp (the migration is overwritten in place at a stable identity).
//! - Otherwise it is genuinely new: assign a fresh timestamp strictly greater
//!   than every existing migration timestamp, so it slots after them and never
//!   collides. Multiple new migrations in one run get successive timestamps,
//!   assigned in sorted order for determinism across runs.
//!
//! If the `migrations/` dir does not exist yet (first generation), the
//! positional timestamps produced by the generators are left untouched.

use std::path::{Path, PathBuf};

use crate::generators::GeneratedOutput;

use super::super::migrations::{
    existing_timestamp_for_suffix, is_unstable_timestamped_migration, max_migration_timestamp,
};

pub(super) fn stabilize_migration_timestamps(generated: &mut GeneratedOutput, output_dir: &Path) {
    let migrations_dir = output_dir.join("migrations");
    if !migrations_dir.exists() {
        return; // First generation: positional timestamps are correct.
    }

    let Some(base) = max_migration_timestamp(&migrations_dir) else {
        return; // No timestamped files on disk — nothing to stabilize against.
    };
    let mut next_new = bump_timestamp(&base);

    // Collect the generated migration paths and sort them so the assignment of
    // fresh timestamps to genuinely-new migrations is deterministic across runs
    // (HashMap iteration order is not).
    let mut mig_paths: Vec<PathBuf> = generated
        .files
        .keys()
        .filter(|p| is_unstable_timestamped_migration(p))
        .cloned()
        .collect();
    mig_paths.sort();

    // (old_path, new_path) remappings to apply after the iteration.
    let mut remaps: Vec<(PathBuf, PathBuf)> = Vec::new();

    for path in &mig_paths {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(suffix) = name.splitn(2, '_').nth(1) else {
            continue;
        };
        let Some(parent) = path.parent() else {
            continue;
        };

        let new_ts = match existing_timestamp_for_suffix(&migrations_dir, suffix) {
            // Existing identity: reuse its on-disk timestamp (overwrite in place).
            Some(existing) => existing,
            // Genuinely new: assign the next fresh, strictly-later timestamp.
            None => {
                let ts = next_new.clone();
                next_new = bump_timestamp(&next_new);
                ts
            }
        };

        let new_path = parent.join(format!("{}_{}", new_ts, suffix));
        if new_path != *path {
            remaps.push((path.clone(), new_path));
        }
    }

    for (old, new) in remaps {
        if let Some(content) = generated.files.remove(&old) {
            generated.files.insert(new, content);
        }
    }
}

/// Add one second to a `YYYYMMDDHHMMSS` timestamp string, returning the next.
/// Falls back to the input unchanged if parsing fails (should not happen for
/// values produced by `max_migration_timestamp`).
fn bump_timestamp(ts: &str) -> String {
    use chrono::{TimeZone, NaiveDateTime};
    if let Ok(ndt) = NaiveDateTime::parse_from_str(ts, "%Y%m%d%H%M%S") {
        let utc = chrono::Utc.from_utc_datetime(&ndt);
        return (utc + chrono::Duration::seconds(1))
            .format("%Y%m%d%H%M%S")
            .to_string();
    }
    ts.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::GeneratedOutput;
    use std::path::Path;

    fn mig(ts: &str, suffix: &str) -> PathBuf {
        PathBuf::from(format!("migrations/{}_{}", ts, suffix))
    }

    fn has(generated: &GeneratedOutput, ts: &str, suffix: &str) -> bool {
        generated.files.contains_key(Path::new(&format!("migrations/{}_{}", ts, suffix)))
    }

    #[test]
    fn bump_adds_one_second() {
        assert_eq!(bump_timestamp("20260723180001"), "20260723180002");
        // Rolls over minutes/hours/days correctly via chrono.
        assert_eq!(bump_timestamp("20260723180059"), "20260723180100");
        assert_eq!(bump_timestamp("20260723235959"), "20260724000000");
    }

    #[test]
    fn existing_identity_keeps_its_on_disk_timestamp() {
        // On disk: company already at 20260426220001. The generator (out of
        // topo order) produced it at a different positional timestamp.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let migrations_dir = dir.join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("20260426220001_create_company_table.up.sql"),
            "-- Generated by metaphor-schema\n-- old\n",
        )
        .unwrap();

        let mut generated = GeneratedOutput::default();
        generated.files.insert(
            mig("20260426220009", "create_company_table.up.sql"),
            "-- new body\n".to_string(),
        );

        stabilize_migration_timestamps(&mut generated, dir);

        // Remapped to the existing on-disk timestamp; body preserved.
        assert!(has(&generated, "20260426220001", "create_company_table.up.sql"));
        assert!(!has(&generated, "20260426220009", "create_company_table.up.sql"));
        assert_eq!(
            generated
                .files
                .get(Path::new("migrations/20260426220001_create_company_table.up.sql"))
                .unwrap(),
            "-- new body\n"
        );
    }

    #[test]
    fn new_migration_gets_max_plus_one_no_collision() {
        // On disk: max timestamp is 20260426220006 (audit triggers).
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let migrations_dir = dir.join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("20260426220006_add_audit_triggers.up.sql"),
            "-- Generated by metaphor-schema\n",
        )
        .unwrap();
        std::fs::write(
            migrations_dir.join("20260426220003_create_department_table.up.sql"),
            "-- Generated by metaphor-schema\n",
        )
        .unwrap();

        // Generator produced a NEW entity at a colliding positional slot (003).
        let mut generated = GeneratedOutput::default();
        generated.files.insert(
            mig("20260426220003", "create_industry_table.up.sql"),
            "-- new\n".to_string(),
        );

        stabilize_migration_timestamps(&mut generated, dir);

        // Must NOT be 003 (collision); must be max+1 = 007.
        assert!(has(&generated, "20260426220007", "create_industry_table.up.sql"));
        assert!(!has(&generated, "20260426220003", "create_industry_table.up.sql"));
    }

    #[test]
    fn multiple_new_migrations_get_distinct_successive_timestamps() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let migrations_dir = dir.join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("20260426220006_add_audit_triggers.up.sql"),
            "-- Generated by metaphor-schema\n",
        )
        .unwrap();

        let mut generated = GeneratedOutput::default();
        generated.files.insert(
            mig("20260426220003", "create_industry_table.up.sql"),
            "-- a\n".to_string(),
        );
        generated.files.insert(
            mig("20260426220003", "create_company_industry_table.up.sql"),
            "-- b\n".to_string(),
        );

        stabilize_migration_timestamps(&mut generated, dir);

        let mut tses: Vec<String> = generated
            .files
            .keys()
            .filter_map(|p| p.file_name()?.to_str())
            .filter_map(|n| n.get(..14).map(|s| s.to_string()))
            .collect();
        // Both new, both > max(006), and distinct.
        assert!(tses.iter().all(|t| t.as_str() > "20260426220006"));
        assert_eq!(tses.len(), 2);
        tses.sort();
        tses.dedup();
        assert_eq!(tses.len(), 2); // no duplicates
    }

    #[test]
    fn no_migrations_dir_leaves_paths_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path(); // no migrations/ subdir
        let mut generated = GeneratedOutput::default();
        generated.files.insert(
            mig("20260426220001", "create_company_table.up.sql"),
            "x".to_string(),
        );
        stabilize_migration_timestamps(&mut generated, dir);
        assert!(has(&generated, "20260426220001", "create_company_table.up.sql"));
    }

    #[test]
    fn non_migration_files_are_not_renamed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("migrations")).unwrap();
        std::fs::write(
            dir.join("migrations").join("20260426220006_add_audit_triggers.up.sql"),
            "-- Generated by metaphor-schema\n",
        )
        .unwrap();
        let mut generated = GeneratedOutput::default();
        generated.files.insert(
            PathBuf::from("src/domain/entity/company.rs"),
            "pub struct Company;\n".to_string(),
        );
        stabilize_migration_timestamps(&mut generated, dir);
        assert!(generated
            .files
            .contains_key(Path::new("src/domain/entity/company.rs")));
    }
}
