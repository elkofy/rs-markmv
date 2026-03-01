#!/usr/bin/env bash
# Generate a synthetic markdown project for benchmarking.
# Creates N_FILES files across a directory tree with cross-links.
#
# Usage: ./generate_fixture.sh <output_dir> [N_FILES]

set -euo pipefail

OUT="${1:?Usage: $0 <output_dir> [N_FILES]}"
N="${2:-100}"

rm -rf "$OUT"
mkdir -p "$OUT"/{docs,guides,archive,src,reference}

DIRS=(docs guides archive src reference)

echo "Generating $N markdown files in $OUT ..."

for i in $(seq 1 "$N"); do
  dir="${DIRS[$((i % ${#DIRS[@]}))]}"
  file="$OUT/$dir/file_$i.md"

  # Pick 3 random "other" files to link to (by index, may not exist yet — that's fine)
  link_a=$(( (i * 7 + 3) % N + 1 ))
  link_b=$(( (i * 13 + 7) % N + 1 ))
  link_c=$(( (i * 17 + 11) % N + 1 ))

  dir_a="${DIRS[$((link_a % ${#DIRS[@]}))]}"
  dir_b="${DIRS[$((link_b % ${#DIRS[@]}))]}"
  dir_c="${DIRS[$((link_c % ${#DIRS[@]}))]}"

  # Compute relative paths from this file's dir to the targets
  rel_a=$(python3 -c "import os.path; print(os.path.relpath('$OUT/$dir_a/file_$link_a.md', '$OUT/$dir'))")
  rel_b=$(python3 -c "import os.path; print(os.path.relpath('$OUT/$dir_b/file_$link_b.md', '$OUT/$dir'))")
  rel_c=$(python3 -c "import os.path; print(os.path.relpath('$OUT/$dir_c/file_$link_c.md', '$OUT/$dir'))")

  cat > "$file" <<EOF
# Document $i

This is document number $i in the $dir section.

## References

- See [document $link_a]($rel_a) for more information.
- Also check [document $link_b]($rel_b) for related content.
- And [document $link_c]($rel_c) covers another aspect.

## Content

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor
incididunt ut labore et dolore magna aliqua.

[Back to index](../README.md)
EOF
done

# Generate a root index
{
  echo "# Index"
  echo ""
  for i in $(seq 1 "$N"); do
    dir="${DIRS[$((i % ${#DIRS[@]}))]}"
    echo "- [Document $i]($dir/file_$i.md)"
  done
} > "$OUT/README.md"

echo "Done: $(find "$OUT" -name '*.md' | wc -l) files created."
