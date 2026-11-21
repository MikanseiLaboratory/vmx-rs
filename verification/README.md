# wgpu path verification (Lean 4)

This directory contains a small, Mathlib-free Lean 4 model of the properties
used in the 2026-11 wgpu audit of `vmx-rs`.

Lean proves *equivalence*; it does not measure speed. The performance numbers
in the audit come from `examples/gpu_bench.rs` and `VMX_GPU_TRACE`.

## What is proved

- `zigzag` / `zigzagInv` are inverse permutations on `0..63`, and the
  encode-pack / decode-load round-trip is the identity.
- The current 32-byte GPU coefficient record is lossless **iff** the
  coefficient is the DC or an AC in `1..30` that fits in `i8`.  Coefficients
  above 30 are lost; values outside `i8` are saturated.
- A 128-byte record (64 `i16`) preserves every coefficient.
- `dq / 16.0` (truncation toward zero) is not the CPU's arithmetic `dq >> 4`
  for negative non-multiples of 16.
- i16 saturation changes values outside `[-32768, 32767]`.
- A shared-memory row stride of 9 is conflict-free with 32 banks; stride 8 is
  not.
- A 16x8 encode tile currently reads 512 source texels; pair fusion (Y+A,
  U+V) halves that, and a fully fused tile quarters it.

## 実装結果

- [実装結果と最終ベンチマーク](IMPLEMENTATION-2026-11.md)

## Run

```bash
cd verification/lean
lake env lean VmxGpu.lean
```

The checked-in `.lean` file uses the Lean 4.36 toolchain pinned in
`lean-toolchain`. No Mathlib download is needed.
