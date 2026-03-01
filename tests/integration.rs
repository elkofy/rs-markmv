use std::{fs, path::PathBuf};

use markmv::ops::move_files;

/// Create a fresh temp directory for a test, removing any leftover from
/// a previous run.
fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("markmv-it-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// Basic link update in another file
// ---------------------------------------------------------------------------

#[test]
fn other_file_link_updated() {
    let root = test_dir("other-link");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("index.md"), "[guide](docs/guide.md)\n").unwrap();
    fs::write(root.join("docs/guide.md"), "# Guide\n").unwrap();

    move_files(
        &[(root.join("docs/guide.md"), root.join("archive/guide.md"))],
        &root,
        false,
    )
    .unwrap();

    let index = fs::read_to_string(root.join("index.md")).unwrap();
    assert!(
        index.contains("archive/guide.md"),
        "expected updated link, got: {index}"
    );
    assert!(!index.contains("docs/guide.md"), "old link still present: {index}");
    assert!(root.join("archive/guide.md").exists());
    assert!(!root.join("docs/guide.md").exists());
}

// ---------------------------------------------------------------------------
// Bare vs ./ link style preserved
// ---------------------------------------------------------------------------

#[test]
fn bare_link_style_preserved() {
    let root = test_dir("bare-style");
    fs::write(root.join("index.md"), "[g](guide.md)\n").unwrap();
    fs::write(root.join("guide.md"), "# Guide\n").unwrap();

    move_files(
        &[(root.join("guide.md"), root.join("docs/guide.md"))],
        &root,
        false,
    )
    .unwrap();

    let index = fs::read_to_string(root.join("index.md")).unwrap();
    // Original had no "./", result must not add it
    assert!(
        index.contains("docs/guide.md") && !index.contains("./docs/guide.md"),
        "expected bare style, got: {index}"
    );
}

#[test]
fn dot_slash_style_preserved() {
    let root = test_dir("dot-slash-style");
    fs::write(root.join("index.md"), "[g](./guide.md)\n").unwrap();
    fs::write(root.join("guide.md"), "# Guide\n").unwrap();

    move_files(
        &[(root.join("guide.md"), root.join("docs/guide.md"))],
        &root,
        false,
    )
    .unwrap();

    let index = fs::read_to_string(root.join("index.md")).unwrap();
    assert!(
        index.contains("./docs/guide.md"),
        "expected ./ style preserved, got: {index}"
    );
}

// ---------------------------------------------------------------------------
// Dry-run makes no changes
// ---------------------------------------------------------------------------

#[test]
fn dry_run_no_changes() {
    let root = test_dir("dry-run");
    fs::write(root.join("index.md"), "[guide](guide.md)\n").unwrap();
    fs::write(root.join("guide.md"), "# Guide\n").unwrap();

    let src = root.join("guide.md");
    let dst = root.join("sub/guide.md");
    move_files(&[(src.clone(), dst.clone())], &root, true).unwrap();

    assert!(src.exists(), "source must still exist after dry-run");
    assert!(!dst.exists(), "destination must not be created in dry-run");
    assert_eq!(
        fs::read_to_string(root.join("index.md")).unwrap(),
        "[guide](guide.md)\n",
        "index.md must be unchanged after dry-run"
    );
}

// ---------------------------------------------------------------------------
// Self-link rebasing when a file moves
// ---------------------------------------------------------------------------

#[test]
fn self_links_rebased() {
    let root = test_dir("self-rebase");
    fs::create_dir_all(root.join("docs")).unwrap();
    // guide.md links to ../other.md (= /root/other.md)
    fs::write(root.join("docs/guide.md"), "[other](../other.md)\n").unwrap();
    fs::write(root.join("other.md"), "# Other\n").unwrap();

    // Move guide.md two levels deeper
    move_files(
        &[(root.join("docs/guide.md"), root.join("a/b/guide.md"))],
        &root,
        false,
    )
    .unwrap();

    let content = fs::read_to_string(root.join("a/b/guide.md")).unwrap();
    // From a/b/, other.md is two levels up
    assert!(
        content.contains("../../other.md"),
        "expected rebased self-link, got: {content}"
    );
}

// ---------------------------------------------------------------------------
// Both files moving — A links to B, both move together
// ---------------------------------------------------------------------------

#[test]
fn both_files_moving() {
    let root = test_dir("both-moving");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.md"), "[b](b.md)\n").unwrap();
    fs::write(root.join("src/b.md"), "# B\n").unwrap();

    move_files(
        &[
            (root.join("src/a.md"), root.join("out/a.md")),
            (root.join("src/b.md"), root.join("out/b.md")),
        ],
        &root,
        false,
    )
    .unwrap();

    let content = fs::read_to_string(root.join("out/a.md")).unwrap();
    // a.md and b.md are siblings in out/ → link stays as b.md (no change needed)
    assert!(
        content.contains("b.md"),
        "expected sibling link to b.md, got: {content}"
    );
}

// ---------------------------------------------------------------------------
// Fragment preserved after rewrite
// ---------------------------------------------------------------------------

#[test]
fn fragment_preserved_after_rewrite() {
    let root = test_dir("fragment");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("index.md"), "[sec](docs/guide.md#section)\n").unwrap();
    fs::write(root.join("docs/guide.md"), "# Guide\n").unwrap();

    move_files(
        &[(root.join("docs/guide.md"), root.join("archive/guide.md"))],
        &root,
        false,
    )
    .unwrap();

    let index = fs::read_to_string(root.join("index.md")).unwrap();
    assert!(
        index.contains("archive/guide.md#section"),
        "expected fragment preserved, got: {index}"
    );
}

// ---------------------------------------------------------------------------
// Missing source file returns an error
// ---------------------------------------------------------------------------

#[test]
fn missing_source_errors() {
    let root = test_dir("missing-src");
    let src = root.join("nonexistent.md");
    let dst = root.join("out.md");
    let result = move_files(&[(src, dst)], &root, false);
    assert!(result.is_err(), "expected error for missing source");
}

// ---------------------------------------------------------------------------
// Regression: href appearing in link text must not corrupt the label
// ---------------------------------------------------------------------------

#[test]
fn href_in_link_text_not_corrupted() {
    let root = test_dir("href-in-text");
    // The link text and href are identical — rewrite must only touch the href
    fs::write(root.join("index.md"), "[guide.md](guide.md)\n").unwrap();
    fs::write(root.join("guide.md"), "# Guide\n").unwrap();

    move_files(
        &[(root.join("guide.md"), root.join("docs/guide.md"))],
        &root,
        false,
    )
    .unwrap();

    let index = fs::read_to_string(root.join("index.md")).unwrap();
    // Label must remain "guide.md", href must be updated
    assert!(
        index.starts_with("[guide.md]("),
        "link text was corrupted: {index}"
    );
    assert!(
        index.contains("docs/guide.md"),
        "href was not updated: {index}"
    );
}
