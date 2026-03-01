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
                if !out.pop() {
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
/// The *target* of the link hasn't changed; only the containing file moved.
/// Preserves the original `./` style: if the original href didn't start with
/// `./`, the result won't either.
pub fn rebase_link(href: &str, old_src_dir: &Path, new_src_dir: &Path) -> String {
    let (path_part, fragment) = split_fragment(href);

    let Some(abs_target) = resolve_href(path_part, old_src_dir) else {
        return href.to_owned();
    };

    let new_rel = make_relative(new_src_dir, &abs_target);
    let mut result = new_rel.to_string_lossy().replace('\\', "/");

    result = apply_dot_slash_style(result, path_part);

    if let Some(frag) = fragment {
        result.push_str(frag);
    }
    result
}

/// Rewrite a link that points to a file that has moved.
///
/// The containing file hasn't moved; only its target did.
/// Preserves the original `./` style of the href.
pub fn retarget_link(href: &str, src_dir: &Path, new_target: &Path) -> String {
    let (path_part, fragment) = split_fragment(href);

    let new_rel = make_relative(src_dir, new_target);
    let mut result = new_rel.to_string_lossy().replace('\\', "/");

    result = apply_dot_slash_style(result, path_part);

    if let Some(frag) = fragment {
        result.push_str(frag);
    }
    result
}

/// Apply the `./` prefix convention from `original` to `result`:
/// - If original started with `./`, ensure result does too (for bare filenames).
/// - If original didn't start with `./`, don't add it.
/// - Paths starting with `..` or `/` are never touched.
fn apply_dot_slash_style(result: String, original: &str) -> String {
    if result.starts_with("..") || result.starts_with('/') || result.starts_with("./") {
        return result;
    }
    if original.starts_with("./") {
        format!("./{result}")
    } else {
        result
    }
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
    fn test_rebase_link_unchanged() {
        // File moves from /project/src to /project/archive (siblings of docs/)
        // Link to ../docs/guide.md resolves to the same relative path from archive/
        let old_dir = Path::new("/project/src");
        let new_dir = Path::new("/project/archive");
        let result = rebase_link("../docs/guide.md", old_dir, new_dir);
        assert_eq!(result, "../docs/guide.md");
    }

    #[test]
    fn test_rebase_link_changes() {
        // File moves from /project/docs to /project/a/b
        // Link to ../README.md (= /project/README.md) must now be ../../README.md
        let old_dir = Path::new("/project/docs");
        let new_dir = Path::new("/project/a/b");
        let result = rebase_link("../README.md", old_dir, new_dir);
        assert_eq!(result, "../../README.md");
    }

    #[test]
    fn test_retarget_link_bare_style() {
        // Original link is bare (no ./); result must also be bare
        let src_dir = Path::new("/project");
        let new_target = Path::new("/project/archive/old.md");
        let result = retarget_link("docs/old.md", src_dir, new_target);
        assert_eq!(result, "archive/old.md");
    }

    #[test]
    fn test_retarget_link_dot_slash_style() {
        // Original link has ./; result must also have ./
        let src_dir = Path::new("/project");
        let new_target = Path::new("/project/archive/old.md");
        let result = retarget_link("./docs/old.md", src_dir, new_target);
        assert_eq!(result, "./archive/old.md");
    }

    #[test]
    fn test_fragment_preserved_bare() {
        let src_dir = Path::new("/project");
        let new_target = Path::new("/project/archive/guide.md");
        let result = retarget_link("docs/guide.md#section", src_dir, new_target);
        assert_eq!(result, "archive/guide.md#section");
    }

    #[test]
    fn test_fragment_preserved_dot_slash() {
        let src_dir = Path::new("/project");
        let new_target = Path::new("/project/archive/guide.md");
        let result = retarget_link("./docs/guide.md#section", src_dir, new_target);
        assert_eq!(result, "./archive/guide.md#section");
    }

    #[test]
    fn test_parent_relative_untouched() {
        // Results starting with `..` are never dot-slash prefixed
        let src_dir = Path::new("/project/docs");
        let new_target = Path::new("/project/README.md");
        let result = retarget_link("./README.md", src_dir, new_target);
        assert_eq!(result, "../README.md");
    }
}
