# rs-markmv

A fast Rust CLI for moving markdown files while automatically rewriting all affected links — a drop-in alternative to the TypeScript [`markmv`](https://www.npmjs.com/package/markmv) tool.

## Features

- Move one or more `.md` files and rewrite every link that breaks
- Updates links **inside** the moved file (relative paths shift when the file moves)
- Updates links in **other files** that point to the moved file
- Handles multiple moves atomically — if A links to B and both move, A's link to B resolves to B's new location
- Supports inline links, images, and reference-style link definitions
- `--dry-run` mode to preview changes without touching anything

## Installation

```bash
cargo install markmv
```

Or from source:

```bash
git clone https://github.com/elkofy/rs-markmv
cd rs-markmv
cargo install --path .
```

## Usage

```
markmv move [OPTIONS] <SOURCE>... <DESTINATION>

Arguments:
  <SOURCE>...    One or more source .md files
  <DESTINATION>  Destination file or directory

Options:
  -n, --dry-run        Print changes without writing files
  -v, --verbose        Show each changed link
      --root <DIR>     Root directory to scan for .md files [default: cwd]
  -h, --help           Print help
```

### Examples

```bash
# Preview moving a single file
markmv move docs/guide.md archive/guide.md --dry-run

# Move with verbose link output
markmv move docs/guide.md archive/guide.md --verbose

# Move multiple files into a directory
markmv move src/*.md archive/

# Specify a different root for link scanning
markmv move notes/old.md notes/new.md --root ~/my-wiki
```

### Sample output

```
Moving: docs/old.md → archive/old.md

  Updated README.md:
    docs/old.md → ./archive/old.md

  Updated src/main.md:
    ../docs/old.md → ../archive/old.md

  Updated docs/old.md (self):
    ./images/fig.png → ../docs/images/fig.png

1 file(s) moved, 3 link(s) updated
```

## Benchmark vs TypeScript markmv

Dry-run move of 10 files across a cross-linked markdown project (5 runs, release build):

| Project size | rs-markmv (Rust) | markmv (npm) | Speedup |
|:---:|:---:|:---:|:---:|
| 100 files  | 2 ms  | 126 ms | **~63x** |
| 500 files  | 7 ms  | 176 ms | **~25x** |
| 1000 files | 12 ms | 241 ms | **~20x** |

The TypeScript version pays a fixed ~120 ms Node.js startup cost on every invocation. The Rust binary starts in under 1 ms and scales linearly with file count.

Run the benchmark yourself:

```bash
bash bench/bench.sh 500 5
```

## How it works

1. **Collect** all `.md` files under `--root` using `walkdir`
2. **Parse** links in every file using `pulldown-cmark`'s offset iterator to get exact byte positions
3. **Compute** two kinds of changes per move:
   - *self-changes*: links inside the moved file whose relative paths shift because the file itself moved
   - *other-changes*: links in other files that pointed to the old location and must now point to the new one
4. **Apply** replacements in reverse byte-offset order (preserves offsets for earlier replacements)
5. **Rename** source files to destinations

## Link types handled

| Type | Example |
|---|---|
| Inline link | `[text](path.md)` |
| Image | `![alt](img.png)` |
| Reference definition | `[id]: path.md` |
| Fragment | `guide.md#section` (fragment preserved) |
| External URL | skipped |
| Anchor-only | `#heading` — skipped |

## Dependencies

- [`clap`](https://crates.io/crates/clap) — CLI argument parsing
- [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark) — markdown parsing with byte offsets
- [`walkdir`](https://crates.io/crates/walkdir) — recursive directory traversal
- [`anyhow`](https://crates.io/crates/anyhow) — error handling
