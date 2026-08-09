# vmx-rs

Pure Rust [VMX1](https://github.com/openmediatransport/libvmx) video codec.

## Goals

- Byte-compatible with `libvmx` (container, entropy, DCT/quant)
- No native library / FFI / runtime DLL dependency
- Cross-platform: Windows / Linux / macOS × x86_64 / ARM64
- Runtime SIMD dispatch: scalar, SSE4.2, AVX2+BMI2, NEON
- MSRV: Rust 1.88 (edition 2024)

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
