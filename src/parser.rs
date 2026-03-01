use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Options, Parser, Tag};

#[derive(Debug, Clone)]
pub struct Link {
    /// Raw href as written in the markdown source
    pub href: String,
    /// Byte offset in `content` where the href string starts
    pub start: usize,
    /// Byte offset in `content` where the href string ends (exclusive)
    pub end: usize,
    /// Absolute, normalized path (None for external URLs or anchor-only links)
    pub resolved: Option<PathBuf>,
}

/// Parse all markdown links and images from `content`, resolving relative
/// hrefs against the directory containing `file_path`.
pub fn parse_links(content: &str, file_path: &Path) -> Vec<Link> {
    let file_dir = file_path.parent().unwrap_or(Path::new("."));
    let mut links = Vec::new();

    let opts = Options::all();
    let parser = Parser::new_ext(content, opts).into_offset_iter();

    for (event, range) in parser {
        let raw_href = match &event {
            Event::Start(Tag::Link { dest_url, .. }) => dest_url.as_ref().to_owned(),
            Event::Start(Tag::Image { dest_url, .. }) => dest_url.as_ref().to_owned(),
            _ => continue,
        };

        // Skip external URLs and anchor-only links
        if is_external(&raw_href) || raw_href.starts_with('#') {
            continue;
        }

        // The raw range covers the entire `[text](url)` or `![alt](url)` span.
        // We need to find where the href sits inside this raw slice.
        let raw_slice = &content[range.clone()];
        if let Some((start, end)) = locate_href_in_slice(raw_slice, &raw_href, range.start) {
            let (href_no_frag, _frag) = split_fragment(&raw_href);
            let resolved = crate::refactor::resolve_href(href_no_frag, file_dir);
            links.push(Link {
                href: raw_href,
                start,
                end,
                resolved,
            });
        }
    }

    // Also collect reference-style link definitions: `[id]: path.md`
    links.extend(parse_ref_definitions(content, file_dir));

    // Sort by start offset for predictable ordering
    links.sort_by_key(|l| l.start);
    links
}

/// Locate the byte range of `href` inside `raw_slice` (which starts at
/// `slice_start` in the full document).  Returns `(abs_start, abs_end)`.
///
/// We first find the `](` boundary. Everything before it is link text — we
/// never search there, which prevents accidentally matching an href that also
/// appears verbatim as the link label (e.g. `[guide.md](guide.md)`).
fn locate_href_in_slice(raw_slice: &str, href: &str, slice_start: usize) -> Option<(usize, usize)> {
    // Find the `](` that separates label from href
    let paren_pos = find_link_paren(raw_slice)?;

    // Search only within the paren content — safe, can't hit link text
    let after_paren = &raw_slice[paren_pos + 1..];
    let pos = after_paren.find(href)?;
    let abs_start = slice_start + paren_pos + 1 + pos;
    Some((abs_start, abs_start + href.len()))
}

/// Find the position of `(` that opens the href in an inline link.
/// Returns the index within `raw_slice` of that `(`.
/// Returns `None` for reference-style links (`[text][ref]`) which have no `](`.
fn find_link_paren(raw_slice: &str) -> Option<usize> {
    let bytes = raw_slice.as_bytes();
    let mut depth = 1i32;
    let mut i = 0;

    // Skip leading `!` for images
    if bytes.first() == Some(&b'!') {
        i += 1;
    }
    // Expect opening `[`
    if bytes.get(i) == Some(&b'[') {
        i += 1;
    } else {
        return None;
    }
    // Walk until the matching `]`
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    // `i` now points just past the closing `]`; expect `(`
    if bytes.get(i) == Some(&b'(') {
        Some(i)
    } else {
        None
    }
}

/// Parse `[id]: href` reference definitions using a line-by-line scan.
/// pulldown-cmark does not emit offset events for these, so we handle them
/// separately.
fn parse_ref_definitions(content: &str, file_dir: &Path) -> Vec<Link> {
    let mut links = Vec::new();
    let mut byte_pos = 0usize;

    for line in content.split('\n') {
        let line_bytes = line.len();
        let trimmed = line.trim_start();

        if trimmed.starts_with('[') {
            if let Some(colon_bracket) = trimmed.find("]:") {
                let after_colon = trimmed[colon_bracket + 2..].trim_start();
                // href is the first whitespace-delimited token
                let href_end = after_colon
                    .find(|c: char| c.is_ascii_whitespace())
                    .unwrap_or(after_colon.len());
                let raw_href = &after_colon[..href_end];

                if !raw_href.is_empty() && !is_external(raw_href) && !raw_href.starts_with('#') {
                    let leading_spaces = line.len() - trimmed.len();
                    let after_colon_full = &trimmed[colon_bracket + 2..];
                    let ws_before_href = after_colon_full.len() - after_colon_full.trim_start().len();
                    let href_offset_in_line =
                        leading_spaces + colon_bracket + 2 + ws_before_href;
                    let abs_start = byte_pos + href_offset_in_line;
                    let abs_end = abs_start + raw_href.len();

                    // Sanity-check: confirm we're pointing at the right bytes
                    if content.get(abs_start..abs_end) == Some(raw_href) {
                        let (href_no_frag, _frag) = split_fragment(raw_href);
                        let resolved = crate::refactor::resolve_href(href_no_frag, file_dir);
                        links.push(Link {
                            href: raw_href.to_owned(),
                            start: abs_start,
                            end: abs_end,
                            resolved,
                        });
                    }
                }
            }
        }

        // +1 for the '\n' consumed by split
        byte_pos += line_bytes + 1;
    }

    links
}

/// Returns `true` if `href` is an absolute URL (http, https, ftp, mailto, …).
pub fn is_external(href: &str) -> bool {
    if let Some(colon) = href.find(':') {
        let scheme = &href[..colon];
        // RFC 3986 scheme: starts with alpha, then alpha / digit / '+' / '-' / '.'
        scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-." .contains(c))
    } else {
        false
    }
}

/// Split `path#fragment` into `(path, Some("#fragment"))` or `(path, None)`.
pub fn split_fragment(href: &str) -> (&str, Option<&str>) {
    if let Some(hash) = href.find('#') {
        (&href[..hash], Some(&href[hash..]))
    } else {
        (href, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_inline_links() {
        let content = "See [guide](../docs/guide.md) for details.";
        let file = Path::new("/project/src/README.md");
        let links = parse_links(content, file);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].href, "../docs/guide.md");
        assert_eq!(&content[links[0].start..links[0].end], "../docs/guide.md");
    }

    #[test]
    fn test_image_links() {
        let content = "![diagram](images/fig.png)";
        let links = parse_links(content, Path::new("/project/README.md"));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].href, "images/fig.png");
    }

    #[test]
    fn test_external_skipped() {
        let content = "[example](https://example.com)";
        let links = parse_links(content, Path::new("/README.md"));
        assert!(links.is_empty());
    }

    #[test]
    fn test_anchor_skipped() {
        let content = "[section](#heading)";
        let links = parse_links(content, Path::new("/README.md"));
        assert!(links.is_empty());
    }

    #[test]
    fn test_fragment_preserved() {
        let (_path, frag) = split_fragment("guide.md#section");
        assert_eq!(frag, Some("#section"));
    }

    /// Regression: href that also appears as link text must not corrupt the label.
    #[test]
    fn test_href_same_as_link_text_not_corrupted() {
        let content = "[guide.md](guide.md)";
        let links = parse_links(content, Path::new("/project/README.md"));
        assert_eq!(links.len(), 1);
        // Must point to the href position, not the label
        assert_eq!(&content[links[0].start..links[0].end], "guide.md");
        // `[guide.md](guide.md)`: `(` is at byte 10, href starts at byte 11
        assert_eq!(links[0].start, 11);
    }

    /// Link with title: `[text](href "title")` — href position must be correct.
    #[test]
    fn test_link_with_title() {
        let content = r#"[text](guide.md "the title")"#;
        let links = parse_links(content, Path::new("/project/README.md"));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].href, "guide.md");
        assert_eq!(&content[links[0].start..links[0].end], "guide.md");
    }

    #[test]
    fn test_ref_definition() {
        let content = "[guide]: docs/guide.md\n\n[See guide][guide]\n";
        let links = parse_links(content, Path::new("/project/README.md"));
        // Only the definition should be collected (not the usage, which has no inline href)
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].href, "docs/guide.md");
        assert_eq!(&content[links[0].start..links[0].end], "docs/guide.md");
    }
}
