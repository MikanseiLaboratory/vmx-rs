# Benchmark pictures

The four 8-bit UYVY frames the harness builds in `img_natural` and `img_bars` (`bench/src/main.rs`). Stride is `width * 2`, packed `U Y V Y`, progressive, no header.

| File | Pixels | Uncompressed |
|---|---|---|
| `1080p-natural.uyvy.gz` | 1920×1080 | 4,147,200 bytes |
| `1080p-bars.uyvy.gz` | 1920×1080 | 4,147,200 bytes |
| `2160p-natural.uyvy.gz` | 3840×2160 | 16,588,800 bytes |
| `2160p-bars.uyvy.gz` | 3840×2160 | 16,588,800 bytes |

`natural` is smooth gradients, a low-frequency texture, about ±6 LSB of noise, one oval, and one rectangle. `bars` is eight flat 75% colour bars and no noise. Both are fully determined by that code (the natural image starts its generator at `0x1234_5678`), so a second run writes the same bytes.

The `.uyvy.gz` files are gzip of those bytes (`mtime` 0). SHA-256 of the uncompressed frames is in `SHA256SUMS`:

```bash
gzip -dc 1080p-natural.uyvy.gz | sha256sum
```

The `.png` files are 480×270 previews for looking at the picture, not a substitute for the UYVY frames. `natural` at 2160p is the same formula on a finer grid, so its preview looks like the 1080p one.
