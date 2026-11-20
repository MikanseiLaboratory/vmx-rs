# Benchmark

Measurements of three VMX1 implementations on the same 8-bit UYVY frames. The tables are medians from one run. They are not a ranking, and a gap inside the run-to-run band described below is not a stable difference.

Fps columns, left to right, are libvmx (official), vmx-codec (Saul), and vmx — the x86-64 detail tables insert libvmx 128-bit after the AVX2 column — and **bold** is the fastest median on that row (a tie bolds each tied value).

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
- Images: `natural` (smooth gradients, low-frequency texture, about ±6 LSB of noise, a few hard edges) and `bars` (75% colour bars, eight flat regions, no noise). The bytes are in [`bench/images/`](bench/images/) (gzip of the UYVY frames, SHA-256 of the uncompressed frames, and 480×270 PNG previews).
- Quality: libvmx's own rate control is run for 60 frames on that picture. The `q` it settles on is then fixed. Every timed encode calls set-quality with that `q`. DC shift and minimum quality stay at the profile defaults. This is not a rate-control benchmark; the profile still decides which `q` the picture settles at, and therefore how large the frame is.
- Encode is colour conversion, coding, and serialize (`VMX_EncodeUYVY` + `VMX_SaveTo`, `Encoder::encode_into` into a reused `Vec`, `Codec::encode_uyvy` + `save_to`).
- Decode is load plus decode to UYVY. Every decoder receives the same packet, produced by libvmx's 128-bit path at the fixed `q`.
- Input and output buffers are allocated before the timer starts. The timed region does not copy the packet into a new buffer.
- Three warmup frames, then seven timed runs of at least 0.3 s each. The published value is the median. The CSV also has the min and max of those seven runs.
- Thread counts: 1 and 8, via each implementation's thread setter. 8 is the vCPU count of the machine.

| 1080p natural | 1080p bars | 2160p natural | 2160p bars |
|---|---|---|---|
| ![1080p natural](bench/images/1080p-natural.png) | ![1080p bars](bench/images/1080p-bars.png) | ![2160p natural](bench/images/2160p-natural.png) | ![2160p bars](bench/images/2160p-bars.png) |

On every one of the 384 rows the encoded bytes matched the libvmx 128-bit packet, and the decoded pixels matched.

In the tables, `✗` means the median is below 60 fps. The `±` figure is half of `(max − min) / median` for the seven runs, rounded to the nearest percent. It is the spread inside that sample, not the 10–20% movement between separate runs. `bytes/frame` is the exact encoded size.

10-bit P216 and PA16 are not in the matrix. `vmx` 0.2.1 returns `invalid parameters` from `encode_p216`, `decode_p216`, `encode_pa16`, and `decode_pa16`. libvmx and vmx-codec both implement those formats; this file does not compare them.

The profile cell in the result tables includes the fixed q, as in `HQ (96)`.

## Profiles

libvmx (`31724bb`, `VMX_BITRATE_TABLE` in `src/vmxcodec_common.h`) picks one row per picture. Rows are ordered with the largest height first. The first row with the same profile id and a height threshold less than or equal to the picture height is the one that applies. The ids are LQ 33, SQ 66, HQ 99, OMT_LQ 133, OMT_SQ 166, and OMT_HQ 199. They are selectors, not positions on one quality scale.

A row holds a target bitrate in Mbit/s, a DC shift, a minimum quality, and a default thread count. `VMX_CalculateBitrate` turns the target into a byte window for a nominal 60 fps frame: `target / (60 × 8) × 1048576`, then ×0.95 and ×1.05, truncated to an integer. After `VMX_SaveTo`, quality steps toward that window and does not go below the row's minimum. The ceiling is `VMX_MAXQ` (98). The header comment on DC shift is the precision of the DC coefficient: 0 keeps 11 bits, 1 keeps 10, 2 keeps 9, and 3 keeps 8. `VMX_SetQuality` immediately before an encode overrides the bitrate step for that frame. It does not change the DC shift or the minimum.

The harness let that rate control run for 60 frames, then called set-quality with the settled q on every timed encode. It also set the thread count itself (1, and the CPU count), so the default thread count in the table is not the thread count in the fps tables.

Only the 1080 and 2160 rows were used. The same table has rows for 4320, 1440, 720, and a height-0 fallback.

### Height 1080

The 1080 row applies when `1080 ≤ height < 1440`.

| Profile | Target | Byte window | DC shift | Minimum quality | Default threads |
|---|---:|---:|---|---:|---:|
| HQ | 260 Mbit/s | 539,579–596,377 | 0 (11-bit DC) | 80 | 2 |
| OMT_HQ | 260 Mbit/s | 539,579–596,377 | 0 (11-bit DC) | 52 | 2 |
| SQ | 130 Mbit/s | 269,789–298,188 | 3 (8-bit DC) | 60 | 2 |
| OMT_SQ | 200 Mbit/s | 415,061–458,751 | 0 (11-bit DC) | 52 | 2 |
| LQ | 86 Mbit/s | 178,476–197,263 | 3 (8-bit DC) | 60 | 2 |
| OMT_LQ | 86 Mbit/s | 178,476–197,263 | 3 (8-bit DC) | 52 | 2 |

HQ and OMT_HQ share the target and the DC shift. LQ and OMT_LQ share the target and the DC shift. In both pairs the OMT row's quality floor is 52 (`VMX_OMT_MINQ`) instead of 80 or 60. SQ and OMT_SQ are not the same setting: OMT_SQ targets 200 Mbit/s with an 11-bit DC, and SQ targets 130 Mbit/s with an 8-bit DC.

### Height 2160

| Profile | Target | Byte window | DC shift | Minimum quality | Default threads |
|---|---:|---:|---|---:|---:|
| HQ | 800 Mbit/s | 1,660,245–1,835,007 | 0 (11-bit DC) | 80 | 4 |
| OMT_HQ | 600 Mbit/s | 1,245,184–1,376,256 | 0 (11-bit DC) | 52 | 4 |
| SQ | 400 Mbit/s | 830,122–917,503 | 3 (8-bit DC) | 60 | 4 |
| OMT_SQ | 300 Mbit/s | 622,592–688,128 | 3 (8-bit DC) | 52 | 4 |
| LQ | 266 Mbit/s | 552,031–610,140 | 3 (8-bit DC) | 60 | 4 |
| OMT_LQ | 200 Mbit/s | 415,061–458,751 | 3 (8-bit DC) | 52 | 4 |

At 2160, each OMT profile targets a lower bitrate than the non-OMT profile of the same LQ/SQ/HQ name, and its quality floor is 52 instead of 80 (HQ) or 60 (LQ and SQ). DC shift matches inside each pair.

Every settled q in the fps tables is 92, 94, 95, 96, or 98, which is above every floor in these two tables, so the floor did not clamp the timed frames. The DC shift still applies at that fixed q. Where a pair shares both the settled q and the DC shift, the encoded size in these tables matches, because the bitrate window is not read once q is pinned.

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

Read the ratios next to the noise. A geometric mean of 1.44 on 8-thread encode is outside a 10–20% band. A single row at 1.13, or the 8-thread bars decode mean of 1.04, is not. Several 8-thread bars decode rows are higher for libvmx's 128-bit path than for either AVX2 libvmx or vmx; those cells are the bold ones in the detail tables.

The single-thread rows where vmx is behind libvmx, and the flat-bar decode rows, are the ones discussed as follow-up work in [issue 39](https://github.com/MikanseiLaboratory/vmx-rs/issues/39). This document does not change the codec.

## x86-64 tables

These four tables are libvmx's AVX2 path, vmx-codec, and vmx. The next section adds libvmx's 128-bit path and the spread inside the seven runs.

### Encode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **379** | 178 | 322 |
| 1080p | SQ (94) | natural | 292,310 | **328** | 153 | 279 |
| 1080p | HQ (96) | natural | 559,463 | **263** | 122 | 179 |
| 1080p | OMT_LQ (92) | natural | 168,577 | **386** | 180 | 339 |
| 1080p | OMT_SQ (95) | natural | 418,883 | **318** | 141 | 225 |
| 1080p | OMT_HQ (96) | natural | 559,463 | **268** | 131 | 200 |
| 1080p | LQ (98) | bars | 22,008 | **609** | 221 | 514 |
| 1080p | SQ (98) | bars | 22,008 | **607** | 224 | 511 |
| 1080p | HQ (98) | bars | 24,303 | **649** | 219 | 506 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **641** | 221 | 517 |
| 1080p | OMT_SQ (98) | bars | 24,303 | **634** | 224 | 506 |
| 1080p | OMT_HQ (98) | bars | 24,303 | **597** | 221 | 510 |
| 2160p | LQ (92) | natural | 595,921 | **102** | 43 ✗ | 78 |
| 2160p | SQ (94) | natural | 1,088,322 | **82** | 38 ✗ | 68 |
| 2160p | HQ (96) | natural | 2,145,917 | **64** | 32 ✗ | 48 ✗ |
| 2160p | OMT_LQ (92) | natural | 595,921 | **102** | 41 ✗ | 86 |
| 2160p | OMT_SQ (92) | natural | 595,921 | **104** | 45 ✗ | 86 |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **98** | 42 ✗ | 64 |
| 2160p | LQ (98) | bars | 76,145 | **160** | 52 ✗ | 117 |
| 2160p | SQ (98) | bars | 76,145 | **154** | 52 ✗ | 118 |
| 2160p | HQ (98) | bars | 80,733 | **153** | 46 ✗ | 120 |
| 2160p | OMT_LQ (98) | bars | 76,145 | **154** | 53 ✗ | 118 |
| 2160p | OMT_SQ (98) | bars | 76,145 | **157** | 48 ✗ | 115 |
| 2160p | OMT_HQ (98) | bars | 80,733 | **148** | 50 ✗ | 116 |

### Encode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | 1331 | 398 | **2011** |
| 1080p | SQ (94) | natural | 292,310 | 1070 | 424 | **1470** |
| 1080p | HQ (96) | natural | 559,463 | 796 | 348 | **1181** |
| 1080p | OMT_LQ (92) | natural | 168,577 | 1296 | 426 | **2110** |
| 1080p | OMT_SQ (95) | natural | 418,883 | 988 | 342 | **1117** |
| 1080p | OMT_HQ (96) | natural | 559,463 | 821 | 393 | **1221** |
| 1080p | LQ (98) | bars | 22,008 | 2086 | 482 | **2843** |
| 1080p | SQ (98) | bars | 22,008 | 2122 | 481 | **3249** |
| 1080p | HQ (98) | bars | 24,303 | 2062 | 462 | **3195** |
| 1080p | OMT_LQ (98) | bars | 22,008 | 2167 | 492 | **3139** |
| 1080p | OMT_SQ (98) | bars | 24,303 | 2098 | 449 | **2843** |
| 1080p | OMT_HQ (98) | bars | 24,303 | 2066 | 482 | **2496** |
| 2160p | LQ (92) | natural | 595,921 | 366 | 132 | **544** |
| 2160p | SQ (94) | natural | 1,088,322 | 300 | 125 | **451** |
| 2160p | HQ (96) | natural | 2,145,917 | 256 | 101 | **330** |
| 2160p | OMT_LQ (92) | natural | 595,921 | 368 | 125 | **572** |
| 2160p | OMT_SQ (92) | natural | 595,921 | 358 | 133 | **544** |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | 348 | 117 | **456** |
| 2160p | LQ (98) | bars | 76,145 | 571 | 122 | **734** |
| 2160p | SQ (98) | bars | 76,145 | 540 | 115 | **839** |
| 2160p | HQ (98) | bars | 80,733 | 540 | 128 | **836** |
| 2160p | OMT_LQ (98) | bars | 76,145 | 515 | 119 | **814** |
| 2160p | OMT_SQ (98) | bars | 76,145 | 524 | 123 | **796** |
| 2160p | OMT_HQ (98) | bars | 80,733 | 530 | 135 | **811** |

### Decode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **407** | 269 | 372 |
| 1080p | SQ (94) | natural | 292,310 | 285 | 193 | **292** |
| 1080p | HQ (96) | natural | 559,463 | **195** | 106 | 188 |
| 1080p | OMT_LQ (92) | natural | 168,577 | **410** | 277 | 377 |
| 1080p | OMT_SQ (95) | natural | 418,883 | **242** | 151 | 231 |
| 1080p | OMT_HQ (96) | natural | 559,463 | 186 | 114 | **200** |
| 1080p | LQ (98) | bars | 22,008 | **1202** | 757 | 914 |
| 1080p | SQ (98) | bars | 22,008 | **1276** | 788 | 958 |
| 1080p | HQ (98) | bars | 24,303 | **1245** | 775 | 957 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **1265** | 777 | 984 |
| 1080p | OMT_SQ (98) | bars | 24,303 | **1244** | 751 | 956 |
| 1080p | OMT_HQ (98) | bars | 24,303 | **1202** | 784 | 988 |
| 2160p | LQ (92) | natural | 595,921 | **117** | 75 | 92 |
| 2160p | SQ (94) | natural | 1,088,322 | **74** | 50 ✗ | 62 |
| 2160p | HQ (96) | natural | 2,145,917 | 44 ✗ | 27 ✗ | **46** ✗ |
| 2160p | OMT_LQ (92) | natural | 595,921 | **110** | 75 | 103 |
| 2160p | OMT_SQ (92) | natural | 595,921 | **119** | 75 | 101 |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **84** | 52 ✗ | 62 |
| 2160p | LQ (98) | bars | 76,145 | **368** | 181 | 263 |
| 2160p | SQ (98) | bars | 76,145 | **355** | 189 | 248 |
| 2160p | HQ (98) | bars | 80,733 | **349** | 170 | 250 |
| 2160p | OMT_LQ (98) | bars | 76,145 | **366** | 186 | 244 |
| 2160p | OMT_SQ (98) | bars | 76,145 | **346** | 179 | 234 |
| 2160p | OMT_HQ (98) | bars | 80,733 | **346** | 189 | 246 |

### Decode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | 1328 | 579 | **2048** |
| 1080p | SQ (94) | natural | 292,310 | 947 | 538 | **1327** |
| 1080p | HQ (96) | natural | 559,463 | 618 | 388 | **1130** |
| 1080p | OMT_LQ (92) | natural | 168,577 | 1335 | 635 | **2169** |
| 1080p | OMT_SQ (95) | natural | 418,883 | 777 | 462 | **1071** |
| 1080p | OMT_HQ (96) | natural | 559,463 | 614 | 424 | **1231** |
| 1080p | LQ (98) | bars | 22,008 | **4220** | 780 | 3692 |
| 1080p | SQ (98) | bars | 22,008 | **4521** | 780 | 4058 |
| 1080p | HQ (98) | bars | 24,303 | **4624** | 792 | 4497 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **4405** | 780 | 4291 |
| 1080p | OMT_SQ (98) | bars | 24,303 | **4411** | 734 | 4380 |
| 1080p | OMT_HQ (98) | bars | 24,303 | 4301 | 781 | **4397** |
| 2160p | LQ (92) | natural | 595,921 | 430 | 193 | **636** |
| 2160p | SQ (94) | natural | 1,088,322 | 283 | 173 | **477** |
| 2160p | HQ (96) | natural | 2,145,917 | 185 | 129 | **209** |
| 2160p | OMT_LQ (92) | natural | 595,921 | 334 | 174 | **685** |
| 2160p | OMT_SQ (92) | natural | 595,921 | 423 | 202 | **645** |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | 296 | 163 | **467** |
| 2160p | LQ (98) | bars | 76,145 | **1072** | 226 | 1068 |
| 2160p | SQ (98) | bars | 76,145 | **1223** | 242 | 1206 |
| 2160p | HQ (98) | bars | 80,733 | 1290 | 250 | **1583** |
| 2160p | OMT_LQ (98) | bars | 76,145 | 1261 | 242 | **1537** |
| 2160p | OMT_SQ (98) | bars | 76,145 | 1234 | 241 | **1523** |
| 2160p | OMT_HQ (98) | bars | 80,733 | 1355 | 253 | **1502** |

## x86-64 tables with the 128-bit path

### Encode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **379** (±4%) | 271 (±4%) | 178 (±9%) | 322 (±4%) |
| 1080p | SQ (94) | natural | 292,310 | **328** (±3%) | 234 (±3%) | 153 (±3%) | 279 (±2%) |
| 1080p | HQ (96) | natural | 559,463 | **263** (±4%) | 178 (±4%) | 122 (±2%) | 179 (±5%) |
| 1080p | OMT_LQ (92) | natural | 168,577 | **386** (±5%) | 272 (±3%) | 180 (±3%) | 339 (±12%) |
| 1080p | OMT_SQ (95) | natural | 418,883 | **318** (±14%) | 222 (±1%) | 141 (±8%) | 225 (±3%) |
| 1080p | OMT_HQ (96) | natural | 559,463 | **268** (±2%) | 190 (±3%) | 131 (±2%) | 200 (±1%) |
| 1080p | LQ (98) | bars | 22,008 | **609** (±4%) | 409 (±1%) | 221 (±3%) | 514 (±3%) |
| 1080p | SQ (98) | bars | 22,008 | **607** (±3%) | 404 (±2%) | 224 (±1%) | 511 (±1%) |
| 1080p | HQ (98) | bars | 24,303 | **649** (±3%) | 407 (±1%) | 219 (±3%) | 506 (±6%) |
| 1080p | OMT_LQ (98) | bars | 22,008 | **641** (±3%) | 409 (±4%) | 221 (±2%) | 517 (±8%) |
| 1080p | OMT_SQ (98) | bars | 24,303 | **634** (±4%) | 414 (±1%) | 224 (±1%) | 506 (±1%) |
| 1080p | OMT_HQ (98) | bars | 24,303 | **597** (±8%) | 412 (±2%) | 221 (±3%) | 510 (±3%) |
| 2160p | LQ (92) | natural | 595,921 | **102** (±3%) | 65 (±6%) | 43 ✗ (±5%) | 78 (±9%) |
| 2160p | SQ (94) | natural | 1,088,322 | **82** (±6%) | 56 ✗ (±4%) | 38 ✗ (±5%) | 68 (±8%) |
| 2160p | HQ (96) | natural | 2,145,917 | **64** (±4%) | 43 ✗ (±4%) | 32 ✗ (±5%) | 48 ✗ (±4%) |
| 2160p | OMT_LQ (92) | natural | 595,921 | **102** (±3%) | 56 ✗ (±14%) | 41 ✗ (±18%) | 86 (±5%) |
| 2160p | OMT_SQ (92) | natural | 595,921 | **104** (±5%) | 71 (±1%) | 45 ✗ (±5%) | 86 (±7%) |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **98** (±1%) | 65 (±1%) | 42 ✗ (±1%) | 64 (±4%) |
| 2160p | LQ (98) | bars | 76,145 | **160** (±6%) | 104 (±8%) | 52 ✗ (±11%) | 117 (±7%) |
| 2160p | SQ (98) | bars | 76,145 | **154** (±4%) | 108 (±1%) | 52 ✗ (±7%) | 118 (±3%) |
| 2160p | HQ (98) | bars | 80,733 | **153** (±3%) | 102 (±10%) | 46 ✗ (±16%) | 120 (±4%) |
| 2160p | OMT_LQ (98) | bars | 76,145 | **154** (±6%) | 99 (±4%) | 53 ✗ (±2%) | 118 (±1%) |
| 2160p | OMT_SQ (98) | bars | 76,145 | **157** (±4%) | 97 (±11%) | 48 ✗ (±9%) | 115 (±2%) |
| 2160p | OMT_HQ (98) | bars | 80,733 | **148** (±6%) | 101 (±2%) | 50 ✗ (±6%) | 116 (±6%) |

### Encode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | 1331 (±3%) | 906 (±5%) | 398 (±5%) | **2011** (±3%) |
| 1080p | SQ (94) | natural | 292,310 | 1070 (±4%) | 791 (±5%) | 424 (±5%) | **1470** (±8%) |
| 1080p | HQ (96) | natural | 559,463 | 796 (±5%) | 590 (±5%) | 348 (±7%) | **1181** (±4%) |
| 1080p | OMT_LQ (92) | natural | 168,577 | 1296 (±3%) | 863 (±8%) | 426 (±6%) | **2110** (±2%) |
| 1080p | OMT_SQ (95) | natural | 418,883 | 988 (±6%) | 721 (±1%) | 342 (±60%) | **1117** (±9%) |
| 1080p | OMT_HQ (96) | natural | 559,463 | 821 (±3%) | 606 (±3%) | 393 (±5%) | **1221** (±4%) |
| 1080p | LQ (98) | bars | 22,008 | 2086 (±4%) | 1404 (±11%) | 482 (±5%) | **2843** (±8%) |
| 1080p | SQ (98) | bars | 22,008 | 2122 (±7%) | 1425 (±10%) | 481 (±3%) | **3249** (±4%) |
| 1080p | HQ (98) | bars | 24,303 | 2062 (±10%) | 1421 (±5%) | 462 (±1%) | **3195** (±2%) |
| 1080p | OMT_LQ (98) | bars | 22,008 | 2167 (±3%) | 1421 (±3%) | 492 (±6%) | **3139** (±5%) |
| 1080p | OMT_SQ (98) | bars | 24,303 | 2098 (±2%) | 1398 (±7%) | 449 (±9%) | **2843** (±5%) |
| 1080p | OMT_HQ (98) | bars | 24,303 | 2066 (±3%) | 1432 (±5%) | 482 (±5%) | **2496** (±5%) |
| 2160p | LQ (92) | natural | 595,921 | 366 (±6%) | 257 (±6%) | 132 (±6%) | **544** (±6%) |
| 2160p | SQ (94) | natural | 1,088,322 | 300 (±9%) | 247 (±7%) | 125 (±5%) | **451** (±4%) |
| 2160p | HQ (96) | natural | 2,145,917 | 256 (±6%) | 220 (±13%) | 101 (±8%) | **330** (±5%) |
| 2160p | OMT_LQ (92) | natural | 595,921 | 368 (±3%) | 245 (±8%) | 125 (±15%) | **572** (±7%) |
| 2160p | OMT_SQ (92) | natural | 595,921 | 358 (±12%) | 257 (±4%) | 133 (±4%) | **544** (±3%) |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | 348 (±5%) | 253 (±4%) | 117 (±8%) | **456** (±3%) |
| 2160p | LQ (98) | bars | 76,145 | 571 (±4%) | 375 (±5%) | 122 (±2%) | **734** (±9%) |
| 2160p | SQ (98) | bars | 76,145 | 540 (±3%) | 340 (±6%) | 115 (±9%) | **839** (±4%) |
| 2160p | HQ (98) | bars | 80,733 | 540 (±1%) | 368 (±3%) | 128 (±6%) | **836** (±3%) |
| 2160p | OMT_LQ (98) | bars | 76,145 | 515 (±5%) | 352 (±5%) | 119 (±7%) | **814** (±7%) |
| 2160p | OMT_SQ (98) | bars | 76,145 | 524 (±10%) | 360 (±3%) | 123 (±6%) | **796** (±2%) |
| 2160p | OMT_HQ (98) | bars | 80,733 | 530 (±2%) | 352 (±7%) | 135 (±7%) | **811** (±8%) |

### Decode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **407** (±6%) | 364 (±2%) | 269 (±5%) | 372 (±2%) |
| 1080p | SQ (94) | natural | 292,310 | 285 (±6%) | 265 (±3%) | 193 (±1%) | **292** (±1%) |
| 1080p | HQ (96) | natural | 559,463 | **195** (±2%) | 163 (±4%) | 106 (±2%) | 188 (±5%) |
| 1080p | OMT_LQ (92) | natural | 168,577 | **410** (±2%) | 361 (±4%) | 277 (±6%) | 377 (±5%) |
| 1080p | OMT_SQ (95) | natural | 418,883 | **242** (±2%) | 216 (±1%) | 151 (±1%) | 231 (±2%) |
| 1080p | OMT_HQ (96) | natural | 559,463 | 186 (±4%) | 167 (±8%) | 114 (±1%) | **200** (±1%) |
| 1080p | LQ (98) | bars | 22,008 | 1202 (±7%) | **1245** (±6%) | 757 (±2%) | 914 (±4%) |
| 1080p | SQ (98) | bars | 22,008 | 1276 (±9%) | **1303** (±4%) | 788 (±3%) | 958 (±1%) |
| 1080p | HQ (98) | bars | 24,303 | 1245 (±1%) | **1291** (±2%) | 775 (±1%) | 957 (±1%) |
| 1080p | OMT_LQ (98) | bars | 22,008 | **1265** (±10%) | 1256 (±3%) | 777 (±1%) | 984 (±4%) |
| 1080p | OMT_SQ (98) | bars | 24,303 | 1244 (±7%) | **1288** (±4%) | 751 (±5%) | 956 (±1%) |
| 1080p | OMT_HQ (98) | bars | 24,303 | 1202 (±4%) | **1270** (±4%) | 784 (±1%) | 988 (±1%) |
| 2160p | LQ (92) | natural | 595,921 | **117** (±2%) | 101 (±13%) | 75 (±1%) | 92 (±11%) |
| 2160p | SQ (94) | natural | 1,088,322 | **74** (±8%) | 60 (±7%) | 50 ✗ (±2%) | 62 (±7%) |
| 2160p | HQ (96) | natural | 2,145,917 | 44 ✗ (±4%) | 38 ✗ (±3%) | 27 ✗ (±1%) | **46** ✗ (±4%) |
| 2160p | OMT_LQ (92) | natural | 595,921 | **110** (±6%) | 96 (±5%) | 75 (±2%) | 103 (±3%) |
| 2160p | OMT_SQ (92) | natural | 595,921 | **119** (±2%) | 102 (±5%) | 75 (±1%) | 101 (±3%) |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **84** (±1%) | 73 (±4%) | 52 ✗ (±2%) | 62 (±2%) |
| 2160p | LQ (98) | bars | 76,145 | **368** (±12%) | 364 (±5%) | 181 (±11%) | 263 (±1%) |
| 2160p | SQ (98) | bars | 76,145 | 355 (±6%) | **375** (±8%) | 189 (±10%) | 248 (±10%) |
| 2160p | HQ (98) | bars | 80,733 | 349 (±3%) | **363** (±3%) | 170 (±7%) | 250 (±2%) |
| 2160p | OMT_LQ (98) | bars | 76,145 | **366** (±9%) | 359 (±2%) | 186 (±9%) | 244 (±7%) |
| 2160p | OMT_SQ (98) | bars | 76,145 | 346 (±4%) | **357** (±2%) | 179 (±6%) | 234 (±17%) |
| 2160p | OMT_HQ (98) | bars | 80,733 | **346** (±4%) | 342 (±8%) | 189 (±1%) | 246 (±2%) |

### Decode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx AVX2 | libvmx 128-bit | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | 1328 (±5%) | 1197 (±7%) | 579 (±2%) | **2048** (±6%) |
| 1080p | SQ (94) | natural | 292,310 | 947 (±5%) | 904 (±2%) | 538 (±4%) | **1327** (±16%) |
| 1080p | HQ (96) | natural | 559,463 | 618 (±11%) | 540 (±26%) | 388 (±1%) | **1130** (±4%) |
| 1080p | OMT_LQ (92) | natural | 168,577 | 1335 (±4%) | 1153 (±2%) | 635 (±2%) | **2169** (±5%) |
| 1080p | OMT_SQ (95) | natural | 418,883 | 777 (±7%) | 664 (±7%) | 462 (±8%) | **1071** (±6%) |
| 1080p | OMT_HQ (96) | natural | 559,463 | 614 (±3%) | 559 (±1%) | 424 (±3%) | **1231** (±4%) |
| 1080p | LQ (98) | bars | 22,008 | 4220 (±3%) | **4660** (±4%) | 780 (±4%) | 3692 (±8%) |
| 1080p | SQ (98) | bars | 22,008 | 4521 (±2%) | **4652** (±4%) | 780 (±4%) | 4058 (±13%) |
| 1080p | HQ (98) | bars | 24,303 | **4624** (±5%) | 4063 (±5%) | 792 (±4%) | 4497 (±3%) |
| 1080p | OMT_LQ (98) | bars | 22,008 | 4405 (±3%) | **4579** (±4%) | 780 (±1%) | 4291 (±2%) |
| 1080p | OMT_SQ (98) | bars | 24,303 | 4411 (±3%) | **4674** (±3%) | 734 (±4%) | 4380 (±11%) |
| 1080p | OMT_HQ (98) | bars | 24,303 | 4301 (±2%) | **4460** (±2%) | 781 (±1%) | 4397 (±3%) |
| 2160p | LQ (92) | natural | 595,921 | 430 (±6%) | 362 (±6%) | 193 (±3%) | **636** (±3%) |
| 2160p | SQ (94) | natural | 1,088,322 | 283 (±21%) | 256 (±7%) | 173 (±3%) | **477** (±3%) |
| 2160p | HQ (96) | natural | 2,145,917 | 185 (±6%) | 207 (±16%) | 129 (±5%) | **209** (±50%) |
| 2160p | OMT_LQ (92) | natural | 595,921 | 334 (±61%) | 310 (±18%) | 174 (±8%) | **685** (±5%) |
| 2160p | OMT_SQ (92) | natural | 595,921 | 423 (±4%) | 382 (±9%) | 202 (±2%) | **645** (±7%) |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | 296 (±9%) | 273 (±5%) | 163 (±6%) | **467** (±7%) |
| 2160p | LQ (98) | bars | 76,145 | **1072** (±8%) | 1052 (±3%) | 226 (±5%) | 1068 (±7%) |
| 2160p | SQ (98) | bars | 76,145 | **1223** (±6%) | 914 (±15%) | 242 (±4%) | 1206 (±11%) |
| 2160p | HQ (98) | bars | 80,733 | 1290 (±17%) | 1179 (±9%) | 250 (±2%) | **1583** (±3%) |
| 2160p | OMT_LQ (98) | bars | 76,145 | 1261 (±20%) | 1279 (±7%) | 242 (±2%) | **1537** (±6%) |
| 2160p | OMT_SQ (98) | bars | 76,145 | 1234 (±12%) | 1283 (±8%) | 241 (±4%) | **1523** (±4%) |
| 2160p | OMT_HQ (98) | bars | 80,733 | 1355 (±18%) | 1214 (±4%) | 253 (±2%) | **1502** (±5%) |


## ARM / NEON

Same harness and the same pictures as the x86-64 run. The settled `q` and the frame size matched that run on all 24 picture/profile pairs. Each host is one pass, from [workflow run 44675775781](https://github.com/MikanseiLaboratory/vmx-rs/actions/runs/44675775781) at commit `1eea3ed`. rustc 1.100.0.

libvmx is the official ARM build (`vmxcodec_arm.cpp`, NEON through sse2neon, no `-mavx2`). That build does not compile the 256-bit path, so there is one libvmx column. vmx-codec reports `NEON (compile-time)`. vmx reports `dct=Neon` and `color=Neon`. On both hosts, all 288 rows matched libvmx byte for byte on encode and pixel for pixel on decode. 10-bit P216 / PA16 was not measured: the harness again got `invalid parameters` from `encode_p216`, `decode_p216`, `encode_pa16`, and `decode_pa16`.

`✗` means the median is below 60 fps, using the unrounded median (a value that rounds to 60 can still be under 60). `bytes/frame` is the exact encoded size. Min and max of the seven runs are in the CSV files linked below.

The ratio tables are `median / libvmx` on that host. libvmx here is the sse2neon path, not the AVX2 path in the x86-64 tables. The geometric mean is over the 24 rows of that operation and thread count. The range is the smallest and largest ratio.

### ubuntu-24.04-arm (Neoverse-N2)

- CPU: ARM Neoverse-N2, implementer `0x41`, part `0xd49`, revision 0, 4 vCPU, no SMT. `lscpu` reports NEON (`asimd`) and SVE.
- OS: Ubuntu 24.04, kernel 7.0.0-1017-azure
- Compiler: clang++ 18.1.3, flags `-O3 -std=c++17 -fdeclspec -fPIC -pthread`
- Threads: 1 and 4
- Raw rows: [`bench/results/linux-arm64.csv`](bench/results/linux-arm64.csv)

1080p: every median is above 60 fps. The lowest is vmx, OMT_HQ, natural, encode, 1 thread, at 98 fps.

2160p, 4 threads: every median is above 60 fps. The lowest is vmx, HQ, natural, encode, at 96 fps.

2160p, 1 thread: encode is below 60 fps for all three implementations, on every profile and both images (25–42 fps). Natural decode is below 60 fps for all three on HQ, SQ, and OMT_HQ. On natural LQ, OMT_LQ, and OMT_SQ, only vmx is below 60 fps (51 fps). Colour-bar decodes are above 60 fps.

| Operation | Threads | vmx / libvmx | vmx-codec / libvmx |
|---|---:|---:|---:|
| Encode | 1 | 0.86 (0.85–0.89) | 1.04 (1.01–1.06) |
| Encode | 4 | 0.89 (0.83–0.97) | 0.93 (0.85–1.05) |
| Decode | 1 | 0.78 (0.70–0.83) | 0.82 (0.71–0.96) |
| Decode | 4 | 0.76 (0.64–0.91) | 0.55 (0.31–0.91) |

vmx by image:

| Operation | Threads | natural | bars |
|---|---:|---:|---:|
| Encode | 1 | 0.87 (0.86–0.89) | 0.85 (0.85–0.86) |
| Encode | 4 | 0.91 (0.85–0.97) | 0.87 (0.83–0.92) |
| Decode | 1 | 0.81 (0.79–0.83) | 0.75 (0.70–0.78) |
| Decode | 4 | 0.82 (0.76–0.89) | 0.71 (0.64–0.91) |

The 1-thread encode ratios sit within a few percent of each other across all 24 rows. This is a single pass on a shared runner, so a gap of that size is not a settled ordering. The decode ratios, and vmx-codec's 4-thread decode mean of 0.55, are wider. The low end of that decode range is colour bars.

### Encode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | 139 | **148** | 123 |
| 1080p | SQ (94) | natural | 292,310 | 130 | **135** | 114 |
| 1080p | HQ (96) | natural | 559,463 | 113 | **115** | 98 |
| 1080p | OMT_LQ (92) | natural | 168,577 | 140 | **147** | 123 |
| 1080p | OMT_SQ (95) | natural | 418,883 | 122 | **127** | 107 |
| 1080p | OMT_HQ (96) | natural | 559,463 | 113 | **115** | 98 |
| 1080p | LQ (98) | bars | 22,008 | 162 | **169** | 138 |
| 1080p | SQ (98) | bars | 22,008 | 162 | **170** | 139 |
| 1080p | HQ (98) | bars | 24,303 | 162 | **170** | 139 |
| 1080p | OMT_LQ (98) | bars | 22,008 | 162 | **171** | 139 |
| 1080p | OMT_SQ (98) | bars | 24,303 | 162 | **171** | 138 |
| 1080p | OMT_HQ (98) | bars | 24,303 | 162 | **171** | 137 |
| 2160p | LQ (92) | natural | 595,921 | 35 ✗ | **37** ✗ | 31 ✗ |
| 2160p | SQ (94) | natural | 1,088,322 | 33 ✗ | **34** ✗ | 29 ✗ |
| 2160p | HQ (96) | natural | 2,145,917 | **29** ✗ | **29** ✗ | 25 ✗ |
| 2160p | OMT_LQ (92) | natural | 595,921 | 35 ✗ | **37** ✗ | 31 ✗ |
| 2160p | OMT_SQ (92) | natural | 595,921 | 35 ✗ | **37** ✗ | 31 ✗ |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | 33 ✗ | **34** ✗ | 28 ✗ |
| 2160p | LQ (98) | bars | 76,145 | 40 ✗ | **42** ✗ | 34 ✗ |
| 2160p | SQ (98) | bars | 76,145 | 40 ✗ | **42** ✗ | 34 ✗ |
| 2160p | HQ (98) | bars | 80,733 | 40 ✗ | **42** ✗ | 35 ✗ |
| 2160p | OMT_LQ (98) | bars | 76,145 | 40 ✗ | **42** ✗ | 35 ✗ |
| 2160p | OMT_SQ (98) | bars | 76,145 | 40 ✗ | **42** ✗ | 35 ✗ |
| 2160p | OMT_HQ (98) | bars | 80,733 | 40 ✗ | **42** ✗ | 35 ✗ |

### Encode, 4 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **536** | 488 | 476 |
| 1080p | SQ (94) | natural | 292,310 | **514** | 457 | 443 |
| 1080p | HQ (96) | natural | 559,463 | **443** | 394 | 378 |
| 1080p | OMT_LQ (92) | natural | 168,577 | **551** | 486 | 475 |
| 1080p | OMT_SQ (95) | natural | 418,883 | **483** | 421 | 412 |
| 1080p | OMT_HQ (96) | natural | 559,463 | **440** | 389 | 376 |
| 1080p | LQ (98) | bars | 22,008 | **633** | 548 | 522 |
| 1080p | SQ (98) | bars | 22,008 | **625** | 550 | 539 |
| 1080p | HQ (98) | bars | 24,303 | **634** | 553 | 536 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **632** | 553 | 540 |
| 1080p | OMT_SQ (98) | bars | 24,303 | **637** | 544 | 541 |
| 1080p | OMT_HQ (98) | bars | 24,303 | **634** | 541 | 540 |
| 2160p | LQ (92) | natural | 595,921 | 124 | **130** | 120 |
| 2160p | SQ (94) | natural | 1,088,322 | **119** | 117 | 113 |
| 2160p | HQ (96) | natural | 2,145,917 | **103** | 102 | 96 |
| 2160p | OMT_LQ (92) | natural | 595,921 | 125 | **129** | 122 |
| 2160p | OMT_SQ (92) | natural | 595,921 | **128** | **128** | 121 |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **118** | 116 | 111 |
| 2160p | LQ (98) | bars | 76,145 | **149** | 143 | 134 |
| 2160p | SQ (98) | bars | 76,145 | **147** | 144 | 134 |
| 2160p | HQ (98) | bars | 80,733 | **149** | 145 | 134 |
| 2160p | OMT_LQ (98) | bars | 76,145 | **149** | 142 | 134 |
| 2160p | OMT_SQ (98) | bars | 76,145 | **149** | 146 | 135 |
| 2160p | OMT_HQ (98) | bars | 80,733 | **150** | 142 | 135 |

### Decode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **240** | 229 | 191 |
| 1080p | SQ (94) | natural | 292,310 | **183** | 170 | 148 |
| 1080p | HQ (96) | natural | 559,463 | **125** | 110 | 103 |
| 1080p | OMT_LQ (92) | natural | 168,577 | **240** | 229 | 191 |
| 1080p | OMT_SQ (95) | natural | 418,883 | **153** | 139 | 125 |
| 1080p | OMT_HQ (96) | natural | 559,463 | **125** | 110 | 103 |
| 1080p | LQ (98) | bars | 22,008 | **1761** | 1252 | 1326 |
| 1080p | SQ (98) | bars | 22,008 | **1764** | 1254 | 1325 |
| 1080p | HQ (98) | bars | 24,303 | **1754** | 1255 | 1324 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **1756** | 1252 | 1327 |
| 1080p | OMT_SQ (98) | bars | 24,303 | **1750** | 1256 | 1324 |
| 1080p | OMT_HQ (98) | bars | 24,303 | **1758** | 1255 | 1325 |
| 2160p | LQ (92) | natural | 595,921 | **65** | 61 | 51 ✗ |
| 2160p | SQ (94) | natural | 1,088,322 | **49** ✗ | 45 ✗ | 40 ✗ |
| 2160p | HQ (96) | natural | 2,145,917 | **32** ✗ | 28 ✗ | 26 ✗ |
| 2160p | OMT_LQ (92) | natural | 595,921 | **65** | 62 | 51 ✗ |
| 2160p | OMT_SQ (92) | natural | 595,921 | **65** | 61 | 51 ✗ |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **49** ✗ | 45 ✗ | 39 ✗ |
| 2160p | LQ (98) | bars | 76,145 | **411** | 323 | 319 |
| 2160p | SQ (98) | bars | 76,145 | **437** | 326 | 316 |
| 2160p | HQ (98) | bars | 80,733 | **430** | 324 | 322 |
| 2160p | OMT_LQ (98) | bars | 76,145 | **443** | 327 | 324 |
| 2160p | OMT_SQ (98) | bars | 76,145 | **441** | 326 | 326 |
| 2160p | OMT_HQ (98) | bars | 80,733 | **450** | 327 | 316 |

### Decode, 4 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **912** | 686 | 701 |
| 1080p | SQ (94) | natural | 292,310 | **704** | 542 | 555 |
| 1080p | HQ (96) | natural | 559,463 | **484** | 380 | 392 |
| 1080p | OMT_LQ (92) | natural | 168,577 | **917** | 690 | 701 |
| 1080p | OMT_SQ (95) | natural | 418,883 | **592** | 463 | 471 |
| 1080p | OMT_HQ (96) | natural | 559,463 | **491** | 375 | 391 |
| 1080p | LQ (98) | bars | 22,008 | **4701** | 1819 | 3657 |
| 1080p | SQ (98) | bars | 22,008 | **5224** | 1838 | 3788 |
| 1080p | HQ (98) | bars | 24,303 | **5869** | 1848 | 3730 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **5431** | 1858 | 3814 |
| 1080p | OMT_SQ (98) | bars | 24,303 | **5450** | 1820 | 3753 |
| 1080p | OMT_HQ (98) | bars | 24,303 | **5762** | 1812 | 3772 |
| 2160p | LQ (92) | natural | 595,921 | **244** | 200 | 198 |
| 2160p | SQ (94) | natural | 1,088,322 | **176** | 156 | 153 |
| 2160p | HQ (96) | natural | 2,145,917 | **118** | 102 | 102 |
| 2160p | OMT_LQ (92) | natural | 595,921 | **227** | 202 | 198 |
| 2160p | OMT_SQ (92) | natural | 595,921 | **222** | 201 | 196 |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **176** | 154 | 151 |
| 2160p | LQ (98) | bars | 76,145 | **1132** | 575 | 1033 |
| 2160p | SQ (98) | bars | 76,145 | **1576** | 572 | 1034 |
| 2160p | HQ (98) | bars | 80,733 | **1536** | 570 | 1031 |
| 2160p | OMT_LQ (98) | bars | 76,145 | **1492** | 593 | 1026 |
| 2160p | OMT_SQ (98) | bars | 76,145 | **1393** | 585 | 1037 |
| 2160p | OMT_HQ (98) | bars | 80,733 | **1547** | 582 | 1030 |

### macos-latest (Apple M1, virtual)

- CPU: Apple M1 (Virtual), 3 vCPU. `hw.optional.neon` is 1.
- OS: macOS 26.7.2, kernel 25.6.0 (`RELEASE_ARM64_VMAPPLE`)
- Compiler: Apple clang 21.0.0, flags `-O3 -std=c++17 -fdeclspec -fPIC`
- Threads: 1 and 3
- Raw rows: [`bench/results/macos-arm64.csv`](bench/results/macos-arm64.csv)

1080p: every median is above 60 fps. The lowest is vmx-codec, HQ, natural, decode, 1 thread, at 69 fps.

2160p, 3 threads: the medians below 60 fps are HQ natural encode (vmx-codec 45, vmx 60 from an unrounded 59.8) and HQ natural decode (vmx-codec 59). The other 3-thread rows are above 60 fps. The lowest libvmx median at 3 threads is HQ natural encode, 66 fps.

2160p, 1 thread: natural encode is below 60 fps for all three implementations on every profile. Natural decode is below 60 fps for HQ and SQ (all three) and for OMT_HQ (libvmx and vmx-codec). Colour-bar encode is mixed; the `✗` marks in the table are the cells under 60 fps. Colour-bar decode at 1 thread is above 60 fps.

The seven-run spread on this virtual machine is wide on some rows. Two examples for vmx at 3 threads: 1080p HQ natural decode, median 159 fps, min 124, max 276; 1080p SQ natural encode, median 255 fps, min 106, max 272. The geometric means below, and especially the 3-thread encode range of 0.40–1.86 for vmx / libvmx, describe that sample. They are not a stable ordering. The Neoverse-N2 ratios above sit in a narrower band.

| Operation | Threads | vmx / libvmx | vmx-codec / libvmx |
|---|---:|---:|---:|
| Encode | 1 | 0.83 (0.60–1.18) | 0.91 (0.57–1.14) |
| Encode | 3 | 0.91 (0.40–1.86) | 0.83 (0.45–1.28) |
| Decode | 1 | 0.89 (0.51–1.27) | 0.79 (0.44–1.04) |
| Decode | 3 | 0.98 (0.62–1.61) | 0.71 (0.40–1.30) |

vmx by image:

| Operation | Threads | natural | bars |
|---|---:|---:|---:|
| Encode | 1 | 0.88 (0.63–1.18) | 0.79 (0.60–1.00) |
| Encode | 3 | 1.01 (0.60–1.86) | 0.81 (0.40–1.11) |
| Decode | 1 | 0.98 (0.83–1.27) | 0.81 (0.51–1.11) |
| Decode | 3 | 1.02 (0.67–1.61) | 0.94 (0.62–1.12) |

### Encode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **149** | 147 | 120 |
| 1080p | SQ (94) | natural | 292,310 | **165** | 148 | 121 |
| 1080p | HQ (96) | natural | 559,463 | **124** | 72 | 78 |
| 1080p | OMT_LQ (92) | natural | 168,577 | 137 | 127 | **152** |
| 1080p | OMT_SQ (95) | natural | 418,883 | **137** | 133 | 103 |
| 1080p | OMT_HQ (96) | natural | 559,463 | 89 | **98** | 97 |
| 1080p | LQ (98) | bars | 22,008 | **302** | 236 | 215 |
| 1080p | SQ (98) | bars | 22,008 | **266** | 151 | 159 |
| 1080p | HQ (98) | bars | 24,303 | 184 | **205** | 184 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **194** | 179 | 174 |
| 1080p | OMT_SQ (98) | bars | 24,303 | 232 | **258** | 162 |
| 1080p | OMT_HQ (98) | bars | 24,303 | 228 | **230** | 202 |
| 2160p | LQ (92) | natural | 595,921 | 38 ✗ | 42 ✗ | **45** ✗ |
| 2160p | SQ (94) | natural | 1,088,322 | **36** ✗ | 25 ✗ | **36** ✗ |
| 2160p | HQ (96) | natural | 2,145,917 | 30 ✗ | **34** ✗ | 23 ✗ |
| 2160p | OMT_LQ (92) | natural | 595,921 | **47** ✗ | 45 ✗ | 41 ✗ |
| 2160p | OMT_SQ (92) | natural | 595,921 | **46** ✗ | 40 ✗ | 45 ✗ |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **42** ✗ | 39 ✗ | 36 ✗ |
| 2160p | LQ (98) | bars | 76,145 | **65** | 55 ✗ | 58 ✗ |
| 2160p | SQ (98) | bars | 76,145 | **66** | 52 ✗ | 55 ✗ |
| 2160p | HQ (98) | bars | 80,733 | **64** | 60 ✗ | 46 ✗ |
| 2160p | OMT_LQ (98) | bars | 76,145 | **71** | 62 | 53 ✗ |
| 2160p | OMT_SQ (98) | bars | 76,145 | 56 ✗ | **63** | 46 ✗ |
| 2160p | OMT_HQ (98) | bars | 80,733 | **72** | 66 | 53 ✗ |

### Encode, 3 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | 284 | **365** | 351 |
| 1080p | SQ (94) | natural | 292,310 | **427** | 192 | 255 |
| 1080p | HQ (96) | natural | 559,463 | 118 | 142 | **220** |
| 1080p | OMT_LQ (92) | natural | 168,577 | **263** | 241 | 258 |
| 1080p | OMT_SQ (95) | natural | 418,883 | 224 | 211 | **243** |
| 1080p | OMT_HQ (96) | natural | 559,463 | 222 | **268** | 198 |
| 1080p | LQ (98) | bars | 22,008 | **681** | 311 | 422 |
| 1080p | SQ (98) | bars | 22,008 | **529** | 239 | 209 |
| 1080p | HQ (98) | bars | 24,303 | **310** | 242 | 292 |
| 1080p | OMT_LQ (98) | bars | 22,008 | **351** | 268 | 319 |
| 1080p | OMT_SQ (98) | bars | 24,303 | 330 | 309 | **353** |
| 1080p | OMT_HQ (98) | bars | 24,303 | 302 | 315 | **334** |
| 2160p | LQ (92) | natural | 595,921 | **78** | 70 | 70 |
| 2160p | SQ (94) | natural | 1,088,322 | 73 | 65 | **75** |
| 2160p | HQ (96) | natural | 2,145,917 | **66** | 45 ✗ | 60 ✗ |
| 2160p | OMT_LQ (92) | natural | 595,921 | **86** | 68 | 82 |
| 2160p | OMT_SQ (92) | natural | 595,921 | 79 | 75 | **86** |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **75** | 65 | 72 |
| 2160p | LQ (98) | bars | 76,145 | **106** | 105 | **106** |
| 2160p | SQ (98) | bars | 76,145 | **110** | 96 | 98 |
| 2160p | HQ (98) | bars | 80,733 | 115 | **129** | 103 |
| 2160p | OMT_LQ (98) | bars | 76,145 | **134** | 85 | 104 |
| 2160p | OMT_SQ (98) | bars | 76,145 | 102 | **110** | 101 |
| 2160p | OMT_HQ (98) | bars | 80,733 | **181** | 120 | 102 |

### Decode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | **294** | 262 | 274 |
| 1080p | SQ (94) | natural | 292,310 | **216** | 191 | 180 |
| 1080p | HQ (96) | natural | 559,463 | **121** | 69 | 116 |
| 1080p | OMT_LQ (92) | natural | 168,577 | 249 | 253 | **268** |
| 1080p | OMT_SQ (95) | natural | 418,883 | 174 | 171 | **180** |
| 1080p | OMT_HQ (96) | natural | 559,463 | 109 | 96 | **139** |
| 1080p | LQ (98) | bars | 22,008 | **2345** | 1405 | 1375 |
| 1080p | SQ (98) | bars | 22,008 | **2106** | 936 | 1083 |
| 1080p | HQ (98) | bars | 24,303 | 1600 | 1262 | **1621** |
| 1080p | OMT_LQ (98) | bars | 22,008 | 1397 | 1261 | **1556** |
| 1080p | OMT_SQ (98) | bars | 24,303 | **1837** | 1532 | 1372 |
| 1080p | OMT_HQ (98) | bars | 24,303 | **2144** | 1518 | 1695 |
| 2160p | LQ (92) | natural | 595,921 | **84** | 77 | 76 |
| 2160p | SQ (94) | natural | 1,088,322 | 52 ✗ | 49 ✗ | **55** ✗ |
| 2160p | HQ (96) | natural | 2,145,917 | **40** ✗ | 30 ✗ | 34 ✗ |
| 2160p | OMT_LQ (92) | natural | 595,921 | 76 | **77** | 74 |
| 2160p | OMT_SQ (92) | natural | 595,921 | **89** | 87 | 75 |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | 57 ✗ | 59 ✗ | **61** |
| 2160p | LQ (98) | bars | 76,145 | **383** | 312 | 330 |
| 2160p | SQ (98) | bars | 76,145 | **375** | 232 | **375** |
| 2160p | HQ (98) | bars | 80,733 | **432** | 281 | 336 |
| 2160p | OMT_LQ (98) | bars | 76,145 | **418** | 289 | 385 |
| 2160p | OMT_SQ (98) | bars | 76,145 | **382** | 260 | 322 |
| 2160p | OMT_HQ (98) | bars | 80,733 | **426** | 352 | 333 |

### Decode, 3 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1080p | LQ (92) | natural | 168,577 | 325 | 350 | **524** |
| 1080p | SQ (94) | natural | 292,310 | **547** | 270 | 368 |
| 1080p | HQ (96) | natural | 559,463 | 156 | 146 | **159** |
| 1080p | OMT_LQ (92) | natural | 168,577 | 444 | 441 | **460** |
| 1080p | OMT_SQ (95) | natural | 418,883 | 276 | 188 | **313** |
| 1080p | OMT_HQ (96) | natural | 559,463 | 249 | **324** | 270 |
| 1080p | LQ (98) | bars | 22,008 | **2630** | 1120 | 2298 |
| 1080p | SQ (98) | bars | 22,008 | **1692** | 1157 | 1673 |
| 1080p | HQ (98) | bars | 24,303 | 1936 | 1108 | **2024** |
| 1080p | OMT_LQ (98) | bars | 22,008 | **2218** | 1201 | 2132 |
| 1080p | OMT_SQ (98) | bars | 24,303 | 2134 | 1254 | **2269** |
| 1080p | OMT_HQ (98) | bars | 24,303 | 2196 | 1297 | **2459** |
| 2160p | LQ (92) | natural | 595,921 | **141** | 125 | 137 |
| 2160p | SQ (94) | natural | 1,088,322 | 102 | 87 | **117** |
| 2160p | HQ (96) | natural | 2,145,917 | 70 | 59 ✗ | **73** |
| 2160p | OMT_LQ (92) | natural | 595,921 | **142** | 119 | 136 |
| 2160p | OMT_SQ (92) | natural | 595,921 | **155** | 132 | 136 |
| 2160p | OMT_HQ (94) | natural | 1,192,159 | **110** | 91 | 103 |
| 2160p | LQ (98) | bars | 76,145 | **523** | 341 | 507 |
| 2160p | SQ (98) | bars | 76,145 | 512 | 333 | **557** |
| 2160p | HQ (98) | bars | 80,733 | 518 | 363 | **543** |
| 2160p | OMT_LQ (98) | bars | 76,145 | **571** | 361 | 354 |
| 2160p | OMT_SQ (98) | bars | 76,145 | 598 | 354 | **628** |
| 2160p | OMT_HQ (98) | bars | 80,733 | **921** | 364 | 586 |

## Reproducing

```bash
cd bench
cargo run --release > results.csv
```

`clang++`, `git`, and `ar` are required. Useful filters: `--quick`, `--res 1080`, `--prof HQ`, `--img natural`, `--impl libvmx`, `--threads 1`, `--thread-counts 1,8`, `--reps`, `--secs`. `VMX_BENCH_META=host.txt` writes the host description that the harness prints to stderr. See [`bench/README.md`](bench/README.md).
