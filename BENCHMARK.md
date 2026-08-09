# Benchmarks

Criterion timings for live SIMD kernels vs scalar, plus end-to-end encode.
Absolute times vary with thermals and background load; prefer the **scalar vs SIMD ratio** from the same run.

## Test environment (this run)

| Item | Value |
|------|--------|
| Date (local) | 2026-08-10 |
| Host | ASUS / OEM “System Product Name” |
| CPU | Intel Core i9-9900K @ 3.60 GHz (8C/16T, Coffee Lake) |
| Detected ISA | SSE2, SSSE3, SSE4.1, SSE4.2, AVX2, BMI2 |
| RAM | 32 GiB |
| OS | Microsoft Windows 11 Pro 25H2 (build 10.0.26200) |
| Power plan | Ultimate Performance (`2ee41d02-…`) |
| rustc | 1.96.0 (`ac68faa20`, 2026-05-25), host `x86_64-pc-windows-msvc`, LLVM 22.1.2 |
| Crate / branch | `vmx` @ `perf/sse-rayon-hotpath` (`7d4c1bf` at measurement time) |
| Cargo profile | Criterion `bench` → release with `lto = "thin"`, `codegen-units = 1`, `opt-level = 3` |
| `RUSTFLAGS` | *(unset — not `target-cpu=native`)* |
| Criterion | warm-up **2 s**, measurement **5 s**, default sample count |
| Parallelism | `available_parallelism` = 16 (rayon uses this for encode benches) |

### Idle / interference check (before measurement)

| Check | Result |
|-------|--------|
| `\Processor(_Total)\% Processor Time` (5×1 s) | ~9–34% (mostly ~10–23%) |
| WMI `LoadPercentage` | ~11% |
| Competing `cargo` / `rustc` | **none** |
| Notable live CPU (≥1% of machine) | Discord ~3%, `ctfmon` ~2%, Epic Games Launcher ~1%, Cursor IDE present |
| Deliberately left running | Desktop apps above (not quit for this run) |

Post-run total CPU samples were higher (~25–38%) while Criterion / rayon were finishing.

## Per-SIMD kernels (`simd_paths`)

Workload: **1280×720** UYVY↔planar, or a full **720p luma** plane as 8×8 FDCT+quant blocks.
Calls go through `vmx::kernels` (`#[doc(hidden)]`); kernels are `pub` so Criterion
(separate crate) can link them. Not a stable API.

```bash
cargo bench --bench simd_paths -- --warm-up-time 2 --measurement-time 5
```

| Kernel | Path | Median time | vs scalar |
|--------|------|-------------|-----------|
| UYVY → planar | scalar | 1.016 ms | 1.0× |
| UYVY → planar | **SSSE3** | 94.7 µs | **~10.7×** |
| planar → UYVY | scalar | 1.329 ms | 1.0× |
| planar → UYVY | **SSE2** | 92.0 µs | **~14.4×** |
| FDCT + quant + zigzag (Y plane) | scalar | 2.352 ms | 1.0× |
| FDCT + quant + zigzag (Y plane) | **SSE4.2** | 697 µs | **~3.4×** |

Notes:

- Convert benches call scalar / SIMD kernels directly (no runtime dispatch beyond the wrapper call).
- FDCT benches walk every 8×8 block in a 1280×720 Y plane (same geometry as encode luma).
- AVX2 and NEON paths are still stubs → scalar; not listed separately.

## End-to-end encode (`encode_decode`)

Includes UYVY convert, slice-parallel encode (rayon), and `save_to` into an 8 MiB buffer.
Codec instance is reused across iterations.

```bash
cargo bench --bench encode_decode -- --warm-up-time 2 --measurement-time 5
```

| Bench | Median time |
|-------|-------------|
| `vmx_encode_uyvy_720p_omt_lq` | **1.063 ms** |
| `vmx_encode_uyvy_1080p_omt_hq` | **2.806 ms** |

## Reproducing

```bash
cargo bench --bench simd_paths -- --warm-up-time 2 --measurement-time 5
cargo bench --bench encode_decode -- --warm-up-time 2 --measurement-time 5

# optional: tune for the host CPU
RUSTFLAGS="-C target-cpu=native" cargo bench --bench simd_paths
```

For cleaner numbers, quit Discord / game launchers / browser heavy tabs and confirm no other `cargo`/`rustc` builds before starting.
