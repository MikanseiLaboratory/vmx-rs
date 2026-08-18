#!/usr/bin/env bash
# Cross-path SIMD benchmark matrix — prints a clear timing table.
#
# Usage:
#   ./scripts/simd_bench_matrix.sh
#   ./scripts/simd_bench_matrix.sh 1920 1080 12
#   CARGO="cargo +nightly" FEATURES="--features portable-simd" ./scripts/simd_bench_matrix.sh

set -euo pipefail

WIDTH="${1:-1920}"
HEIGHT="${2:-1080}"
ITERS="${3:-12}"
CARGO="${CARGO:-cargo}"
FEATURES="${FEATURES:-}"
OUT_MD="${SIMD_BENCH_MD:-simd-bench.md}"
ARTIFACT_DIR="${SIMD_BENCH_DIR:-simd-bench-out}"

mkdir -p "$ARTIFACT_DIR"
SUMMARY_CSV="$ARTIFACT_DIR/timings.csv"
echo "path,encode_uyvy_ms,encode_bgra_ms,decode_uyvy_ms,decode_bgra_ms" >"$SUMMARY_CSV"

have_cmd() { command -v "$1" >/dev/null 2>&1; }

detect_cpu_md() {
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
    for f in sse2 ssse3 sse4_2 avx2 bmi2 avx512f avx512bw neon sve sve2; do
      if grep -qw "$f" <<<"$flags"; then echo "- \`$f\`: yes"; else echo "- \`$f\`: no"; fi
    done
    # getauxval-style Features line may use different names on some kernels
    if [[ -r /proc/cpuinfo ]]; then
      if grep -qi 'Features.*:.*\bsve\b' /proc/cpuinfo 2>/dev/null; then
        echo "- \`sve\` (Features): yes"
      fi
      if grep -qi 'Features.*:.*\bsve2\b' /proc/cpuinfo 2>/dev/null; then
        echo "- \`sve2\` (Features): yes"
      fi
    fi
  elif have_cmd sysctl; then
    sysctl -n machdep.cpu.brand_string 2>/dev/null | sed 's/^/- brand: /' || true
    for k in hw.optional.avx2_0 hw.optional.avx512f hw.optional.AdvSIMD; do
      echo "- \`$k\`: $(sysctl -n "$k" 2>/dev/null || echo n/a)"
    done
  fi
  echo
}

extract_ms() {
  # $1=log $2=label prefix e.g. encode_uyvy
  local line
  line=$(grep -E "^$2:" "$1" | head -1 || true)
  if [[ -z "$line" ]]; then
    echo ""
    return
  fi
  # median=1.234 ms/frame
  sed -n 's/.*median=\([0-9.]*\) ms.*/\1/p' <<<"$line"
}

candidate_paths() {
  echo "auto|auto|auto"
  echo "scalar|scalar|scalar"
  if [[ "$FEATURES" == *portable-simd* ]]; then
    echo "portable|portable|portable"
  fi
  case "$(uname -m)" in
    x86_64|amd64)
      echo "sse128|sse128|sse2"
      echo "avx2|avx2|avx2"
      echo "avx512|avx512|avx512"
      ;;
    aarch64|arm64)
      echo "neon|neon|neon"
      if [[ "$FEATURES" == *sve* ]]; then
        echo "sve|sve|sve"
      fi
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

  # shellcheck disable=SC2086
  set +e
  $CARGO run --release $FEATURES --example simd_report -- "${args[@]}" >"$log" 2>&1
  local rc=$?
  set -e

  if grep -q '^skip:' "$log"; then
    echo "| \`$label\` | skipped | skipped | skipped | skipped |"
    return
  fi
  if [[ $rc -ne 0 ]]; then
    echo "| \`$label\` | error | error | error | error |"
    return
  fi

  local enc_u enc_b dec_u dec_b
  enc_u=$(extract_ms "$log" encode_uyvy)
  enc_b=$(extract_ms "$log" encode_bgra)
  dec_u=$(extract_ms "$log" 'load_from\+decode_uyvy')
  dec_b=$(extract_ms "$log" 'load_from\+decode_bgra')
  echo "| \`$label\` | ${enc_u:-?} | ${enc_b:-?} | ${dec_u:-?} | ${dec_b:-?} |"
  echo "$label,${enc_u:-},${enc_b:-},${dec_u:-},${dec_b:-}" >>"$SUMMARY_CSV"
}

{
  echo "# vmx-rs SIMD timing table"
  echo
  detect_cpu_md
  echo "## Toolchain"
  echo
  echo '```'
  $CARGO --version
  if [[ "$CARGO" == *nightly* ]]; then
    rustup run nightly rustc --version || true
  else
    # Prefer the rustc that matches CARGO when possible.
    $CARGO rustc -vV 2>/dev/null | head -1 || rustc --version || true
  fi
  echo "FEATURES=$FEATURES  size=${WIDTH}x${HEIGHT} iters=$ITERS"
  echo '```'
  echo
  echo "Times are **median ms/frame** (lower is better). Compare rows in this table only."
  echo
  echo "| path | encode_uyvy | encode_bgra | decode_uyvy | decode_bgra |"
  echo "|------|------------:|------------:|------------:|------------:|"
} >"$OUT_MD"

while IFS='|' read -r label dct color; do
  run_one "$label" "$dct" "$color" >>"$OUT_MD"
done < <(candidate_paths)

{
  echo
  echo "Raw logs: \`$ARTIFACT_DIR/*.log\` · CSV: \`$SUMMARY_CSV\`"
  echo
} >>"$OUT_MD"

if [[ "$(realpath "$OUT_MD" 2>/dev/null || echo "$OUT_MD")" != "$(realpath "$ARTIFACT_DIR/simd-bench.md" 2>/dev/null || echo "$ARTIFACT_DIR/simd-bench.md")" ]]; then
  cp "$OUT_MD" "$ARTIFACT_DIR/simd-bench.md"
fi
cat "$OUT_MD"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  cat "$OUT_MD" >>"$GITHUB_STEP_SUMMARY"
fi
