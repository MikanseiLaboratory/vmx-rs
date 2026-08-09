# vmx-rs

Pure Rust [VMX1](https://github.com/openmediatransport/libvmx) video codec.

> **Disclaimer:** This is an independent, community-maintained project. It is **not** an official Open Media Transport product or repository.

## Related projects

| Project | Description |
|---------|-------------|
| [Open Media Transport (official)](https://github.com/openmediatransport) | Official OMT organization and documentation |
| [libomt](https://github.com/openmediatransport/libomt) | Official C/C++ OMT core library |
| [libomtnet](https://github.com/openmediatransport/libomtnet) | Official .NET OMT bindings |
| [libvmx](https://github.com/openmediatransport/libvmx) | Official VMX1 video codec (reference implementation) |
| [openmediatransport-rs](https://github.com/MikanseiLaboratory/openmediatransport-rs) | Pure Rust OMT protocol stack |

## Goals

- Byte-compatible with `libvmx` (container, entropy, DCT/quant)
- No native library / FFI / runtime DLL dependency
- Cross-platform: Windows / Linux / macOS × x86_64 / ARM64
- Runtime SIMD dispatch where implemented (see table below)
- Slice-parallel encode/decode via [rayon](https://crates.io/crates/rayon)
- MSRV: Rust 1.88 (edition 2024)

## SIMD & parallelism vs official `libvmx`

Status of hot-path kernels compared to
[`openmediatransport/libvmx`](https://github.com/openmediatransport/libvmx)
(`vmxcodec_x86.cpp` / `vmxcodec_avx2.cpp` / `vmxcodec_arm.cpp` + `ThreadTasks`).

| Kernel | libvmx (official) | vmx-rs (this crate) |
|--------|-------------------|---------------------|
| **FDCT + quant + zigzag (8-bit)** | SSE4.2 / SSSE3 (`VMX_FDCT_8X8_QUANT_ZIG_128`); AVX2+BMI2 (`…_256`) when chroma width ÷16 | **Live:** SSE4.2 (`fdct_quant_zig_sse`). AVX2 / NEON modules exist but still call scalar |
| **FDCT + quant (10/16-bit)** | SSE / AVX2 `…_128_16` / `…_256_16` | Scalar only |
| **IDCT + dequant + zigzag** | SSE128 / AVX2 (`VMX_ZIG_INVQUANTIZE_IDCT_8X8_*`) | **Scalar only** (byte-compatible port in `codec/dct.rs`) |
| **UYVY → planar** | SSE (`VMX_UYVYToPlanar`, SSSE3 shuffle) | **Live:** SSSE3 (`uyvy_to_planar_ssse3`), scalar fallback |
| **planar → UYVY** | SSE (`VMX_PlanarToUYVY`) | **Live:** SSE2 (`planar_to_uyvy_sse2`), scalar fallback |
| **YUY2 / NV12 / YV12 / P216 / BGRA ↔ planar** | SSE paths in libvmx | **Scalar only** |
| **Slice parallelism** | `ThreadTasks` — N workers from bitrate table; convert+encode fused per slice | **rayon** pool sized from the same bitrate `Threads` column; full-frame convert then parallel encode/decode slices |
| **ARM64** | SSE intrinsics via `sse2neon.h` | NEON dispatch stub → scalar (no live NEON kernels yet) |

**Summary:** encode on x86_64 with SSE4.2 + SSSE3 is the path that currently matches libvmx’s *class* of acceleration (128-bit). Decode IDCT, AVX2, NEON, and most extra color formats are still behind the official library.

Runtime feature checks use `is_x86_feature_detected!` (SSE4.2 for FDCT, SSSE3/SSE2 for UYVY). AVX2 is detected and recorded on `CpuFeatures` but not used for DCT until the intrinsic port lands.

## Rayon (this crate) vs Tokio (OMT stack)

`vmx` depends on **rayon only** — it does **not** depend on Tokio.

| Layer | Crate | Role |
|-------|--------|------|
| CPU (DCT / slices) | `vmx` → **rayon** | Data-parallel work over slice bands |
| I/O (TCP / timers) | `openmediatransport` → **tokio** (optional) | Async networking |

That split is intentional and matches common Rust practice (and Tokio’s guidance): keep CPU work off the async executor; use `block_in_place` / `spawn_blocking` at the OMT boundary when calling into `vmx` from async code.

Coexistence in one *process* is fine. Watch **thread oversubscription** at high thread counts (Tokio worker threads + rayon’s bitrate-sized pool). For 720p OMT profiles the pool is typically 2 threads, which is usually comfortable.

## Performance notes

Prefer Release builds. Optional fat LTO profile:

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --profile release-fast
```

```bash
cargo bench --bench encode_decode
```

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

## License

MIT — Copyright (c) 2025 Open Media Transport Contributors and MikanseiLaboratory.
