use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
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

    // Validate: all source files must exist before we touch anything
    for (src, _) in &moves {
        if !src.exists() {
            bail!("source file does not exist: {}", src.display());
        }
    }

    // Build a lookup: old_abs → new_abs
    let move_map: HashMap<PathBuf, PathBuf> = moves.iter().cloned().collect();

    // 1. Collect all .md files under root
    let md_files = collect_md_files(root);

    // 2. Read all content upfront — single I/O pass, reused for both
    //    link analysis and (if !dry_run) writing the updated content.
    let mut file_contents: HashMap<PathBuf, String> = HashMap::with_capacity(md_files.len());
    for md_file in &md_files {
        let content = fs::read_to_string(md_file)
            .with_context(|| format!("reading {}", md_file.display()))?;
        file_contents.insert(md_file.clone(), content);
    }

    // 3. Compute required replacements for every affected file
    let mut replacements: HashMap<PathBuf, Vec<Replacement>> = HashMap::new();

    for md_file in &md_files {
        let content = &file_contents[md_file];
        let links = parse_links(content, md_file);
        let file_dir = md_file.parent().unwrap_or(Path::new("."));

        for link in &links {
            let (path_part, _frag) = split_fragment(&link.href);

            // --- Case A: this file is one of the sources being moved ---
            // Its relative links need rebasing because the file itself moves.
            if let Some(new_src) = move_map.get(md_file) {
                let new_src_dir = new_src.parent().unwrap_or(Path::new("."));

                if let Some(resolved) = &link.resolved {
                    if let Some(new_target) = move_map.get(resolved) {
                        // Both the containing file AND its target are moving:
                        // point from new_src_dir directly to new_target.
                        let new_href = retarget_link(&link.href, new_src_dir, new_target);
                        if new_href != link.href {
                            push_replacement(&mut replacements, md_file, link, new_href);
                        }
                        continue;
                    }
                }

                // Target is not moving — rebase from old dir to new dir
                if !path_part.is_empty() && resolve_href(path_part, file_dir).is_some() {
                    let new_href = rebase_link(&link.href, file_dir, new_src_dir);
                    if new_href != link.href {
                        push_replacement(&mut replacements, md_file, link, new_href);
                    }
                }
            }

            // --- Case B: file is NOT moving, but links to a file that IS ---
            else if let Some(resolved) = &link.resolved {
                if let Some(new_target) = move_map.get(resolved) {
                    let new_href = retarget_link(&link.href, file_dir, new_target);
                    if new_href != link.href {
                        push_replacement(&mut replacements, md_file, link, new_href);
                    }
                }
            }
        }
    }

    // 4. Build the human-readable report
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
        // 5. Apply content changes using the cached content (no second read)
        for (file_path, reps) in &replacements {
            let content = &file_contents[file_path];

            let mut sorted = reps.clone();
            sorted.sort_by_key(|r| r.start);
            sorted.dedup_by_key(|r| r.start);

            let new_content = apply_replacements(content, &sorted)
                .with_context(|| format!("applying replacements to {}", file_path.display()))?;

            fs::write(file_path, new_content)
                .with_context(|| format!("writing {}", file_path.display()))?;
        }

        // 6. Rename/move the source files to their destinations
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn push_replacement(
    map: &mut HashMap<PathBuf, Vec<Replacement>>,
    file: &PathBuf,
    link: &crate::parser::Link,
    new_href: String,
) {
    map.entry(file.clone()).or_default().push(Replacement {
        start: link.start,
        end: link.end,
        new_href,
        old_href: link.href.clone(),
    });
}

/// Collect all `.md` files under `root` (follows symlinks, silently skips errors).
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

/// Apply a list of replacements to `content` in reverse byte-offset order.
///
/// Returns an error if the byte splicing somehow produces invalid UTF-8 (should
/// not happen in practice since hrefs are valid UTF-8 substrings, but we
/// handle it gracefully instead of panicking).
fn apply_replacements(content: &str, reps: &[Replacement]) -> Result<String> {
    // Sort descending so earlier offsets aren't invalidated by later splices
    let mut sorted = reps.to_vec();
    sorted.sort_by(|a, b| b.start.cmp(&a.start));

    let mut bytes = content.as_bytes().to_vec();
    for rep in sorted {
        bytes.splice(rep.start..rep.end, rep.new_href.as_bytes().iter().cloned());
    }

    String::from_utf8(bytes).context("replacement produced invalid UTF-8 — this is a bug, please report it")
}
