# vmx-rs

[![CI](https://github.com/MikanseiLaboratory/vmx-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MikanseiLaboratory/vmx-rs/actions/workflows/ci.yml)

Pure Rust [VMX1](https://github.com/openmediatransport/libvmx) video codec.

> **Disclaimer:** Independent community project. Separate from official Open Media Transport products and repositories.

## Related projects

| Project | Description |
|---------|-------------|
| [Open Media Transport](https://github.com/openmediatransport) | OMT organization and documentation |
| [libomtnet](https://github.com/openmediatransport/libomtnet) | .NET OMT core |
| [libomt](https://github.com/openmediatransport/libomt) | C wrapper for libomtnet |
| [libvmx](https://github.com/openmediatransport/libvmx) | VMX1 video codec |
| [openmediatransport-rs](https://github.com/MikanseiLaboratory/openmediatransport-rs) | Rust OMT protocol stack |

## Goals

- Byte-compatible with `libvmx`
- No native library / FFI / runtime DLL dependency
- Cross-platform: Windows / Linux / macOS × x86_64 / ARM64
- Runtime SIMD dispatch where implemented
- Slice-parallel encode/decode via [rayon](https://crates.io/crates/rayon)
- MSRV: Rust 1.97 (edition 2024)

## SIMD vs `libvmx`

| Kernel | libvmx | vmx-rs |
|--------|--------|--------|
| FDCT + quant (8-bit) | SSE4.2 / AVX2+BMI2 | SSE4.2; AVX2 dual-8×8 |
| FDCT + quant (10/16-bit) | SSE / AVX2 | Unimplemented |
| IDCT + dequant (8-bit) | SSE / AVX2 | SSE4.1; AVX2 dual-block; NEON; optional `std::simd` |
| UYVY ↔ planar | SSSE3 | SSSE3 |
| planar ↔ UYVY | SSE | SSE2 |
| Other color formats | SSE | BGRA→YUV encode: SSSE3 (Avx2 path falls through); YUV→BGRA: SSE2/AVX2/NEON (+ optional `std::simd`); others: scalar |
| Preview | DC 1/8 | DC 1/8 + planar pack |
| Slice parallelism | `ThreadTasks` | rayon |
| ARM64 | sse2neon | Native NEON |

### Instruction-family checklist

| Family | Role | Status | Verified |
|--------|------|--------|----------|
| **SSE2** | planar ↔ UYVY | Live | [x] |
| **SSSE3** | UYVY ↔ planar | Live | [x] |
| **SSE4.2** | FDCT + quant encode | Live | [x] |
| **AVX2 + BMI2** | Dual-block plane encode/decode | Live when UV width % 16 == 0 | [x] |
| **AVX-512F+BW** | Quad IDCT + 32-wide YUV→BGRA | Live when UV width % 32 == 0 | [x] |
| **NEON** | ARM64 plane encode/decode | Live | [x] |
| **`std::simd`** | Portable FDCT/IDCT + YUV→BGRA | Opt-in (`portable-simd`, nightly) | [x] |

### Runtime path reporting

```rust
let codec = Codec::new(Config::new(1920, 1080))?;
println!("path={}", codec.simd_path()); // "avx2" | "sse128" | "neon" | "portable" | "scalar"
let caps = codec.simd_capabilities();
```

Selection priority:

- **x86_64:** AVX-512 if `avx512f+bw && bmi2 && (width/2) % 32 == 0`, else AVX2 if `avx2 && bmi2 && (width/2) % 16 == 0`, else SSE4.2+SSSE3, else `portable` (feature) / Scalar
- **aarch64:** Neon, else `portable` (feature) / Scalar

### Optional nightly portable SIMD

```bash
rustup run nightly cargo test --features portable-simd
rustup run nightly cargo run --release --features portable-simd --example simd_report -- \
  1920 1080 16 portable portable
```

The `portable-simd` feature enables a `std::simd` fallback that accelerates the
scalar path on hosts without arch-specific kernels (≈4× IDCT, ≈5× YUV→BGRA vs
scalar in local release measurements). Default stable builds are unchanged.

### Cross-ISA CI benchmarks

Workflow [`.github/workflows/simd-bench.yml`](.github/workflows/simd-bench.yml) runs
`scripts/simd_bench_matrix.sh` on:

| Runner | Typical ISA | Paths compared |
|--------|-------------|----------------|
| `ubuntu-latest` | x86_64 AVX2 | auto / scalar / sse128 / avx2 / portable |
| `ubuntu-24.04-arm` | ARM64 NEON | auto / scalar / neon / portable |
| `macos-latest` | Apple Silicon NEON | auto / scalar / neon / portable |
| `macos-15-intel` | x86_64 AVX2 (if available) | auto / scalar / sse128 / avx2 / portable |

Results land in the job summary and as downloadable artifacts (`simd-bench-*`).

**AVX-512:** Hosted runners may not expose it; this environment does. The workflow
detects `avx512f`/`avx512bw` and the `avx512` path is included in the timing table
when available. Plane encode still uses the tuned AVX2 dual-block kernels;
decode color uses a real 32-wide AVX-512 YUV→BGRA pack, and a quad-block AVX-512
IDCT kernel is covered by unit/micro benches.

## Usage

```rust
use vmx::{Codec, Config, Profile};

let mut enc = Codec::new(Config {
    width: 1920,
    height: 1080,
    profile: Profile::OmtHq,
    color_space: Default::default(),
})?;

enc.encode_uyvy(&frame, stride)?;
let mut buf = vec![0u8; 8 << 20];
let len = enc.save_to(&mut buf)?;

let mut dec = Codec::new(Config::new(1920, 1080))?;
dec.load_from(&buf[..len])?;
dec.decode_uyvy(&mut out, stride)?;
```

```bash
cargo test
RUSTFLAGS="-C target-cpu=native" cargo build --profile release-fast
```

## License

MIT — Copyright (c) 2026 Open Media Transport Contributors and MikanseiLaboratory.
