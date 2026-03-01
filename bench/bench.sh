#!/usr/bin/env bash
# Benchmark rs-markmv (Rust) vs markmv (TypeScript) on a dry-run move
# of N files in a cross-linked markdown project.
#
# Usage: ./bench.sh [N_FILES] [REPEATS]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"

N="${1:-100}"
REPEATS="${2:-5}"

RS_BIN="$REPO_DIR/target/release/markmv"
TS_BIN="node /home/ndaikh/.nvm/versions/node/v24.0.2/bin/markmv"

FIXTURE="/tmp/markmv-bench-$N"

echo "========================================"
echo " markmv benchmark"
echo "========================================"
echo " Files  : $N markdown files"
echo " Repeats: $REPEATS"
echo " Fixture: $FIXTURE"
echo ""

# --- Generate fixture ---
bash "$SCRIPT_DIR/generate_fixture.sh" "$FIXTURE" "$N" 2>&1

# Pick files to move: first 10 from docs/
SOURCES=()
for f in "$FIXTURE"/docs/file_*.md; do
  SOURCES+=("$f")
  [[ ${#SOURCES[@]} -ge 10 ]] && break
done
DEST="$FIXTURE/archive/"
mkdir -p "$DEST"

echo ""
echo "Moving ${#SOURCES[@]} files: docs/file_*.md → archive/"
echo ""

# --- Helper: time N runs, print min/avg/max in ms ---
run_bench() {
  local label="$1"
  shift
  local cmd=("$@")

  echo "--- $label ---"
  local times=()
  for i in $(seq 1 "$REPEATS"); do
    local start end elapsed
    start=$(date +%s%N)
    "${cmd[@]}" > /dev/null 2>&1
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    times+=("$elapsed")
    printf "  run %d: %d ms\n" "$i" "$elapsed"
  done

  # Sort and compute stats with python3
  python3 - "${times[@]}" <<'PYEOF'
import sys
vals = list(map(int, sys.argv[1:]))
vals.sort()
print(f"  min={vals[0]}ms  avg={sum(vals)//len(vals)}ms  max={vals[-1]}ms")
PYEOF
  echo ""
}

# --- Rust ---
run_bench "Rust  (rs-markmv, release)" \
  "$RS_BIN" move "${SOURCES[@]}" "$DEST" --dry-run --root "$FIXTURE"

# --- TypeScript ---
run_bench "TypeScript (markmv npm)" \
  $TS_BIN move "${SOURCES[@]}" "$DEST" --dry-run

echo "========================================"
echo " Rust binary : $RS_BIN"
echo " TS   binary : $(which node) + markmv"
echo "========================================"
