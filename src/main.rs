use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use markmv::{
    ops::{move_files, FileReport},
    refactor::normalize_path,
};

#[derive(Parser)]
#[command(name = "markmv", about = "Move markdown files and rewrite links")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Move markdown files and rewrite all affected links
    Move {
        /// Source file(s); last positional argument is the destination
        #[arg(required = true, num_args = 2..)]
        paths: Vec<PathBuf>,

        /// Print changes without writing files
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Show each changed link
        #[arg(short, long)]
        verbose: bool,

        /// Root directory to scan for .md files (default: current directory)
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Move {
            paths,
            dry_run,
            verbose,
            root,
        } => {
            let cwd = env::current_dir().context("getting current directory")?;

            // Resolve root to absolute before normalizing (avoid stripping "." → "")
            let root = root.unwrap_or_else(|| cwd.clone());
            let root = if root.is_absolute() {
                normalize_path(&root)
            } else {
                normalize_path(&cwd.join(&root))
            };

            // Split paths into sources + destination
            let (sources, dest_raw) = paths.split_at(paths.len() - 1);
            let dest_raw = &dest_raw[0];
            let dest = if dest_raw.is_absolute() {
                normalize_path(dest_raw)
            } else {
                normalize_path(&cwd.join(dest_raw))
            };

            // Resolve each source to absolute
            let sources: Vec<PathBuf> = sources
                .iter()
                .map(|p| {
                    if p.is_absolute() {
                        normalize_path(p)
                    } else {
                        normalize_path(&cwd.join(p))
                    }
                })
                .collect();

            let dest = if dest.is_absolute() {
                dest
            } else {
                normalize_path(&cwd.join(dest))
            };

            // Validate: if multiple sources, destination must be a directory
            let moves = build_move_pairs(&sources, &dest, dry_run)?;

            if dry_run {
                println!("[dry-run] No files will be modified.\n");
            }

            // Print the moves
            for (src, dst) in &moves {
                println!(
                    "Moving: {} → {}",
                    display_relative(&cwd, src),
                    display_relative(&cwd, dst)
                );
            }
            println!();

            let report = move_files(&moves, &root, dry_run)?;

            // Print per-file link reports
            for file_report in &report.file_reports {
                let is_src = moves.iter().any(|(s, _)| s == &file_report.path);
                let label = if is_src { " (self)" } else { "" };
                println!(
                    "  Updated {}{}:",
                    display_relative(&cwd, &file_report.path),
                    label
                );
                print_changes(file_report, verbose);
            }

            if !report.file_reports.is_empty() {
                println!();
            }

            println!(
                "{} file(s) moved, {} link(s) updated",
                report.files_moved, report.links_updated
            );

            Ok(())
        }
    }
}

/// Construct `(src_abs, dst_abs)` pairs.
fn build_move_pairs(
    sources: &[PathBuf],
    dest: &Path,
    dry_run: bool,
) -> Result<Vec<(PathBuf, PathBuf)>> {
    let dest_is_dir = if dry_run {
        // In dry-run mode the destination may not exist yet — infer from trailing `/`
        dest.to_string_lossy().ends_with('/')
            || dest.is_dir()
    } else {
        dest.is_dir()
    };

    if sources.len() > 1 && !dest_is_dir {
        bail!(
            "Destination '{}' must be a directory when moving multiple files",
            dest.display()
        );
    }

    let mut pairs = Vec::new();
    for src in sources {
        let dst = if dest_is_dir {
            let filename = src
                .file_name()
                .with_context(|| format!("source '{}' has no filename", src.display()))?;
            dest.join(filename)
        } else {
            dest.to_path_buf()
        };
        pairs.push((src.clone(), dst));
    }
    Ok(pairs)
}

fn print_changes(report: &FileReport, verbose: bool) {
    if report.changes.is_empty() {
        if verbose {
            println!("    (no link changes)");
        }
        return;
    }
    for change in &report.changes {
        println!("    {} → {}", change.old_href, change.new_href);
    }
}

/// Display a path relative to `base` if possible, otherwise absolute.
fn display_relative(base: &Path, path: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(base) {
        rel.to_string_lossy().into_owned()
    } else {
        path.to_string_lossy().into_owned()
    }
}
