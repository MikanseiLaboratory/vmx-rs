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
| ARMv7 (A32) | — | Native NEON behind the `arm32-neon` feature (nightly) |

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

## Install

The crates.io package is [`vmx`](https://crates.io/crates/vmx).

```toml
vmx = "0.2"
```

AArch32 (armv7) NEON kernels are opt-in and require a nightly toolchain, because
the A32 intrinsics are still unstable (rust-lang/rust#111800):

```toml
vmx = { version = "0.2.2", features = ["arm32-neon"] }
```

```sh
cargo +nightly build --target armv7-unknown-linux-gnueabi --features arm32-neon
```

On a soft-float target LLVM cannot always codegen these kernels; compiling only
this crate with `-C target-feature=-soft-float,+neon` is known to work (see the
`lg-webos-tv-omt-monitor` project).

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
texture and return after `queue.submit`. Each call writes a new texture;
later decodes do not overwrite it. Later submits on the same queue can
sample it; CPU readback still waits in `read_texture_bgra`.
`decode_to_texture_into` writes into a caller-owned texture instead of
allocating one.

`decode_to_texture` is the fast path: 32-byte coefficient records and a float
AAN IDCT. It is useful for previews and monitoring, but it is **not
bit-identical** to the CPU decoder (coefficients 31..63 are dropped and AC is
clamped to i8). `decode_to_texture_exact` uses 132-byte records, an integer
IDCT and the CPU's DC-only shortcut; it matches the CPU output byte for byte.

`encode_from_texture` accepts `Bgra8Unorm` or `Rgba8Unorm` with `COPY_SRC` and
is followed by `save_to`. Use `encode_from_texture_opaque` to skip the alpha
plane (libvmx `EncodeBGRX`). `encode_from_texture_submit` /
`encode_submitted_finish` pipeline GPU FDCT against CPU entropy coding; up to
three frames can be in flight. `encode_from_texture_view` accepts a
caller-owned `TextureView` to avoid a per-frame view creation.

`gpu_path_recommended(&adapter)` reports whether the wgpu path is expected to
beat the CPU path for that adapter. On integrated GPUs (including Apple
silicon) the CPU path is faster at 1080p and 2160p; the wgpu path is mainly a
way to avoid host↔device transfers when the frame already lives on a discrete
GPU. `gpu_stats()` returns the last decode/encode stage timings.

Encode fuses the 16×8 Y/U/V/A tile into one dispatch and samples each source
pixel once. Decode uses a 9-float shared-memory row stride to avoid bank
conflicts. When the adapter exposes `BGRA8UNORM_STORAGE`, the compute pass
writes the output texture directly (no buffer→texture copy). The headless
device helper requests the adapter's real 8K buffer/storage limits.

```bash
cargo test --features wgpu
cargo run --release --example gpu_bench --features wgpu
```

## License

MIT — Copyright (c) 2026 Open Media Transport Contributors and MikanseiLaboratory.
