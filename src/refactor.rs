use std::path::{Component, Path, PathBuf};

use crate::parser::split_fragment;

/// Normalize a path by resolving `.` and `..` components without touching
/// the filesystem (unlike `canonicalize`).
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Pop the last real component, if any
                if !out.pop() {
                    // If we can't pop (e.g. we're at root), keep the `..`
                    out.push(Component::ParentDir);
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Compute a relative path from directory `from_dir` to file `to_file`.
/// Both paths must be absolute and normalized.
pub fn make_relative(from_dir: &Path, to_file: &Path) -> PathBuf {
    // Find the common prefix length
    let from_parts: Vec<_> = from_dir.components().collect();
    let to_parts: Vec<_> = to_file.components().collect();

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let up_count = from_parts.len() - common;
    let mut result = PathBuf::new();
    for _ in 0..up_count {
        result.push("..");
    }
    for part in &to_parts[common..] {
        result.push(part);
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

/// Resolve a markdown href (possibly relative) to an absolute normalized path.
/// Returns `None` for external URLs or anchor-only hrefs.
pub fn resolve_href(href: &str, file_dir: &Path) -> Option<PathBuf> {
    if href.is_empty() || href.starts_with('#') || crate::parser::is_external(href) {
        return None;
    }
    let (path_part, _frag) = split_fragment(href);
    let raw = if Path::new(path_part).is_absolute() {
        PathBuf::from(path_part)
    } else {
        file_dir.join(path_part)
    };
    Some(normalize_path(&raw))
}

/// Rewrite a link that lives inside a file that has moved.
///
/// `href`        — the original href in the source file
/// `old_src_dir` — the directory the file was in before moving
/// `new_src_dir` — the directory the file is in after moving
///
/// The *target* of the link hasn't changed; only the file containing it moved.
pub fn rebase_link(href: &str, old_src_dir: &Path, new_src_dir: &Path) -> String {
    let (path_part, fragment) = split_fragment(href);

    // Resolve target relative to the old location
    let Some(abs_target) = resolve_href(path_part, old_src_dir) else {
        return href.to_owned();
    };

    // Compute the new relative path from the new location
    let new_rel = make_relative(new_src_dir, &abs_target);
    let mut result = new_rel.to_string_lossy().into_owned();

    // Ensure forward slashes on all platforms
    result = result.replace('\\', "/");

    // Preserve leading `./` if the original had it (avoids ambiguity)
    if !result.starts_with("..") && !result.starts_with('/') && !result.starts_with("./") {
        result = format!("./{result}");
    }

    if let Some(frag) = fragment {
        result.push_str(frag);
    }
    result
}

/// Rewrite a link that points to a file that has moved.
///
/// `href`       — the original href in the containing file
/// `src_dir`    — the directory of the *containing* file (which hasn't moved)
/// `new_target` — the new absolute, normalized path of the target file
pub fn retarget_link(href: &str, src_dir: &Path, new_target: &Path) -> String {
    let (_, fragment) = split_fragment(href);

    let new_rel = make_relative(src_dir, new_target);
    let mut result = new_rel.to_string_lossy().into_owned();
    result = result.replace('\\', "/");

    if !result.starts_with("..") && !result.starts_with('/') && !result.starts_with("./") {
        result = format!("./{result}");
    }

    if let Some(frag) = fragment {
        result.push_str(frag);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        let p = Path::new("/a/b/../c/./d");
        assert_eq!(normalize_path(p), PathBuf::from("/a/c/d"));
    }

    #[test]
    fn test_make_relative_sibling() {
        let from = Path::new("/project/docs");
        let to = Path::new("/project/archive/old.md");
        assert_eq!(make_relative(from, to), PathBuf::from("../archive/old.md"));
    }

    #[test]
    fn test_make_relative_child() {
        let from = Path::new("/project");
        let to = Path::new("/project/docs/guide.md");
        assert_eq!(make_relative(from, to), PathBuf::from("docs/guide.md"));
    }

    #[test]
    fn test_resolve_href_relative() {
        let dir = Path::new("/project/docs");
        let resolved = resolve_href("../README.md", dir);
        assert_eq!(resolved, Some(PathBuf::from("/project/README.md")));
    }

    #[test]
    fn test_rebase_link() {
        // File moves from /project/src to /project/archive
        // Link originally pointed to ../docs/guide.md
        let old_dir = Path::new("/project/src");
        let new_dir = Path::new("/project/archive");
        let result = rebase_link("../docs/guide.md", old_dir, new_dir);
        assert_eq!(result, "../docs/guide.md");
    }

    #[test]
    fn test_retarget_link() {
        // File at /project/README.md links to docs/old.md
        // old.md moves to /project/archive/old.md
        let src_dir = Path::new("/project");
        let new_target = Path::new("/project/archive/old.md");
        let result = retarget_link("docs/old.md", src_dir, new_target);
        assert_eq!(result, "./archive/old.md");
    }

    #[test]
    fn test_fragment_preserved() {
        let src_dir = Path::new("/project");
        let new_target = Path::new("/project/archive/guide.md");
        let result = retarget_link("docs/guide.md#section", src_dir, new_target);
        assert_eq!(result, "./archive/guide.md#section");
    }
}
