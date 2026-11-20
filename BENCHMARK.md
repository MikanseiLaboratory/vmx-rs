# Benchmark

Measurements of three VMX1 implementations on the same 8-bit UYVY frames. The tables are medians from one run. They are not a ranking, and a gap inside the run-to-run band described below is not a stable difference.

| Implementation | Commit / version | What was timed |
|---|---|---|
| [libvmx](https://github.com/openmediatransport/libvmx) | `31724bb96530325a989bc157c8e07538d6f1c4db` | Official C++ codec |
| [vmx-codec](https://github.com/Saul-Punybz/open-media-transport) | `fbde53cccabb1ebfff7ba773a72510ab48a64f3a` (`crates/vmx-codec`) | Pure Rust codec by Saul González |
| vmx (this repository) | 0.2.1 | Pure Rust codec |

The harness and the raw rows are in [`bench/`](bench/). Re-running it on other hardware will not reproduce these frames per second.

## Environment (x86-64)

- CPU: Intel Xeon, family 6 model 207 (Emerald Rapids), AVX2 and AVX-512 available, KVM guest, 8 vCPU, no SMT
- Memory: 15 GB
- OS: Debian 13, kernel 6.12.113+
- rustc 1.100.0, clang++ 19.1.7
- The machine is a shared VM. The run notes put movement between repeats on that host at about 10–20%.

libvmx was compiled with the flags from `build/buildlinuxx64.sh`: `clang++ -O3 -std=c++17 -fdeclspec -mlzcnt -mavx2 -mbmi`, and linked statically. The process enables the AVX2 path when the CPU reports AVX2 and BMI2. A second column forces that flag off so the 128-bit path is what runs.

vmx-codec on x86-64 uses the crate's compile-time SSE2 code. It is not an AVX2 build. libvmx's 128-bit path is the closer instruction-set neighbor.

vmx 0.2.1 dispatches at runtime. On this host the codec reported `dct=Avx2` and `color=Avx2`. The Rust crates were built with this repository's release profile: opt-level 3, thin LTO, one codegen unit. libvmx is `-O3` per translation unit, without link-time optimization, which is how its Linux build script invokes the compiler.

A second pass over a subset of the natural-image rows, with `RUSTFLAGS="-C target-cpu=native"` on the Rust crates only, is checked in as [`bench/results/x86-64-native.csv`](bench/results/x86-64-native.csv). Those medians move within the same 10–20% band, so the tables in this file are the default release build ([`bench/results/x86-64-default.csv`](bench/results/x86-64-default.csv), 384 rows).

## Method

`bench` fetches libvmx at the commit above, builds it, and depends on vmx-codec at the commit above. Each row is one implementation, one operation, and one thread count.

- Pictures: 1920×1080 and 3840×2160, 8-bit UYVY, progressive.
- Profiles: LQ, SQ, HQ, OMT_LQ, OMT_SQ, OMT_HQ (the `VMX_PROFILE` values).
- Images: `natural` (smooth gradients, low-frequency texture, about ±6 LSB of noise, a few hard edges) and `bars` (75% colour bars, eight flat regions, no noise).
- Quality: libvmx's own rate control is run for 60 frames on that picture. The `q` it settles on is then fixed. Every timed encode calls set-quality with that `q`. DC shift and minimum quality stay at the profile defaults. This is not a rate-control benchmark; the profile still decides which `q` the picture settles at, and therefore how large the frame is.
- Encode is colour conversion, coding, and serialize (`VMX_EncodeUYVY` + `VMX_SaveTo`, `Encoder::encode_into` into a reused `Vec`, `Codec::encode_uyvy` + `save_to`).
- Decode is load plus decode to UYVY. Every decoder receives the same packet, produced by libvmx's 128-bit path at the fixed `q`.
- Input and output buffers are allocated before the timer starts. The timed region does not copy the packet into a new buffer.
- Three warmup frames, then seven timed runs of at least 0.3 s each. The published value is the median. The CSV also has the min and max of those seven runs.
- Thread counts: 1 and 8, via each implementation's thread setter. 8 is the vCPU count of the machine.

On every one of the 384 rows the encoded bytes matched the libvmx 128-bit packet, and the decoded pixels matched.

In the tables, `✗` means the median is below 60 fps. The `±` figure is half of `(max − min) / median` for the seven runs, rounded to the nearest percent. It is the spread inside that sample, not the 10–20% movement between separate runs. `KB/frame` in the short tables is `bytes / 1000`, rounded. `highest` is whichever of the four medians on that row was largest.

10-bit P216 and PA16 are not in the matrix. `vmx` 0.2.1 returns `invalid parameters` from `encode_p216`, `decode_p216`, `encode_pa16`, and `decode_pa16`. libvmx and vmx-codec both implement those formats; this file does not compare them.

## Realtime (1080p60 / 2160p60)

60 fps is one frame interval at 1080p60 or 2160p60. The figure is the codec call only. It does not include capture, a colour conversion outside the codec, or a transport.

1080p, every profile, both images, encode and decode, 1 thread and 8 threads: all three medians are above 60 fps. The lowest 1080p median in the short tables is vmx-codec, HQ, natural, decode, 1 thread, at 106 fps.

2160p, 8 threads: every median in the short tables is above 60 fps. The lowest is vmx-codec, HQ, natural, encode, at 101 fps.

2160p, 1 thread, from the short tables:

- Encode: libvmx stays above 60 fps on every row. vmx is below 60 fps only for HQ natural (48 fps). vmx-codec is below 60 fps on every 2160p encode row (32–53 fps).
- Decode: HQ natural is below 60 fps for all three (libvmx 44, vmx-codec 27, vmx 47). vmx-codec is also below 60 fps for natural SQ (50) and natural OMT_HQ (52). The other 1-thread 2160p decode rows are above 60 fps.

## Relative to libvmx

Ratio of medians, `vmx / libvmx` and `vmx-codec / libvmx`, using the AVX2 libvmx column. The summary is the geometric mean over the 24 rows of that operation and thread count (6 profiles × 2 images × 2 resolutions). The range is the smallest and largest ratio in that set.

| Operation | Threads | vmx / libvmx | vmx-codec / libvmx |
|---|---:|---:|---:|
| Encode | 1 | 0.78 (0.65–0.88) | 0.39 (0.30–0.50) |
| Encode | 8 | 1.44 (1.13–1.63) | 0.29 (0.21–0.48) |
| Decode | 1 | 0.82 (0.67–1.08) | 0.60 (0.49–0.68) |
| Decode | 8 | 1.28 (0.87–2.05) | 0.32 (0.17–0.70) |

Split by image, vmx only:

| Operation | Threads | natural | bars |
|---|---:|---:|---:|
| Encode | 1 | 0.78 (0.65–0.88) | 0.79 (0.73–0.86) |
| Encode | 8 | 1.43 (1.13–1.63) | 1.45 (1.21–1.58) |
| Decode | 1 | 0.92 (0.74–1.08) | 0.73 (0.67–0.82) |
| Decode | 8 | 1.58 (1.13–2.05) | 1.04 (0.87–1.23) |

vmx-codec's SSE2 code is also slower than libvmx's own 128-bit path, so the gap above is not only the wider AVX2 code. Geometric mean of `vmx-codec / libvmx 128-bit` over the same 24 rows: encode 1 thread 0.59, encode 8 threads 0.42, decode 1 thread 0.64, decode 8 threads 0.34. The low end of decode at 8 threads is the flat colour bars (0.16–0.26 against the 128-bit path).

Read the ratios next to the noise. A geometric mean of 1.44 on 8-thread encode is outside a 10–20% band. A single row at 1.13, or the 8-thread bars decode mean of 1.04, is not. Several 8-thread bars decode rows are higher for libvmx's 128-bit path than for either AVX2 libvmx or vmx; the `highest` column records that.

The single-thread rows where vmx is behind libvmx, and the flat-bar decode rows, are the ones discussed as follow-up work in [issue 39](https://github.com/MikanseiLaboratory/vmx-rs/issues/39). This document does not change the codec.

## x86-64 tables

Short tables: libvmx AVX2 / vmx-codec / vmx. Full tables, including libvmx's 128-bit path, follow.

### None

| Resolution | Profile | Image | q | KB/frame | libvmx | vmx-codec | vmx |
|---|---|---|---:|---:|---:|---:|---:|

### Encode, 1 thread

| Resolution | Profile | Image | q | KB/frame | libvmx | vmx-codec | vmx |
|---|---|---|---:|---:|---:|---:|---:|
| 1080p | LQ | natural | 92 | 169 | 379 | 178 | 322 |
| 1080p | SQ | natural | 94 | 292 | 328 | 153 | 279 |
| 1080p | HQ | natural | 96 | 559 | 263 | 122 | 179 |
| 1080p | OMT_LQ | natural | 92 | 169 | 386 | 180 | 339 |
| 1080p | OMT_SQ | natural | 95 | 419 | 318 | 141 | 225 |
| 1080p | OMT_HQ | natural | 96 | 559 | 268 | 131 | 200 |
| 1080p | LQ | bars | 98 | 22 | 609 | 221 | 514 |
| 1080p | SQ | bars | 98 | 22 | 607 | 224 | 511 |
| 1080p | HQ | bars | 98 | 24 | 649 | 219 | 506 |
| 1080p | OMT_LQ | bars | 98 | 22 | 641 | 221 | 517 |
| 1080p | OMT_SQ | bars | 98 | 24 | 634 | 224 | 506 |
| 1080p | OMT_HQ | bars | 98 | 24 | 597 | 221 | 510 |
| 2160p | LQ | natural | 92 | 596 | 102 | 43 ✗ | 78 |
| 2160p | SQ | natural | 94 | 1088 | 82 | 38 ✗ | 68 |
| 2160p | HQ | natural | 96 | 2146 | 64 | 32 ✗ | 48 ✗ |
| 2160p | OMT_LQ | natural | 92 | 596 | 102 | 41 ✗ | 86 |
| 2160p | OMT_SQ | natural | 92 | 596 | 104 | 45 ✗ | 86 |
| 2160p | OMT_HQ | natural | 94 | 1192 | 98 | 42 ✗ | 64 |
| 2160p | LQ | bars | 98 | 76 | 160 | 52 ✗ | 117 |
| 2160p | SQ | bars | 98 | 76 | 154 | 52 ✗ | 118 |
| 2160p | HQ | bars | 98 | 81 | 153 | 46 ✗ | 120 |
| 2160p | OMT_LQ | bars | 98 | 76 | 154 | 53 ✗ | 118 |
| 2160p | OMT_SQ | bars | 98 | 76 | 157 | 48 ✗ | 115 |
| 2160p | OMT_HQ | bars | 98 | 81 | 148 | 50 ✗ | 116 |

### Encode, 8 threads

| Resolution | Profile | Image | q | KB/frame | libvmx | vmx-codec | vmx |
|---|---|---|---:|---:|---:|---:|---:|
| 1080p | LQ | natural | 92 | 169 | 1331 | 398 | 2011 |
| 1080p | SQ | natural | 94 | 292 | 1070 | 424 | 1470 |
| 1080p | HQ | natural | 96 | 559 | 796 | 348 | 1181 |
| 1080p | OMT_LQ | natural | 92 | 169 | 1296 | 426 | 2110 |
| 1080p | OMT_SQ | natural | 95 | 419 | 988 | 342 | 1117 |
| 1080p | OMT_HQ | natural | 96 | 559 | 821 | 393 | 1221 |
| 1080p | LQ | bars | 98 | 22 | 2086 | 482 | 2843 |
| 1080p | SQ | bars | 98 | 22 | 2122 | 481 | 3249 |
| 1080p | HQ | bars | 98 | 24 | 2062 | 462 | 3195 |
| 1080p | OMT_LQ | bars | 98 | 22 | 2167 | 492 | 3139 |
| 1080p | OMT_SQ | bars | 98 | 24 | 2098 | 449 | 2843 |
| 1080p | OMT_HQ | bars | 98 | 24 | 2066 | 482 | 2496 |
| 2160p | LQ | natural | 92 | 596 | 366 | 132 | 544 |
| 2160p | SQ | natural | 94 | 1088 | 300 | 125 | 451 |
| 2160p | HQ | natural | 96 | 2146 | 256 | 101 | 330 |
| 2160p | OMT_LQ | natural | 92 | 596 | 368 | 125 | 572 |
| 2160p | OMT_SQ | natural | 92 | 596 | 358 | 133 | 544 |
| 2160p | OMT_HQ | natural | 94 | 1192 | 348 | 117 | 456 |
| 2160p | LQ | bars | 98 | 76 | 571 | 122 | 734 |
| 2160p | SQ | bars | 98 | 76 | 540 | 115 | 839 |
| 2160p | HQ | bars | 98 | 81 | 540 | 128 | 836 |
| 2160p | OMT_LQ | bars | 98 | 76 | 515 | 119 | 814 |
| 2160p | OMT_SQ | bars | 98 | 76 | 524 | 123 | 796 |
| 2160p | OMT_HQ | bars | 98 | 81 | 530 | 135 | 811 |

### Decode, 1 thread

| Resolution | Profile | Image | q | KB/frame | libvmx | vmx-codec | vmx |
|---|---|---|---:|---:|---:|---:|---:|
| 1080p | LQ | natural | 92 | 169 | 407 | 269 | 372 |
| 1080p | SQ | natural | 94 | 292 | 285 | 193 | 292 |
| 1080p | HQ | natural | 96 | 559 | 195 | 106 | 188 |
| 1080p | OMT_LQ | natural | 92 | 169 | 410 | 277 | 377 |
| 1080p | OMT_SQ | natural | 95 | 419 | 242 | 151 | 231 |
| 1080p | OMT_HQ | natural | 96 | 559 | 186 | 114 | 200 |
| 1080p | LQ | bars | 98 | 22 | 1202 | 757 | 914 |
| 1080p | SQ | bars | 98 | 22 | 1276 | 788 | 958 |
| 1080p | HQ | bars | 98 | 24 | 1245 | 775 | 957 |
| 1080p | OMT_LQ | bars | 98 | 22 | 1265 | 777 | 984 |
| 1080p | OMT_SQ | bars | 98 | 24 | 1244 | 751 | 956 |
| 1080p | OMT_HQ | bars | 98 | 24 | 1202 | 784 | 988 |
| 2160p | LQ | natural | 92 | 596 | 117 | 75 | 92 |
| 2160p | SQ | natural | 94 | 1088 | 74 | 50 ✗ | 62 |
| 2160p | HQ | natural | 96 | 2146 | 44 ✗ | 27 ✗ | 46 ✗ |
| 2160p | OMT_LQ | natural | 92 | 596 | 110 | 75 | 103 |
| 2160p | OMT_SQ | natural | 92 | 596 | 119 | 75 | 101 |
| 2160p | OMT_HQ | natural | 94 | 1192 | 84 | 52 ✗ | 62 |
| 2160p | LQ | bars | 98 | 76 | 368 | 181 | 263 |
| 2160p | SQ | bars | 98 | 76 | 355 | 189 | 248 |
| 2160p | HQ | bars | 98 | 81 | 349 | 170 | 250 |
| 2160p | OMT_LQ | bars | 98 | 76 | 366 | 186 | 244 |
| 2160p | OMT_SQ | bars | 98 | 76 | 346 | 179 | 234 |
| 2160p | OMT_HQ | bars | 98 | 81 | 346 | 189 | 246 |

### Decode, 8 threads

| Resolution | Profile | Image | q | KB/frame | libvmx | vmx-codec | vmx |
|---|---|---|---:|---:|---:|---:|---:|
| 1080p | LQ | natural | 92 | 169 | 1328 | 579 | 2048 |
| 1080p | SQ | natural | 94 | 292 | 947 | 538 | 1327 |
| 1080p | HQ | natural | 96 | 559 | 618 | 388 | 1130 |
| 1080p | OMT_LQ | natural | 92 | 169 | 1335 | 635 | 2169 |
| 1080p | OMT_SQ | natural | 95 | 419 | 777 | 462 | 1071 |
| 1080p | OMT_HQ | natural | 96 | 559 | 614 | 424 | 1231 |
| 1080p | LQ | bars | 98 | 22 | 4220 | 780 | 3692 |
| 1080p | SQ | bars | 98 | 22 | 4521 | 780 | 4058 |
| 1080p | HQ | bars | 98 | 24 | 4624 | 792 | 4497 |
| 1080p | OMT_LQ | bars | 98 | 22 | 4405 | 780 | 4291 |
| 1080p | OMT_SQ | bars | 98 | 24 | 4411 | 734 | 4380 |
| 1080p | OMT_HQ | bars | 98 | 24 | 4301 | 781 | 4397 |
| 2160p | LQ | natural | 92 | 596 | 430 | 193 | 636 |
| 2160p | SQ | natural | 94 | 1088 | 283 | 173 | 477 |
| 2160p | HQ | natural | 96 | 2146 | 185 | 129 | 209 |
| 2160p | OMT_LQ | natural | 92 | 596 | 334 | 174 | 685 |
| 2160p | OMT_SQ | natural | 92 | 596 | 423 | 202 | 645 |
| 2160p | OMT_HQ | natural | 94 | 1192 | 296 | 163 | 467 |
| 2160p | LQ | bars | 98 | 76 | 1072 | 226 | 1068 |
| 2160p | SQ | bars | 98 | 76 | 1223 | 242 | 1206 |
| 2160p | HQ | bars | 98 | 81 | 1290 | 250 | 1583 |
| 2160p | OMT_LQ | bars | 98 | 76 | 1261 | 242 | 1537 |
| 2160p | OMT_SQ | bars | 98 | 76 | 1234 | 241 | 1523 |
| 2160p | OMT_HQ | bars | 98 | 81 | 1355 | 253 | 1502 |

## x86-64 tables with the 128-bit path

### Encode, 1 thread
| Resolution | Profile | Image | q | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx | highest |
|---|---|---|---|---|---|---|---|---|---|
| 1080p | LQ | natural | 92 | 168,577 | 379 (±4%) | 271 (±4%) | 178 (±9%) | 322 (±4%) | libvmx |
| 1080p | SQ | natural | 94 | 292,310 | 328 (±3%) | 234 (±3%) | 153 (±3%) | 279 (±2%) | libvmx |
| 1080p | HQ | natural | 96 | 559,463 | 263 (±4%) | 178 (±4%) | 122 (±2%) | 179 (±5%) | libvmx |
| 1080p | OMT_LQ | natural | 92 | 168,577 | 386 (±5%) | 272 (±3%) | 180 (±3%) | 339 (±12%) | libvmx |
| 1080p | OMT_SQ | natural | 95 | 418,883 | 318 (±14%) | 222 (±1%) | 141 (±8%) | 225 (±3%) | libvmx |
| 1080p | OMT_HQ | natural | 96 | 559,463 | 268 (±2%) | 190 (±3%) | 131 (±2%) | 200 (±1%) | libvmx |
| 1080p | LQ | bars | 98 | 22,008 | 609 (±4%) | 409 (±1%) | 221 (±3%) | 514 (±3%) | libvmx |
| 1080p | SQ | bars | 98 | 22,008 | 607 (±3%) | 404 (±2%) | 224 (±1%) | 511 (±1%) | libvmx |
| 1080p | HQ | bars | 98 | 24,303 | 649 (±3%) | 407 (±1%) | 219 (±3%) | 506 (±6%) | libvmx |
| 1080p | OMT_LQ | bars | 98 | 22,008 | 641 (±3%) | 409 (±4%) | 221 (±2%) | 517 (±8%) | libvmx |
| 1080p | OMT_SQ | bars | 98 | 24,303 | 634 (±4%) | 414 (±1%) | 224 (±1%) | 506 (±1%) | libvmx |
| 1080p | OMT_HQ | bars | 98 | 24,303 | 597 (±8%) | 412 (±2%) | 221 (±3%) | 510 (±3%) | libvmx |
| 2160p | LQ | natural | 92 | 595,921 | 102 (±3%) | 65 (±6%) | 43 ✗ (±5%) | 78 (±9%) | libvmx |
| 2160p | SQ | natural | 94 | 1,088,322 | 82 (±6%) | 56 ✗ (±4%) | 38 ✗ (±5%) | 68 (±8%) | libvmx |
| 2160p | HQ | natural | 96 | 2,145,917 | 64 (±4%) | 43 ✗ (±4%) | 32 ✗ (±5%) | 48 ✗ (±4%) | libvmx |
| 2160p | OMT_LQ | natural | 92 | 595,921 | 102 (±3%) | 56 ✗ (±14%) | 41 ✗ (±18%) | 86 (±5%) | libvmx |
| 2160p | OMT_SQ | natural | 92 | 595,921 | 104 (±5%) | 71 (±1%) | 45 ✗ (±5%) | 86 (±7%) | libvmx |
| 2160p | OMT_HQ | natural | 94 | 1,192,159 | 98 (±1%) | 65 (±1%) | 42 ✗ (±1%) | 64 (±4%) | libvmx |
| 2160p | LQ | bars | 98 | 76,145 | 160 (±6%) | 104 (±8%) | 52 ✗ (±11%) | 117 (±7%) | libvmx |
| 2160p | SQ | bars | 98 | 76,145 | 154 (±4%) | 108 (±1%) | 52 ✗ (±7%) | 118 (±3%) | libvmx |
| 2160p | HQ | bars | 98 | 80,733 | 153 (±3%) | 102 (±10%) | 46 ✗ (±16%) | 120 (±4%) | libvmx |
| 2160p | OMT_LQ | bars | 98 | 76,145 | 154 (±6%) | 99 (±4%) | 53 ✗ (±2%) | 118 (±1%) | libvmx |
| 2160p | OMT_SQ | bars | 98 | 76,145 | 157 (±4%) | 97 (±11%) | 48 ✗ (±9%) | 115 (±2%) | libvmx |
| 2160p | OMT_HQ | bars | 98 | 80,733 | 148 (±6%) | 101 (±2%) | 50 ✗ (±6%) | 116 (±6%) | libvmx |

### Encode, 8 threads
| Resolution | Profile | Image | q | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx | highest |
|---|---|---|---|---|---|---|---|---|---|
| 1080p | LQ | natural | 92 | 168,577 | 1331 (±3%) | 906 (±5%) | 398 (±5%) | 2011 (±3%) | vmx |
| 1080p | SQ | natural | 94 | 292,310 | 1070 (±4%) | 791 (±5%) | 424 (±5%) | 1470 (±8%) | vmx |
| 1080p | HQ | natural | 96 | 559,463 | 796 (±5%) | 590 (±5%) | 348 (±7%) | 1181 (±4%) | vmx |
| 1080p | OMT_LQ | natural | 92 | 168,577 | 1296 (±3%) | 863 (±8%) | 426 (±6%) | 2110 (±2%) | vmx |
| 1080p | OMT_SQ | natural | 95 | 418,883 | 988 (±6%) | 721 (±1%) | 342 (±60%) | 1117 (±9%) | vmx |
| 1080p | OMT_HQ | natural | 96 | 559,463 | 821 (±3%) | 606 (±3%) | 393 (±5%) | 1221 (±4%) | vmx |
| 1080p | LQ | bars | 98 | 22,008 | 2086 (±4%) | 1404 (±11%) | 482 (±5%) | 2843 (±8%) | vmx |
| 1080p | SQ | bars | 98 | 22,008 | 2122 (±7%) | 1425 (±10%) | 481 (±3%) | 3249 (±4%) | vmx |
| 1080p | HQ | bars | 98 | 24,303 | 2062 (±10%) | 1421 (±5%) | 462 (±1%) | 3195 (±2%) | vmx |
| 1080p | OMT_LQ | bars | 98 | 22,008 | 2167 (±3%) | 1421 (±3%) | 492 (±6%) | 3139 (±5%) | vmx |
| 1080p | OMT_SQ | bars | 98 | 24,303 | 2098 (±2%) | 1398 (±7%) | 449 (±9%) | 2843 (±5%) | vmx |
| 1080p | OMT_HQ | bars | 98 | 24,303 | 2066 (±3%) | 1432 (±5%) | 482 (±5%) | 2496 (±5%) | vmx |
| 2160p | LQ | natural | 92 | 595,921 | 366 (±6%) | 257 (±6%) | 132 (±6%) | 544 (±6%) | vmx |
| 2160p | SQ | natural | 94 | 1,088,322 | 300 (±9%) | 247 (±7%) | 125 (±5%) | 451 (±4%) | vmx |
| 2160p | HQ | natural | 96 | 2,145,917 | 256 (±6%) | 220 (±13%) | 101 (±8%) | 330 (±5%) | vmx |
| 2160p | OMT_LQ | natural | 92 | 595,921 | 368 (±3%) | 245 (±8%) | 125 (±15%) | 572 (±7%) | vmx |
| 2160p | OMT_SQ | natural | 92 | 595,921 | 358 (±12%) | 257 (±4%) | 133 (±4%) | 544 (±3%) | vmx |
| 2160p | OMT_HQ | natural | 94 | 1,192,159 | 348 (±5%) | 253 (±4%) | 117 (±8%) | 456 (±3%) | vmx |
| 2160p | LQ | bars | 98 | 76,145 | 571 (±4%) | 375 (±5%) | 122 (±2%) | 734 (±9%) | vmx |
| 2160p | SQ | bars | 98 | 76,145 | 540 (±3%) | 340 (±6%) | 115 (±9%) | 839 (±4%) | vmx |
| 2160p | HQ | bars | 98 | 80,733 | 540 (±1%) | 368 (±3%) | 128 (±6%) | 836 (±3%) | vmx |
| 2160p | OMT_LQ | bars | 98 | 76,145 | 515 (±5%) | 352 (±5%) | 119 (±7%) | 814 (±7%) | vmx |
| 2160p | OMT_SQ | bars | 98 | 76,145 | 524 (±10%) | 360 (±3%) | 123 (±6%) | 796 (±2%) | vmx |
| 2160p | OMT_HQ | bars | 98 | 80,733 | 530 (±2%) | 352 (±7%) | 135 (±7%) | 811 (±8%) | vmx |

### Decode, 1 thread
| Resolution | Profile | Image | q | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx | highest |
|---|---|---|---|---|---|---|---|---|---|
| 1080p | LQ | natural | 92 | 168,577 | 407 (±6%) | 364 (±2%) | 269 (±5%) | 372 (±2%) | libvmx |
| 1080p | SQ | natural | 94 | 292,310 | 285 (±6%) | 265 (±3%) | 193 (±1%) | 292 (±1%) | vmx |
| 1080p | HQ | natural | 96 | 559,463 | 195 (±2%) | 163 (±4%) | 106 (±2%) | 188 (±5%) | libvmx |
| 1080p | OMT_LQ | natural | 92 | 168,577 | 410 (±2%) | 361 (±4%) | 277 (±6%) | 377 (±5%) | libvmx |
| 1080p | OMT_SQ | natural | 95 | 418,883 | 242 (±2%) | 216 (±1%) | 151 (±1%) | 231 (±2%) | libvmx |
| 1080p | OMT_HQ | natural | 96 | 559,463 | 186 (±4%) | 167 (±8%) | 114 (±1%) | 200 (±1%) | vmx |
| 1080p | LQ | bars | 98 | 22,008 | 1202 (±7%) | 1245 (±6%) | 757 (±2%) | 914 (±4%) | libvmx 128-bit |
| 1080p | SQ | bars | 98 | 22,008 | 1276 (±9%) | 1303 (±4%) | 788 (±3%) | 958 (±1%) | libvmx 128-bit |
| 1080p | HQ | bars | 98 | 24,303 | 1245 (±1%) | 1291 (±2%) | 775 (±1%) | 957 (±1%) | libvmx 128-bit |
| 1080p | OMT_LQ | bars | 98 | 22,008 | 1265 (±10%) | 1256 (±3%) | 777 (±1%) | 984 (±4%) | libvmx |
| 1080p | OMT_SQ | bars | 98 | 24,303 | 1244 (±7%) | 1288 (±4%) | 751 (±5%) | 956 (±1%) | libvmx 128-bit |
| 1080p | OMT_HQ | bars | 98 | 24,303 | 1202 (±4%) | 1270 (±4%) | 784 (±1%) | 988 (±1%) | libvmx 128-bit |
| 2160p | LQ | natural | 92 | 595,921 | 117 (±2%) | 101 (±13%) | 75 (±1%) | 92 (±11%) | libvmx |
| 2160p | SQ | natural | 94 | 1,088,322 | 74 (±8%) | 60 (±7%) | 50 ✗ (±2%) | 62 (±7%) | libvmx |
| 2160p | HQ | natural | 96 | 2,145,917 | 44 ✗ (±4%) | 38 ✗ (±3%) | 27 ✗ (±1%) | 46 ✗ (±4%) | vmx |
| 2160p | OMT_LQ | natural | 92 | 595,921 | 110 (±6%) | 96 (±5%) | 75 (±2%) | 103 (±3%) | libvmx |
| 2160p | OMT_SQ | natural | 92 | 595,921 | 119 (±2%) | 102 (±5%) | 75 (±1%) | 101 (±3%) | libvmx |
| 2160p | OMT_HQ | natural | 94 | 1,192,159 | 84 (±1%) | 73 (±4%) | 52 ✗ (±2%) | 62 (±2%) | libvmx |
| 2160p | LQ | bars | 98 | 76,145 | 368 (±12%) | 364 (±5%) | 181 (±11%) | 263 (±1%) | libvmx |
| 2160p | SQ | bars | 98 | 76,145 | 355 (±6%) | 375 (±8%) | 189 (±10%) | 248 (±10%) | libvmx 128-bit |
| 2160p | HQ | bars | 98 | 80,733 | 349 (±3%) | 363 (±3%) | 170 (±7%) | 250 (±2%) | libvmx 128-bit |
| 2160p | OMT_LQ | bars | 98 | 76,145 | 366 (±9%) | 359 (±2%) | 186 (±9%) | 244 (±7%) | libvmx |
| 2160p | OMT_SQ | bars | 98 | 76,145 | 346 (±4%) | 357 (±2%) | 179 (±6%) | 234 (±17%) | libvmx 128-bit |
| 2160p | OMT_HQ | bars | 98 | 80,733 | 346 (±4%) | 342 (±8%) | 189 (±1%) | 246 (±2%) | libvmx |

### Decode, 8 threads
| Resolution | Profile | Image | q | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx | highest |
|---|---|---|---|---|---|---|---|---|---|
| 1080p | LQ | natural | 92 | 168,577 | 1328 (±5%) | 1197 (±7%) | 579 (±2%) | 2048 (±6%) | vmx |
| 1080p | SQ | natural | 94 | 292,310 | 947 (±5%) | 904 (±2%) | 538 (±4%) | 1327 (±16%) | vmx |
| 1080p | HQ | natural | 96 | 559,463 | 618 (±11%) | 540 (±26%) | 388 (±1%) | 1130 (±4%) | vmx |
| 1080p | OMT_LQ | natural | 92 | 168,577 | 1335 (±4%) | 1153 (±2%) | 635 (±2%) | 2169 (±5%) | vmx |
| 1080p | OMT_SQ | natural | 95 | 418,883 | 777 (±7%) | 664 (±7%) | 462 (±8%) | 1071 (±6%) | vmx |
| 1080p | OMT_HQ | natural | 96 | 559,463 | 614 (±3%) | 559 (±1%) | 424 (±3%) | 1231 (±4%) | vmx |
| 1080p | LQ | bars | 98 | 22,008 | 4220 (±3%) | 4660 (±4%) | 780 (±4%) | 3692 (±8%) | libvmx 128-bit |
| 1080p | SQ | bars | 98 | 22,008 | 4521 (±2%) | 4652 (±4%) | 780 (±4%) | 4058 (±13%) | libvmx 128-bit |
| 1080p | HQ | bars | 98 | 24,303 | 4624 (±5%) | 4063 (±5%) | 792 (±4%) | 4497 (±3%) | libvmx |
| 1080p | OMT_LQ | bars | 98 | 22,008 | 4405 (±3%) | 4579 (±4%) | 780 (±1%) | 4291 (±2%) | libvmx 128-bit |
| 1080p | OMT_SQ | bars | 98 | 24,303 | 4411 (±3%) | 4674 (±3%) | 734 (±4%) | 4380 (±11%) | libvmx 128-bit |
| 1080p | OMT_HQ | bars | 98 | 24,303 | 4301 (±2%) | 4460 (±2%) | 781 (±1%) | 4397 (±3%) | libvmx 128-bit |
| 2160p | LQ | natural | 92 | 595,921 | 430 (±6%) | 362 (±6%) | 193 (±3%) | 636 (±3%) | vmx |
| 2160p | SQ | natural | 94 | 1,088,322 | 283 (±21%) | 256 (±7%) | 173 (±3%) | 477 (±3%) | vmx |
| 2160p | HQ | natural | 96 | 2,145,917 | 185 (±6%) | 207 (±16%) | 129 (±5%) | 209 (±50%) | vmx |
| 2160p | OMT_LQ | natural | 92 | 595,921 | 334 (±61%) | 310 (±18%) | 174 (±8%) | 685 (±5%) | vmx |
| 2160p | OMT_SQ | natural | 92 | 595,921 | 423 (±4%) | 382 (±9%) | 202 (±2%) | 645 (±7%) | vmx |
| 2160p | OMT_HQ | natural | 94 | 1,192,159 | 296 (±9%) | 273 (±5%) | 163 (±6%) | 467 (±7%) | vmx |
| 2160p | LQ | bars | 98 | 76,145 | 1072 (±8%) | 1052 (±3%) | 226 (±5%) | 1068 (±7%) | libvmx |
| 2160p | SQ | bars | 98 | 76,145 | 1223 (±6%) | 914 (±15%) | 242 (±4%) | 1206 (±11%) | libvmx |
| 2160p | HQ | bars | 98 | 80,733 | 1290 (±17%) | 1179 (±9%) | 250 (±2%) | 1583 (±3%) | vmx |
| 2160p | OMT_LQ | bars | 98 | 76,145 | 1261 (±20%) | 1279 (±7%) | 242 (±2%) | 1537 (±6%) | vmx |
| 2160p | OMT_SQ | bars | 98 | 76,145 | 1234 (±12%) | 1283 (±8%) | 241 (±4%) | 1523 (±4%) | vmx |
| 2160p | OMT_HQ | bars | 98 | 80,733 | 1355 (±18%) | 1214 (±4%) | 253 (±2%) | 1502 (±5%) | vmx |


## ARM / NEON

The x86-64 host above does not run NEON. [`.github/workflows/bench.yml`](.github/workflows/bench.yml) runs the same harness on `ubuntu-24.04-arm` and `macos-latest`. Trigger it with `workflow_dispatch`. The branch that adds the workflow also runs it once on push, because GitHub only lists a new workflow for manual dispatch after the file is on the default branch.

On aarch64 the harness builds libvmx the way `build/buildlinuxarm64.sh` does (`vmxcodec_arm.cpp`, NEON through sse2neon, no `-mavx2`). That translation unit does not compile the 256-bit functions, so there is one libvmx column rather than an AVX2 column and a 128-bit column. vmx-codec uses its compile-time NEON code. vmx reports its runtime path, which on AArch64 is NEON.

10-bit P216 / PA16 stays unmeasured on these hosts for the same reason as on x86-64: `vmx` 0.2.1 returns `invalid parameters` for those four entry points.

Runner results are not in this revision yet. The workflow uploads `results.csv`, `cpuinfo.txt`, and `host.txt` as artifacts. This section will be replaced with those medians and the runner's CPU description when a run finishes. If a runner cannot complete the job, the failure is recorded here instead of a table.

## Reproducing

```bash
cd bench
cargo run --release > results.csv
```

`clang++`, `git`, and `ar` are required. Useful filters: `--quick`, `--res 1080`, `--prof HQ`, `--img natural`, `--impl libvmx`, `--threads 1`, `--thread-counts 1,8`, `--reps`, `--secs`. `VMX_BENCH_META=host.txt` writes the host description that the harness prints to stderr. See [`bench/README.md`](bench/README.md).
