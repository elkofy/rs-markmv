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
fn locate_href_in_slice(raw_slice: &str, href: &str, slice_start: usize) -> Option<(usize, usize)> {
    // For inline links: `[text](href)` or `![alt](href)`
    // The href appears after the first `(` that follows `]`
    if let Some(paren_pos) = find_link_paren(raw_slice) {
        let after_paren = &raw_slice[paren_pos + 1..];
        // The href may have a title: `[text](href "title")`, so find
        // href as the content before any space+quote or closing paren.
        let href_in_src = extract_href_from_paren_content(after_paren);
        if href_in_src == href || href_in_src.starts_with(href) {
            let rel_start = paren_pos + 1;
            let abs_start = slice_start + rel_start;
            let abs_end = abs_start + href.len();
            return Some((abs_start, abs_end));
        }
    }

    // Fallback: plain substring search
    if let Some(pos) = raw_slice.find(href) {
        return Some((slice_start + pos, slice_start + pos + href.len()));
    }

    None
}

/// Find the position of `(` that opens the href in an inline link.
/// Handles `[text](href)` and `![alt](href)`.
fn find_link_paren(raw_slice: &str) -> Option<usize> {
    // Find the closing `]` then the immediately following `(`
    let bytes = raw_slice.as_bytes();
    let mut depth = 1i32;
    let mut i = 0;
    // skip leading `!` for images
    if bytes.first() == Some(&b'!') {
        i += 1;
    }
    // skip `[`
    if bytes.get(i) == Some(&b'[') {
        i += 1;
    } else {
        return None;
    }
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    // Now `i` points just past the closing `]`
    if bytes.get(i) == Some(&b'(') {
        Some(i)
    } else {
        None
    }
}

/// Extract the href portion from the content inside `(...)`.
/// Handles optional `<angle-bracket>` form and optional trailing title.
fn extract_href_from_paren_content(s: &str) -> &str {
    let s = s.trim_start();
    if s.starts_with('<') {
        // Angle-bracket form: `<href>`
        if let Some(end) = s.find('>') {
            return &s[1..end];
        }
    }
    // Plain form — terminate at whitespace or `)`
    let end = s
        .find(|c: char| c == ')' || c.is_ascii_whitespace())
        .unwrap_or(s.len());
    &s[..end]
}

/// Parse `[id]: href` reference definitions using a line-by-line scan.
fn parse_ref_definitions(content: &str, file_dir: &Path) -> Vec<Link> {
    let mut links = Vec::new();
    let mut byte_pos = 0usize;

    for line in content.split('\n') {
        let line_with_nl = &content[byte_pos..byte_pos + line.len()];
        // Reference definition pattern: optional leading spaces, [id]: href
        let trimmed = line_with_nl.trim_start();
        if trimmed.starts_with('[') {
            if let Some(colon_bracket) = trimmed.find("]:") {
                let after_colon = trimmed[colon_bracket + 2..].trim_start();
                // href is the first token (ends at whitespace or end-of-line)
                let href_end = after_colon
                    .find(|c: char| c.is_ascii_whitespace())
                    .unwrap_or(after_colon.len());
                let raw_href = &after_colon[..href_end];

                if !raw_href.is_empty() && !is_external(raw_href) && !raw_href.starts_with('#') {
                    // Absolute position of this href in `content`
                    let leading_spaces = line_with_nl.len() - trimmed.len();
                    let href_offset_in_line = leading_spaces
                        + colon_bracket
                        + 2
                        + (after_colon.len() - after_colon.trim_start().len());
                    let abs_start = byte_pos + href_offset_in_line;
                    let abs_end = abs_start + raw_href.len();

                    // Verify we actually point to the right bytes
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
        // +1 for the '\n' that split removed
        byte_pos += line.len() + 1;
    }

    links
}

/// Returns `true` if `href` is an absolute URL (http/https/ftp/mailto/etc.)
pub fn is_external(href: &str) -> bool {
    // A colon before any `/` indicates a scheme
    if let Some(colon) = href.find(':') {
        let before = &href[..colon];
        // scheme chars are ASCII alpha + digit + '+' + '-' + '.'
        before.chars().all(|c| c.is_ascii_alphanumeric() || "+-." .contains(c))
    } else {
        false
    }
}

/// Split `path#fragment` into `(path, Some(fragment))` or `(path, None)`.
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
}
