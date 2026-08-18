#!/usr/bin/env bash
# Cross-path SIMD benchmark matrix for CI / local use.
#
# Usage:
#   ./scripts/simd_bench_matrix.sh
#   ./scripts/simd_bench_matrix.sh 1280 720 8
#
# With portable-simd (nightly):
#   CARGO="cargo +nightly" FEATURES="--features portable-simd" ./scripts/simd_bench_matrix.sh
#
# Writes markdown to SIMD_BENCH_MD (default: simd-bench.md) and, when set,
# appends the same content to $GITHUB_STEP_SUMMARY.

set -euo pipefail

WIDTH="${1:-1920}"
HEIGHT="${2:-1080}"
ITERS="${3:-12}"
CARGO="${CARGO:-cargo}"
FEATURES="${FEATURES:-}"
OUT_MD="${SIMD_BENCH_MD:-simd-bench.md}"
ARTIFACT_DIR="${SIMD_BENCH_DIR:-simd-bench-out}"

mkdir -p "$ARTIFACT_DIR"

have_cmd() { command -v "$1" >/dev/null 2>&1; }

detect_cpu() {
  echo "## Host CPU"
  echo
  echo "- date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- uname: $(uname -srm)"
  if [[ -f /proc/cpuinfo ]]; then
    local model
    model=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ //' || true)
    echo "- model: ${model:-unknown}"
    local flags
    flags=$(grep -m1 '^flags' /proc/cpuinfo | cut -d: -f2- || true)
    for f in sse2 ssse3 sse4_1 sse4_2 avx avx2 bmi1 bmi2 avx512f avx512bw avx512vl neon asimd; do
      if grep -qw "$f" <<<"$flags" || grep -qw "$f" /proc/cpuinfo 2>/dev/null; then
        echo "- feature \`$f\`: yes"
      else
        echo "- feature \`$f\`: no"
      fi
    done
  elif have_cmd sysctl; then
    sysctl -n machdep.cpu.brand_string 2>/dev/null | sed 's/^/- brand: /' || true
    for k in hw.optional.avx2_0 hw.optional.avx512f hw.optional.sse4_2 hw.optional.AdvSIMD; do
      local v
      v=$(sysctl -n "$k" 2>/dev/null || echo "n/a")
      echo "- \`$k\`: $v"
    done
  fi
  echo
}

# Paths to try: name|dct|color
# Arch-specific entries are skipped when the binary reports they are unavailable
# by checking a tiny probe via simd_report output / env.
candidate_paths() {
  echo "auto|auto|auto"
  echo "scalar|scalar|scalar"
  if [[ -n "$FEATURES" ]]; then
    echo "portable|portable|portable"
  fi
  case "$(uname -m)" in
    x86_64|amd64)
      echo "sse128|sse128|sse2"
      echo "avx2|avx2|avx2"
      ;;
    aarch64|arm64)
      echo "neon|neon|neon"
      ;;
  esac
}

run_one() {
  local label="$1" dct="$2" color="$3"
  local log="$ARTIFACT_DIR/${label}.log"
  local args=("$WIDTH" "$HEIGHT" "$ITERS")
  if [[ "$dct" != "auto" ]]; then
    args+=("$dct" "$color")
  fi

  echo "### path=$label (dct=$dct color=$color)"
  echo
  echo '```'
  # shellcheck disable=SC2086
  if ! $CARGO run --release $FEATURES --example simd_report -- "${args[@]}" 2>&1 | tee "$log"; then
    echo "(run failed — path may be unavailable on this host)"
  fi
  echo '```'
  echo

  # Pull key medians into a one-line summary if present.
  local enc dec
  enc=$(grep -E '^encode_uyvy:' "$log" | head -1 || true)
  dec=$(grep -E '^load_from\+decode_bgra:' "$log" | head -1 || true)
  if [[ -n "$enc$dec" ]]; then
    echo "| \`$label\` | ${enc#encode_uyvy: } | ${dec#load_from+decode_bgra: } |" >>"$ARTIFACT_DIR/summary_rows.md"
  fi
}

{
  echo "# vmx-rs SIMD path benchmark"
  echo
  detect_cpu
  echo "## Toolchain"
  echo
  echo '```'
  $CARGO --version
  if [[ "$CARGO" == *"+nightly"* ]] || [[ "$CARGO" == *"nightly"* ]]; then
    rustup run nightly rustc --version || true
  else
    rustc --version || true
  fi
  echo "CARGO=$CARGO"
  echo "FEATURES=$FEATURES"
  echo "size=${WIDTH}x${HEIGHT} iters=$ITERS"
  echo '```'
  echo
  echo "## Notes"
  echo
  echo "- Hosted GitHub runners typically expose **AVX2** on \`ubuntu-latest\` and **NEON** on \`macos-latest\` / \`ubuntu-*-arm\`."
  echo "- **AVX-512** is *not* guaranteed on GitHub-hosted runners; this repo also has no AVX-512 kernels yet (detection only)."
  echo "- \`portable\` requires nightly + \`--features portable-simd\`."
  echo "- Absolute ms/frame numbers are noisy across shared CI VMs; compare paths **within the same job**."
  echo
  echo "## Runs"
  echo
} >"$OUT_MD"

: >"$ARTIFACT_DIR/summary_rows.md"

while IFS='|' read -r label dct color; do
  {
    run_one "$label" "$dct" "$color"
  } >>"$OUT_MD"
done < <(candidate_paths)

{
  echo "## Summary (encode_uyvy / decode_bgra medians)"
  echo
  echo "| path | encode_uyvy | decode_bgra |"
  echo "|------|-------------|-------------|"
  cat "$ARTIFACT_DIR/summary_rows.md"
  echo
} >>"$OUT_MD"

cp "$OUT_MD" "$ARTIFACT_DIR/simd-bench.md"

if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  cat "$OUT_MD" >>"$GITHUB_STEP_SUMMARY"
fi

echo "Wrote $OUT_MD (and $ARTIFACT_DIR/)"
