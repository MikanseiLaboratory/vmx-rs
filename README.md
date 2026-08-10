# vmx-rs

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
| FDCT + quant (8-bit) | SSE4.2 / AVX2+BMI2 | **SSE4.2 live**; AVX2 stub → scalar |
| FDCT + quant (10/16-bit) | SSE / AVX2 | Scalar |
| IDCT + dequant | SSE / AVX2 | Scalar |
| UYVY → planar | SSE (SSSE3) | **SSSE3 live** |
| planar → UYVY | SSE | **SSE2 live** |
| Other color formats | SSE | Scalar |
| Slice parallelism | `ThreadTasks` | rayon (bitrate `Threads`) |
| ARM64 | sse2neon | NEON stub → scalar |

### Instruction-family checklist

| Family | Role | Status | Verified |
|--------|------|--------|----------|
| **SSE2** | planar → UYVY | Live | [x] |
| **SSSE3** | UYVY → planar | Live | [x] |
| **SSE4.2** | FDCT + quant encode | Live | [x] |
| **AVX2 + BMI2** | DCT (libvmx path) | Stub → scalar | [ ] |
| **NEON** | ARM64 kernels | Stub → scalar | [ ] |

Measured scalar vs SIMD timings (i9-9900K): see **[BENCHMARK.md](BENCHMARK.md)** (~11× SSSE3 convert, ~14× SSE2 convert, ~3.4× SSE4.2 FDCT).

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
cargo bench --bench simd_paths
cargo bench --bench encode_decode
RUSTFLAGS="-C target-cpu=native" cargo build --profile release-fast
```

## License

MIT — Copyright (c) 2026 Open Media Transport Contributors and MikanseiLaboratory.
