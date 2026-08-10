# vmx-rs decode benchmarks (1080p59.94)

See also [`../openmediatransport-rs/BENCHMARK.md`](https://github.com/MikanseiLaboratory/openmediatransport-rs/blob/main/BENCHMARK.md).

## Criterion (release)

```powershell
cargo bench --bench encode_decode -- vmx_decode_bgra_1080p
```

Observed (Windows x86_64, 2026-08-10):

| Bench | Mean |
|-------|------|
| `vmx_decode_bgra_1080p_60000_1001` | ~3.09 ms |

Optimizations contributing to the gate:

- SSE4.1 zig/dequant + pack path for plane decode
- Decode thread count scaled by resolution / `available_parallelism`
- **Fused per-slice decode → BGRA pack** (`decode_slices_fused_bgra`) to avoid a second full-frame planar scan
- Opaque BGRA path (no alpha plane fill/read)
