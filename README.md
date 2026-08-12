# vmx-rs

[![CI](https://github.com/MikanseiLaboratory/vmx-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MikanseiLaboratory/vmx-rs/actions/workflows/ci.yml)

Pure Rust [VMX1](https://github.com/openmediatransport/libvmx) video codec.

> **Disclaimer:** This is an independent, community-maintained project. It is **not** an official Open Media Transport product or repository.

## Related projects

| Project | Description |
|---------|-------------|
| [Open Media Transport (official)](https://github.com/openmediatransport) | Official OMT organization and documentation |
| [libomtnet](https://github.com/openmediatransport/libomtnet) | Official .NET OMT core |
| [libomt](https://github.com/openmediatransport/libomt) | Official C wrapper for libomtnet |
| [libvmx](https://github.com/openmediatransport/libvmx) | Official VMX1 video codec |
| [openmediatransport-rs](https://github.com/MikanseiLaboratory/openmediatransport-rs) | Pure Rust OMT protocol stack |

## Goals

- Byte-compatible with `libvmx` (container, entropy, DCT/quant)
- No native library / FFI / runtime DLL dependency
- Cross-platform: Windows / Linux / macOS × x86_64 / ARM64
- Runtime SIMD dispatch where implemented
- Slice-parallel encode/decode via [rayon](https://crates.io/crates/rayon)
- MSRV: Rust 1.97 (edition 2024)

## SIMD vs `libvmx`

| Kernel | libvmx | vmx-rs |
|--------|--------|--------|
| FDCT + quant (8-bit) | SSE4.2 / AVX2+BMI2 | **SSE4.2 live**; **AVX2+BMI2** dual-block path (FDCT via SSE kernel ×2 + AVX2 mask) |
| FDCT + quant (10/16-bit) | SSE / AVX2 | Scalar |
| IDCT + dequant (8-bit) | SSE / AVX2 | **SSE4.1** dequant/pack; **AVX2** dual-block decode; **NEON** dequant/pack + scalar IDCT on aarch64 |
| UYVY → planar | SSE (SSSE3) | **SSSE3 live** |
| planar → UYVY | SSE | **SSE2 live** |
| Other color formats | SSE | Scalar |
| Slice parallelism | `ThreadTasks` | rayon |
| ARM64 | sse2neon | **Native NEON** FDCT+quant+zigzag encode; hybrid NEON dequant/pack decode |

### Instruction-family checklist

| Family | Role | Status | Verified |
|--------|------|--------|----------|
| **SSE2** | planar → UYVY | Live | [x] |
| **SSSE3** | UYVY → planar | Live | [x] |
| **SSE4.2** | FDCT + quant encode | Live | [x] |
| **AVX2 + BMI2** | Dual-block plane encode/decode | Live (x86_64, UV width % 16 == 0) | [x] |
| **NEON** | ARM64 plane encode/decode | Live (aarch64) | [x] cross-compile |

### Runtime path reporting

```rust
let codec = Codec::new(Config::new(1920, 1080))?;
println!("path={}", codec.simd_path()); // "avx2" | "sse128" | "neon" | "scalar"
let caps = codec.simd_capabilities();   // host features (not rewritten by geometry)
```

Selection priority (matches libvmx gates):

- **x86_64:** AVX2 if `avx2 && bmi2 && (width/2) % 16 == 0`, else SSE4.2+SSSE3, else Scalar
- **aarch64:** Neon, else Scalar

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
