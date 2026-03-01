use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::{
    parser::{parse_links, split_fragment},
    refactor::{normalize_path, rebase_link, resolve_href, retarget_link},
};

/// A single link replacement to apply to a file's contents.
#[derive(Debug, Clone)]
struct Replacement {
    start: usize,
    end: usize,
    new_href: String,
    old_href: String,
}

/// Per-file report of what changed.
#[derive(Debug, Default)]
pub struct FileReport {
    pub path: PathBuf,
    pub changes: Vec<LinkChange>,
}

#[derive(Debug, Clone)]
pub struct LinkChange {
    pub old_href: String,
    pub new_href: String,
}

/// Summary returned from `move_files`.
pub struct MoveReport {
    pub file_reports: Vec<FileReport>,
    pub files_moved: usize,
    pub links_updated: usize,
}

/// Orchestrate moving `moves` (pairs of absolute source → absolute destination paths).
///
/// `root`    — directory to scan for .md files
/// `dry_run` — if true, compute and report changes without writing anything
/// `verbose` — if true, report unchanged links too (currently unused here;
///             the caller decides what to print from the report)
pub fn move_files(
    moves: &[(PathBuf, PathBuf)],
    root: &Path,
    dry_run: bool,
) -> Result<MoveReport> {
    // Normalize all move paths
    let moves: Vec<(PathBuf, PathBuf)> = moves
        .iter()
        .map(|(s, d)| (normalize_path(s), normalize_path(d)))
        .collect();

    // Build a lookup: old_abs → new_abs
    let move_map: HashMap<PathBuf, PathBuf> = moves.iter().cloned().collect();

    // 1. Collect all .md files under root
    let md_files = collect_md_files(root);

    // 2. For every .md file, parse its links and compute required replacements
    //    We accumulate: file_path → Vec<Replacement>
    let mut replacements: HashMap<PathBuf, Vec<Replacement>> = HashMap::new();

    for md_file in &md_files {
        let content = fs::read_to_string(md_file)
            .with_context(|| format!("reading {}", md_file.display()))?;

        let links = parse_links(&content, md_file);
        let file_dir = md_file.parent().unwrap_or(Path::new("."));

        for link in &links {
            let (path_part, _frag) = split_fragment(&link.href);

            // --- Case A: this file is one of the sources being moved ---
            // Its relative links need rebasing because the file itself moves.
            if let Some(new_src) = move_map.get(md_file) {
                let new_src_dir = new_src.parent().unwrap_or(Path::new("."));

                // If the link's target is ALSO moving, we need to point to its
                // new location from the new source directory.
                if let Some(resolved) = &link.resolved {
                    if let Some(new_target) = move_map.get(resolved) {
                        // Both the containing file AND the target are moving
                        let new_href = retarget_link_from_new_src(
                            &link.href,
                            new_src_dir,
                            new_target,
                        );
                        if new_href != link.href {
                            replacements
                                .entry(md_file.clone())
                                .or_default()
                                .push(Replacement {
                                    start: link.start,
                                    end: link.end,
                                    new_href,
                                    old_href: link.href.clone(),
                                });
                        }
                        continue;
                    }
                }

                // Target is not moving — just rebase from old dir to new dir
                if !path_part.is_empty() && resolve_href(path_part, file_dir).is_some() {
                    let new_href = rebase_link(&link.href, file_dir, new_src_dir);
                    if new_href != link.href {
                        replacements
                            .entry(md_file.clone())
                            .or_default()
                            .push(Replacement {
                                start: link.start,
                                end: link.end,
                                new_href,
                                old_href: link.href.clone(),
                            });
                    }
                }
            }

            // --- Case B: this file is NOT moving, but it links to a file that IS ---
            else if let Some(resolved) = &link.resolved {
                if let Some(new_target) = move_map.get(resolved) {
                    let new_href = retarget_link(&link.href, file_dir, new_target);
                    if new_href != link.href {
                        replacements
                            .entry(md_file.clone())
                            .or_default()
                            .push(Replacement {
                                start: link.start,
                                end: link.end,
                                new_href,
                                old_href: link.href.clone(),
                            });
                    }
                }
            }
        }
    }

    // 3. Build the human-readable report
    let mut file_reports: Vec<FileReport> = replacements
        .iter()
        .map(|(path, reps)| FileReport {
            path: path.clone(),
            changes: reps
                .iter()
                .map(|r| LinkChange {
                    old_href: r.old_href.clone(),
                    new_href: r.new_href.clone(),
                })
                .collect(),
        })
        .collect();
    file_reports.sort_by(|a, b| a.path.cmp(&b.path));

    let links_updated: usize = file_reports.iter().map(|r| r.changes.len()).sum();
    let files_moved = moves.len();

    if !dry_run {
        // 4. Apply content changes
        for (file_path, reps) in &replacements {
            let content = fs::read_to_string(file_path)
                .with_context(|| format!("reading {}", file_path.display()))?;

            // Dedup: same byte range might appear twice (shouldn't, but guard)
            let mut sorted_reps = reps.clone();
            sorted_reps.sort_by_key(|r| r.start);
            sorted_reps.dedup_by_key(|r| r.start);

            let new_content = apply_replacements(&content, &sorted_reps);

            fs::write(file_path, new_content)
                .with_context(|| format!("writing {}", file_path.display()))?;
        }

        // 5. Move the files
        for (src, dst) in &moves {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating dir {}", parent.display()))?;
            }
            fs::rename(src, dst)
                .with_context(|| format!("moving {} → {}", src.display(), dst.display()))?;
        }
    }

    Ok(MoveReport {
        file_reports,
        files_moved,
        links_updated,
    })
}

/// Collect all `.md` files under `root` (follows symlinks, ignores errors).
fn collect_md_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().map_or(false, |ext| ext == "md")
        })
        .map(|e| normalize_path(e.path()))
        .collect()
}

/// Like `retarget_link` but the source file itself has also moved, so we
/// compute the href from `new_src_dir` to `new_target`.
fn retarget_link_from_new_src(href: &str, new_src_dir: &Path, new_target: &Path) -> String {
    crate::refactor::retarget_link(href, new_src_dir, new_target)
}

/// Apply a list of replacements to `content` in reverse byte-offset order.
fn apply_replacements(content: &str, reps: &[Replacement]) -> String {
    // Sort descending so earlier offsets aren't invalidated
    let mut sorted = reps.to_vec();
    sorted.sort_by(|a, b| b.start.cmp(&a.start));

    let mut bytes = content.as_bytes().to_vec();
    for rep in sorted {
        let range = rep.start..rep.end;
        bytes.splice(range, rep.new_href.as_bytes().iter().cloned());
    }
    String::from_utf8(bytes).expect("utf8 invariant: only replacing ascii hrefs")
}
