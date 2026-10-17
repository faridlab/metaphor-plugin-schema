//! Removing what an earlier run generated for something the schema no longer declares.
//!
//! The generator writes every file of a module fresh, but nothing deletes the files of an entity
//! that has left the schema: they stay behind, still exported by nobody, still compiled, and
//! still read by a screen that would otherwise fail loudly. After a module is generated, the
//! files under its output directory that this run did not write are removed, with three limits
//! that keep the removal from ever reaching a person's work:
//!
//! - only a file the generator recognisably wrote is a candidate: one whose header carries the
//!   generator's mark (see [`is_generated`]). A file without it is never touched;
//! - only a directory this run wrote into is swept, so a run limited to some targets or to one
//!   entity cannot sweep away what other targets wrote;
//! - a stale file whose `// <<< CUSTOM` block holds authored code is kept and reported, since
//!   that code was written by hand.
//!
//! Directories left empty are removed too.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a sweep removed, or would remove on a dry run.
#[derive(Debug, Default, Clone)]
pub struct PruneReport {
    /// Stale generated files removed (or, on a dry run, that would be).
    pub removed: Vec<PathBuf>,
    /// Stale generated files kept because a CUSTOM block in them holds authored code.
    pub kept_authored: Vec<PathBuf>,
    /// Directories removed because nothing was left in them.
    pub removed_dirs: Vec<PathBuf>,
}

/// Phrases the generator writes into the header of the files it emits. The first two are its
/// marks; the rest are fixed sentences of per-entity templates that carried no mark in earlier
/// releases, so files those releases wrote are still recognised.
const MARKS: &[&str] = &[
    "metaphor-webgen",
    "Generated from schema definition",
    "Generic CRUD orchestration over the service port",
    "Built from the generic `makeCrudAppService`",
    "Extends the generic `BaseRepositoryImpl`, delegating to the API client",
    // The API client and service port name their base, which differs for a soft-delete entity.
    "Thin REST client — extends the generic ",
    "Implementations live in the\n * infrastructure layer.",
    "The infrastructure API client implements it;\n * the application layer consumes it via",
];

/// How many leading lines of a file make its header.
const HEADER_LINES: usize = 20;

/// Whether `content` is a file this generator wrote: its header carries one of the marks.
pub fn is_generated(content: &str) -> bool {
    let header: String = content
        .lines()
        .take(HEADER_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    MARKS.iter().any(|m| header.contains(m))
}

/// Whether a `// <<< CUSTOM … // END CUSTOM` block in `content` holds code someone wrote. A
/// generated placeholder body is empty or comments only.
fn has_authored_custom_code(content: &str) -> bool {
    let mut inside = false;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("// <<< CUSTOM") {
            inside = true;
        } else if t == "// END CUSTOM" {
            inside = false;
        } else if inside && !t.is_empty() && !t.starts_with("//") {
            return true;
        }
    }
    false
}

fn normal(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Sweep `module_dir` of the generated files this run did not write.
///
/// A file counts as written when it is in `written`, or, outside a dry run, when it was
/// modified at or after `since` (the run's start): every writer of the generator rewrites its
/// file whole, so this catches a file a writer did not list. On a dry run nothing is deleted
/// and the report says what would be.
pub fn prune_stale(
    module_dir: &Path,
    written: &[PathBuf],
    since: SystemTime,
    dry_run: bool,
) -> std::io::Result<PruneReport> {
    let mut report = PruneReport::default();
    if !module_dir.is_dir() {
        return Ok(report);
    }
    let listed: HashSet<PathBuf> = written.iter().map(|p| normal(p)).collect();
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(module_dir)
        .sort_by_file_name()
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    let was_written = |path: &Path| {
        listed.contains(&normal(path))
            || (!dry_run
                && std::fs::metadata(path)
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t >= since))
    };
    let touched: HashSet<PathBuf> = files
        .iter()
        .filter(|p| was_written(p))
        .filter_map(|p| p.parent().map(normal))
        .collect();

    for path in &files {
        if was_written(path) {
            continue;
        }
        let Some(parent) = path.parent() else {
            continue;
        };
        if !touched.contains(&normal(parent)) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if !is_generated(&content) {
            continue;
        }
        if has_authored_custom_code(&content) {
            report.kept_authored.push(path.clone());
            continue;
        }
        if !dry_run {
            std::fs::remove_file(path)?;
        }
        report.removed.push(path.clone());
    }

    if !dry_run {
        // Deepest first, so a directory emptied by removing its subdirectories goes too.
        let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(module_dir)
            .min_depth(1)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_dir())
            .map(|e| e.into_path())
            .collect();
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        for dir in dirs {
            if std::fs::read_dir(&dir).is_ok_and(|mut it| it.next().is_none()) {
                std::fs::remove_dir(&dir)?;
                report.removed_dirs.push(dir);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const GENERATED: &str = "/**\n * Widget API Client\n *\n * Generated by metaphor-webgen.\n */\nexport const x = 1;\n";
    const LEGACY: &str = "/**\n * Gadget use cases\n *\n * Generic CRUD orchestration over the service port (cross-cutting concerns like\n */\n";
    const HAND: &str =
        "// A helper someone wrote beside the generated files.\nexport const y = 2;\n";

    /// A module tree with a file the current run wrote, a stale generated file, a stale file
    /// from a release without the mark, a hand-written file, and a stale file with authored
    /// custom code; plus a directory the run never wrote into.
    fn tree() -> (tempfile::TempDir, PathBuf, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("billing");
        let api = module.join("infrastructure/api");
        let usecases = module.join("application/usecases");
        let untouched = module.join("presentation/forms");
        for d in [&api, &usecases, &untouched] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(api.join("WidgetApiClient.ts"), GENERATED).unwrap();
        fs::write(api.join("OldApiClient.ts"), GENERATED).unwrap();
        fs::write(api.join("helpers.ts"), HAND).unwrap();
        fs::write(
            api.join("KeptApiClient.ts"),
            format!("{GENERATED}// <<< CUSTOM\nexport const listSchema = 1;\n// END CUSTOM\n"),
        )
        .unwrap();
        fs::write(usecases.join("GadgetUseCases.ts"), LEGACY).unwrap();
        fs::write(untouched.join("OldForm.tsx"), GENERATED).unwrap();
        let written = vec![api.join("WidgetApiClient.ts")];
        (dir, module, written)
    }

    fn far_future() -> SystemTime {
        SystemTime::now() + std::time::Duration::from_secs(3600)
    }

    #[test]
    fn a_dry_run_deletes_nothing_and_never_names_a_hand_written_file() {
        let (_dir, module, written) = tree();
        let report = prune_stale(&module, &written, far_future(), true).unwrap();
        let names: Vec<String> = report
            .removed
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["OldApiClient.ts".to_string()],
            "only the stale generated file in a directory the run wrote into"
        );
        assert!(
            module.join("infrastructure/api/OldApiClient.ts").exists(),
            "a dry run deletes nothing"
        );
        assert!(module.join("infrastructure/api/helpers.ts").exists());
        assert!(report.removed_dirs.is_empty());
    }

    #[test]
    fn a_sweep_removes_stale_generated_files_only() {
        let (_dir, module, written) = tree();
        let report = prune_stale(&module, &written, far_future(), false).unwrap();
        let api = module.join("infrastructure/api");
        assert!(
            !api.join("OldApiClient.ts").exists(),
            "a stale generated file goes"
        );
        assert!(
            api.join("WidgetApiClient.ts").exists(),
            "a file this run wrote stays"
        );
        assert!(
            api.join("helpers.ts").exists(),
            "a file without the generator's mark is never deleted"
        );
        assert!(
            api.join("KeptApiClient.ts").exists(),
            "authored custom code is kept"
        );
        assert_eq!(report.kept_authored, vec![api.join("KeptApiClient.ts")]);
        // Directories the run never wrote into are not swept.
        assert!(module
            .join("application/usecases/GadgetUseCases.ts")
            .exists());
        assert!(module.join("presentation/forms/OldForm.tsx").exists());
    }

    #[test]
    fn a_file_from_a_release_without_the_mark_is_recognised_and_empty_directories_go() {
        let (_dir, module, mut written) = tree();
        let usecases = module.join("application/usecases");
        fs::write(usecases.join("WidgetUseCases.ts"), GENERATED).unwrap();
        written.push(usecases.join("WidgetUseCases.ts"));
        // A directory holding only a stale file is emptied and removed with its parent.
        let stale_dir = module.join("domain/service");
        fs::create_dir_all(&stale_dir).unwrap();
        fs::write(stale_dir.join("GoneService.ts"), GENERATED).unwrap();
        fs::write(stale_dir.join("WidgetService.ts"), GENERATED).unwrap();
        written.push(stale_dir.join("WidgetService.ts"));
        prune_stale(&module, &written, far_future(), false).unwrap();
        assert!(
            !usecases.join("GadgetUseCases.ts").exists(),
            "an unmarked template from an earlier release is still recognised"
        );
        assert!(!stale_dir.join("GoneService.ts").exists());

        let only = module.join("domain/repository");
        fs::create_dir_all(&only).unwrap();
        let report = prune_stale(&module, &written, far_future(), false).unwrap();
        assert!(
            report.removed_dirs.contains(&only),
            "{:?}",
            report.removed_dirs
        );
        assert!(!only.exists());
    }

    #[test]
    fn a_file_modified_during_the_run_counts_as_written() {
        let (_dir, module, _) = tree();
        // Nothing listed, but everything was modified after `since`: nothing is stale.
        let report = prune_stale(&module, &[], SystemTime::UNIX_EPOCH, false).unwrap();
        assert!(report.removed.is_empty(), "{:?}", report.removed);
    }

    #[test]
    fn the_mark_is_read_from_the_header_only() {
        assert!(is_generated(GENERATED));
        assert!(is_generated(LEGACY));
        // A soft-delete entity's client and port name another base class.
        assert!(is_generated("/**\n * Gadget API Client\n *\n * Thin REST client — extends the generic SoftDeleteCrudApiClient from `shared/crud`,\n */\n"));
        assert!(is_generated("/**\n * Gadget Service port + injectable accessor\n *\n * Extends the generic `SoftDeleteCrudService`. The infrastructure API client implements it;\n * the application layer consumes it via `getGadgetService()`.\n */\n"));
        assert!(!is_generated(HAND));
        let late = format!("{}// metaphor-webgen\n", "line\n".repeat(30));
        assert!(!is_generated(&late));
    }
}
