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
| IDCT + dequant (8-bit) | SSE / AVX2 | SSE4.1; AVX2 dual-block; NEON |
| UYVY ↔ planar | SSSE3 | SSSE3 |
| planar ↔ UYVY | SSE | SSE2 |
| Other color formats | SSE | BGRA→YUV encode: SSSE3 (Avx2 path falls through); YUV→BGRA: SSE2/AVX2/NEON; others: scalar |
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
| **NEON** | ARM64 plane encode/decode | Live | [x] |

### Runtime path reporting

```rust
let codec = Codec::new(Config::new(1920, 1080))?;
println!("path={}", codec.simd_path()); // "avx2" | "sse128" | "neon" | "scalar"
let caps = codec.simd_capabilities();
```

Selection priority:

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

## wgpu texture I/O (`feature = "wgpu"`)

Callers pass their existing `Device` / `Queue`. After `load_from`,
`decode_to_texture` / `decode_preview_to_texture` produce a `Bgra8Unorm`
texture (the API waits for GPU completion). `encode_from_texture` accepts
`Bgra8Unorm` or `Rgba8Unorm` with `COPY_SRC` and is followed by `save_to`.

Integer AAN IDCT/FDCT matches the CPU scalar 8-bit path.

```bash
cargo test --features wgpu
```

## License

MIT — Copyright (c) 2026 Open Media Transport Contributors and MikanseiLaboratory.
