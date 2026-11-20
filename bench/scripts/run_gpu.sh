#!/usr/bin/env bash
# Measure every bench/images UYVY frame at every OMT profile on a GPU.
#
# Intended host: a GCP g2 VM (Ubuntu) with an NVIDIA L4, the NVIDIA driver,
# libnvidia-gl-580-server, and libegl1. wgpu reports success on llvmpipe when
# those GL/EGL libraries are missing. This script exits before any timed
# sample if the adapter is llvmpipe, lavapipe, SwiftShader, or a CPU device.
#
# From a checkout of this repo on that VM:
#   bash bench/scripts/run_gpu.sh
#   OUT=/tmp/gpu.csv GPU_BENCH_ITERS=20 bash bench/scripts/run_gpu.sh
#
# 1080p uses GPU_BENCH_ITERS (default 20). 2160p uses half of that, and at
# least 4, matching the synthetic second pass in examples/gpu_bench.rs.
# gpu_bench then discards GPU_BENCH_PRIME (default 20) untimed iterations of
# each GPU path before the timed samples. Those iterations are not in the
# CSV. The per-case warm-up inside the timed loop is also excluded. The CSV
# replaces OUT.
#
# One image and one profile (CSV on stdout, notes on stderr):
#   cargo run --release --example gpu_bench --features wgpu --locked -- \
#     --image bench/images/1080p-natural.uyvy.gz --profile OMT_HQ --iters 20 \
#     --prime 20 --require-gpu
#
# Synthetic gradient, OMT_SQ, 1920x1080 and then 3840x2160:
#   cargo run --release --example gpu_bench --features wgpu --locked -- \
#     --synthetic --require-gpu

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

OUT=${OUT:-"$ROOT/bench/results/gcp-gpu-l4-images.csv"}
ITERS=${GPU_BENCH_ITERS:-20}
PRIME=${GPU_BENCH_PRIME:-20}
if ! [[ "$ITERS" =~ ^[1-9][0-9]*$ ]]; then
  echo "GPU_BENCH_ITERS must be a positive integer (got $ITERS)" >&2
  exit 1
fi
if ! [[ "$PRIME" =~ ^[0-9]+$ ]]; then
  echo "GPU_BENCH_PRIME must be a non-negative integer (got $PRIME)" >&2
  exit 1
fi

images=(
  "$ROOT/bench/images/1080p-natural.uyvy.gz"
  "$ROOT/bench/images/1080p-bars.uyvy.gz"
  "$ROOT/bench/images/2160p-natural.uyvy.gz"
  "$ROOT/bench/images/2160p-bars.uyvy.gz"
)
for image in "${images[@]}"; do
  if [[ ! -f "$image" ]]; then
    echo "missing image: $image" >&2
    exit 1
  fi
done

mkdir -p "$(dirname "$OUT")"

echo "checking wgpu adapter (exit if llvmpipe or a CPU device)" >&2
cargo run --release --example gpu_bench --features wgpu --locked -- \
  --probe --require-gpu

half=$((ITERS / 2))
if ((half < 4)); then
  half=4
fi

echo "res,image,profile,case,ms_median,fps_median,ms_samples" >"$OUT"

profiles=(OMT_LQ OMT_SQ OMT_HQ)
for image in "${images[@]}"; do
  base=$(basename "$image")
  iters=$ITERS
  if [[ "$base" == 2160p-* ]]; then
    iters=$half
  fi
  for profile in "${profiles[@]}"; do
    echo "measuring $base $profile iters=$iters" >&2
    cargo run --release --example gpu_bench --features wgpu --locked -- \
      --image "$image" \
      --profile "$profile" \
      --iters "$iters" \
      --prime "$PRIME" \
      --require-gpu \
      --no-header \
      >>"$OUT"
  done
done

echo "wrote $OUT" >&2
