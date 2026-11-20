# Benchmark

CPU numbers below are from dedicated GCP Spot VMs in Tokyo (`asia-northeast1`) on 2026-11-20. They replace the earlier shared-VM and GitHub Actions tables as the primary result. Each table is one pass. A gap of a few percent is one sample, not a settled ordering.

Fps columns, left to right, are libvmx (official), vmx-codec (Saul), and vmx. The x86-64 tables insert libvmx 128-bit after the AVX2 column. Every comparison table ends with a Fastest column naming the winner on that row (`libvmx`, `libvmx 128-bit`, `vmx-codec`, `vmx`, or, in the wgpu table, the path name). **Bold** is that winner's value; a tie bolds each tied value and lists every name. In the fps tables the winner is the highest median. In the ratio tables the winner is the highest geometric mean of fps over the rows in that line; libvmx's ratio is 1 and is not printed, and the winner's ratio cell is bold when the winner is vmx or vmx-codec. In the wgpu table, Fastest and bold ignore the host-pixel reference and name the faster of the GPU path and the CPU path that includes the texture transfer. The profile cell is the OMT name, the sender's quality name, and the fixed q, as in `OMT_HQ — High (96)`. `✗` is a median below 60 fps.

| Implementation | Commit / version | What was timed |
|---|---|---|
| [libvmx](https://github.com/openmediatransport/libvmx) | `31724bb96530325a989bc157c8e07538d6f1c4db` | Official C++ codec |
| [vmx-codec](https://github.com/Saul-Punybz/open-media-transport) | `fbde53cccabb1ebfff7ba773a72510ab48a64f3a` (`crates/vmx-codec`) | Pure Rust codec by Saul González |
| vmx (this repository) | 0.2.1 | Pure Rust codec |

Raw rows are in [`bench/results/`](bench/results/). The harness is [`bench/`](bench/).

## Environment

| Machine | CPU | OS and compilers |
|---|---|---|
| `c4-standard-8` Spot, asia-northeast1-b | Intel Xeon Platinum 8581C (Emerald Rapids), 2.3 GHz, 8 vCPU = 4 cores × 2 SMT, AVX2 and AVX-512, 29 GB | Debian 12, kernel 6.1, rustc 1.100.0, Debian clang 14.0.6 |
| `c4a-standard-8` Spot, asia-northeast1-b | Google Axion (Neoverse-V2), 8 cores, no SMT, NEON and SVE2, 31 GB | Debian 12 arm64, kernel 6.1, rustc 1.100.0, Debian clang 14.0.6 |
| `g2-standard-4` Spot, asia-northeast1-c | Intel Xeon at 2.20 GHz (Cascade Lake), 4 vCPU, plus NVIDIA L4 24 GB | Ubuntu 24.04.5, kernel 7.0, driver 580.178.04, Vulkan 1.4.312, rustc 1.100.0, clang 18.1.3 |

libvmx on these Debian 12 machines is clang 14.0.6 with the flags from `build/buildlinuxx64.sh` (`-O3 -std=c++17 -fdeclspec -mlzcnt -mavx2 -mbmi` on x86-64; the ARM line has no AVX flags). The earlier shared-VM and GitHub Actions runs compiled libvmx with clang 19.1.7. Do not read an fps difference between those hosts and these as a compiler-only or CPU-only change.

On the c4, vmx reported `dct=Avx2` and `color=Avx2`. vmx-codec is compile-time SSE2. On the c4a, libvmx is the sse2neon path (one column), vmx-codec is compile-time NEON, and vmx reported `dct=Neon` and `color=Neon`. Rust crates use this repository's release profile (opt-level 3, thin LTO, one codegen unit). libvmx is `-O3` per translation unit, without link-time optimization.

## Method

`bench` fetches libvmx at the commit above and depends on vmx-codec at the commit above.

- Pictures: 1920×1080 and 3840×2160, 8-bit UYVY, progressive. `natural` and `bars` are in [`bench/images/`](bench/images/) (gzip of the UYVY frames, SHA-256 of the uncompressed bytes, and 480×270 PNG previews).
- Profiles: only OMT_LQ, OMT_SQ, and OMT_HQ. See below.
- Quality: libvmx rate control runs for 60 frames on that picture. The settled `q` is then fixed, and every timed encode calls set-quality with that `q`. DC shift and minimum quality stay at the profile defaults.
- Encode is colour conversion, coding, and serialize. Decode is load plus decode to UYVY. Every decoder receives the same packet, produced by libvmx's 128-bit path at the fixed `q`.
- Buffers are allocated before the timer. Three warmup frames, then seven runs of at least 0.3 s. The table value is the median.
- Threads: 1 and 8. On the c4, 8 threads is two per core (SMT). On the c4a, 8 threads is one per core.

| 1080p natural | 1080p bars | 2160p natural | 2160p bars |
|---|---|---|---|
| ![1080p natural](bench/images/1080p-natural.png) | ![1080p bars](bench/images/1080p-bars.png) | ![2160p natural](bench/images/2160p-natural.png) | ![2160p bars](bench/images/2160p-bars.png) |

Every row in [`gcp-x86-c4.csv`](bench/results/gcp-x86-c4.csv) (192) and [`gcp-arm-c4a.csv`](bench/results/gcp-arm-c4a.csv) (144) matched the libvmx 128-bit packet on encode and its pixels on decode. `bytes/frame` is that size. 10-bit P216 and PA16 are not in the matrix: `vmx` 0.2.1 returns `invalid parameters` from those entry points.

## Profiles

The constants are the `VMX_PROFILE` enum in official libvmx `31724bb`, [`src/vmxcodec.h` lines 76–84](https://github.com/openmediatransport/libvmx/blob/31724bb96530325a989bc157c8e07538d6f1c4db/src/vmxcodec.h#L76-L84). libomtnet mirrors those six values in `VMXProfile` ([`src/codecs/OMTVMX1Codec.cs` lines 43–53](https://github.com/openmediatransport/libomtnet/blob/7d29031ff3f355ffa1f0cd08ee24d02e258445dd/src/codecs/OMTVMX1Codec.cs#L43-L53), commit `7d29031`). `OMTSend.CreateCodec` ([`src/OMTSend.cs` lines 511–518](https://github.com/openmediatransport/libomtnet/blob/7d29031ff3f355ffa1f0cd08ee24d02e258445dd/src/OMTSend.cs#L511-L518)) selects only the OMT profiles: `OMTQuality.Low` → `OMT_LQ` (133), `Medium` → `OMT_SQ` (166), `High` → `OMT_HQ` (199). The codec constructor replaces `VMXProfile.Default` with `OMT_SQ` ([`OMTVMX1Codec.cs` line 105](https://github.com/openmediatransport/libomtnet/blob/7d29031ff3f355ffa1f0cd08ee24d02e258445dd/src/codecs/OMTVMX1Codec.cs#L105)). LQ, SQ, and HQ (33, 66, 99) are in the same enum. That sender does not select them.

`VMX_BITRATE_TABLE` in `src/vmxcodec_common.h` supplies the row for the picture height. The byte window is `VMX_CalculateBitrate`: `target / (60 × 8) × 1048576`, then ×0.95 and ×1.05, truncated to an integer. DC shift 0 keeps an 11-bit DC coefficient and 3 keeps 8 bits. The OMT quality floor is `VMX_OMT_MINQ` (52). The ceiling is `VMX_MAXQ` (98). `VMX_SetQuality` overrides the bitrate step for that frame and does not change the DC shift. The harness pinned q after 60 frames of rate control and set the thread count itself, so the default thread count below is not the thread count in the fps tables.

### Height 1080

| Profile | OMT quality | Target | Byte window | DC shift | Minimum quality | Default threads |
|---|---|---:|---:|---|---:|---:|
| OMT_LQ | Low | 86 Mbit/s | 178,476–197,263 | 3 (8-bit DC) | 52 | 2 |
| OMT_SQ | Medium | 200 Mbit/s | 415,061–458,751 | 0 (11-bit DC) | 52 | 2 |
| OMT_HQ | High | 260 Mbit/s | 539,579–596,377 | 0 (11-bit DC) | 52 | 2 |

### Height 2160

| Profile | OMT quality | Target | Byte window | DC shift | Minimum quality | Default threads |
|---|---|---:|---:|---|---:|---:|
| OMT_LQ | Low | 200 Mbit/s | 415,061–458,751 | 3 (8-bit DC) | 52 | 4 |
| OMT_SQ | Medium | 300 Mbit/s | 622,592–688,128 | 3 (8-bit DC) | 52 | 4 |
| OMT_HQ | High | 600 Mbit/s | 1,245,184–1,376,256 | 0 (11-bit DC) | 52 | 4 |

At 1080, Low uses an 8-bit DC and a lower target than Medium and High, which both keep an 11-bit DC. At 2160, Low and Medium use an 8-bit DC and High keeps 11 bits. Every settled q in the fps tables is 92, 94, 95, 96, or 98, above the floor of 52, so the floor did not clamp the timed frames. The DC shift still applies at that fixed q.

## Realtime (1080p60 / 2160p60)

60 fps is one frame interval. The figure is the codec call only.

On the c4, 1080p is above 60 fps for all three implementations, both images, encode and decode, at 1 and 8 threads. The lowest 1080p median is vmx-codec, OMT_HQ, natural, decode, 1 thread, at 122 fps. 2160p at 8 threads is also above 60 fps; the lowest is vmx-codec, OMT_HQ, natural, encode, at 131 fps. At 2160p and 1 thread, vmx-codec encode is below 60 fps on every row (45–59 fps), and its OMT_HQ natural decode is 56 fps. libvmx and vmx stay above 60 fps on every 2160p encode and decode row.

On the c4a, 1080p is above 60 fps on every row. The lowest is vmx, OMT_HQ, natural, decode, 1 thread, at 128 fps. 2160p at 8 threads is above 60 fps; the lowest is libvmx, OMT_HQ, natural, encode, at 164 fps. At 2160p and 1 thread, natural encode is below 60 fps for all three. Colour-bar encode is below 60 fps for libvmx (58) and vmx (54); vmx-codec is at 61–62 fps. OMT_HQ natural decode is below 60 fps for vmx-codec (58) and vmx (50); libvmx is at 62 fps. The other 1-thread 2160p decode rows are above 60 fps.

## Relative to libvmx

Geometric mean of `median / libvmx` over the 12 rows of that operation and thread count (3 profiles × 2 images × 2 resolutions). The range is the smallest and largest ratio. On x86-64, libvmx is the AVX2 column. On ARM, libvmx is the only official column (sse2neon).

### c4-standard-8 (4 cores, SMT)

| Operation | Threads | vmx / libvmx | vmx-codec / libvmx | Fastest |
|---|---:|---:|---:|---|
| Encode | 1 | 0.75 (0.68–0.82) | 0.38 (0.32–0.46) | libvmx |
| Encode | 8 | **1.09 (0.97–1.28)** | 0.32 (0.25–0.42) | vmx |
| Decode | 1 | 0.73 (0.55–1.00) | 0.52 (0.43–0.63) | libvmx |
| Decode | 8 | **1.03 (0.75–1.43)** | 0.33 (0.18–0.60) | vmx |

vmx / libvmx by image. Fastest is still over every implementation, so a bold ratio means vmx is ahead of libvmx, vmx-codec, and, on x86-64, libvmx 128-bit.

| Operation | Threads | natural | bars | Fastest (natural) | Fastest (bars) |
|---|---:|---:|---:|---|---|
| Encode | 1 | 0.75 (0.68–0.82) | 0.74 (0.74–0.75) | libvmx | libvmx |
| Encode | 8 | **1.16 (1.02–1.28)** | **1.03 (0.97–1.08)** | vmx | vmx |
| Decode | 1 | 0.91 (0.86–1.00) | 0.59 (0.55–0.62) | libvmx | libvmx 128-bit |
| Decode | 8 | **1.31 (1.19–1.43)** | 0.81 (0.75–0.86) | vmx | libvmx 128-bit |

Eight threads here share four cores. The 8-thread encode mean of 1.09 is much closer to libvmx than the same ratio on the 8-core c4a below. vmx-codec's SSE2 path is also behind libvmx's 128-bit path on this machine (geometric mean 0.58, 0.48, 0.54, and 0.35 for encode 1 thread, encode 8, decode 1, and decode 8). Several 8-thread colour-bar decode rows are higher for the 128-bit path than for AVX2 libvmx or vmx; those cells are the bold ones.

### c4a-standard-8 (8 cores)

| Operation | Threads | vmx / libvmx | vmx-codec / libvmx | Fastest |
|---|---:|---:|---:|---|
| Encode | 1 | 0.94 (0.93–0.97) | **1.07 (1.05–1.09)** | vmx-codec |
| Encode | 8 | **1.85 (1.76–1.94)** | 1.66 (1.59–1.79) | vmx |
| Decode | 1 | 0.74 (0.66–0.83) | 0.76 (0.60–0.97) | libvmx |
| Decode | 8 | **1.40 (1.16–1.64)** | 0.80 (0.45–1.44) | vmx |

vmx / libvmx by image:

| Operation | Threads | natural | bars | Fastest (natural) | Fastest (bars) |
|---|---:|---:|---:|---|---|
| Encode | 1 | 0.95 (0.93–0.97) | 0.93 (0.93–0.93) | vmx-codec | vmx-codec |
| Encode | 8 | **1.82 (1.76–1.90)** | **1.87 (1.78–1.94)** | vmx | vmx |
| Decode | 1 | 0.80 (0.78–0.83) | 0.68 (0.66–0.71) | libvmx | libvmx |
| Decode | 8 | **1.59 (1.53–1.64)** | **1.23 (1.16–1.28)** | vmx | vmx |

The 1-thread encode ratios sit in a band of a few percent. The within-run spread on this machine is usually well under that (median `(max − min) / median` about 0.3%). vmx-codec's 8-thread decode mean of 0.80 is pulled down by colour bars (0.45–0.48); on natural images that ratio is 1.39 (1.30–1.44).

## x86-64 — c4-standard-8

Xeon Platinum 8581C, 4 cores / 8 threads. 192 rows, all byte-identical to libvmx 128-bit. CSV: [`bench/results/gcp-x86-c4.csv`](bench/results/gcp-x86-c4.csv).


### Encode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | libvmx 128-bit | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | **451** | 305 | 194 | 353 | libvmx |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | **360** | 238 | 160 | 247 | libvmx |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | **307** | 206 | 140 | 208 | libvmx |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | **703** | 440 | 232 | 523 | libvmx |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | **712** | 440 | 231 | 524 | libvmx |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | **706** | 439 | 230 | 523 | libvmx |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | **111** | 79 | 50 ✗ | 90 | libvmx |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | **111** | 79 | 49 ✗ | 91 | libvmx |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | **103** | 69 | 45 ✗ | 74 | libvmx |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | **174** | 111 | 59 ✗ | 131 | libvmx |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | **176** | 111 | 58 ✗ | 131 | libvmx |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | **175** | 111 | 58 ✗ | 131 | libvmx |

### Encode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx | libvmx 128-bit | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | 1409 | 957 | 488 | **1657** | vmx |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | 1152 | 801 | 433 | **1179** | vmx |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | 950 | 692 | 398 | **1010** | vmx |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | 2130 | 1364 | 544 | **2160** | vmx |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | **2158** | 1343 | 531 | 2104 | libvmx |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | 2100 | 1304 | 543 | **2132** | vmx |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | 353 | 269 | 138 | **453** | vmx |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | 356 | 255 | 140 | **454** | vmx |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | 328 | 237 | 131 | **377** | vmx |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | 517 | 344 | 153 | **560** | vmx |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | 538 | 351 | 150 | **562** | vmx |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | 533 | 338 | 152 | **566** | vmx |

### Decode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | libvmx 128-bit | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | **488** | 416 | 306 | 426 | libvmx |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | **274** | 240 | 164 | 262 | libvmx |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | **215** | 190 | 122 | 214 | libvmx |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | 1861 | **1933** | 826 | 1159 | libvmx 128-bit |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | 1853 | **1996** | 826 | 1116 | libvmx 128-bit |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | 1880 | **1997** | 812 | 1090 | libvmx 128-bit |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | **132** | 114 | 83 | 114 | libvmx |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | **132** | 115 | 82 | 113 | libvmx |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | **92** | 80 | 56 ✗ | 83 | libvmx |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | 498 | **552** | 238 | 293 | libvmx 128-bit |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | 514 | **558** | 237 | 292 | libvmx 128-bit |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | 511 | **557** | 232 | 283 | libvmx 128-bit |

### Decode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx | libvmx 128-bit | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | 1538 | 1284 | 743 | **1832** | vmx |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | 915 | 783 | 506 | **1237** | vmx |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | 728 | 637 | 425 | **1042** | vmx |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | 4940 | **5265** | 952 | 4055 | libvmx 128-bit |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | **5063** | 4843 | 936 | 3787 | libvmx |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | 4745 | **5117** | 957 | 3865 | libvmx 128-bit |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | 441 | 401 | 223 | **557** | vmx |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | 451 | 373 | 220 | **567** | vmx |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | 308 | 264 | 184 | **427** | vmx |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | 1483 | **1585** | 323 | 1230 | libvmx 128-bit |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | 1520 | **1592** | 318 | 1238 | libvmx 128-bit |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | 1420 | **1615** | 324 | 1227 | libvmx 128-bit |

## ARM64 — c4a-standard-8

Google Axion, Neoverse-V2, 8 cores. 144 rows, all byte-identical to libvmx 128-bit. One libvmx column (sse2neon). CSV: [`bench/results/gcp-arm-c4a.csv`](bench/results/gcp-arm-c4a.csv).


### Encode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | 183 | **200** | 177 | vmx-codec |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | 156 | **167** | 146 | vmx-codec |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | 140 | **147** | 130 | vmx-codec |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | 230 | **243** | 215 | vmx-codec |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | 230 | **245** | 214 | vmx-codec |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | 230 | **244** | 214 | vmx-codec |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | 47 ✗ | **50 ✗** | 45 ✗ | vmx-codec |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | 47 ✗ | **50 ✗** | 45 ✗ | vmx-codec |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | 43 ✗ | **46 ✗** | 40 ✗ | vmx-codec |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | 58 ✗ | **61** | 54 ✗ | vmx-codec |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | 58 ✗ | **62** | 54 ✗ | vmx-codec |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | 58 ✗ | **61** | 54 ✗ | vmx-codec |

### Encode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | 710 | 1132 | **1282** | vmx |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | 571 | 976 | **1056** | vmx |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | 505 | 874 | **940** | vmx |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | 805 | 1309 | **1543** | vmx |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | 814 | 1321 | **1542** | vmx |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | 797 | 1308 | **1548** | vmx |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | 198 | 320 | **350** | vmx |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | 198 | 318 | **350** | vmx |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | 164 | 295 | **312** | vmx |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | 231 | 385 | **420** | vmx |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | 237 | 383 | **421** | vmx |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | 226 | 384 | **419** | vmx |

### Decode, 1 thread

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | **315** | 306 | 246 | libvmx |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | **191** | 173 | 155 | libvmx |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | **153** | 132 | 128 | libvmx |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | **2135** | 1353 | 1518 | libvmx |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | **2197** | 1405 | 1499 | libvmx |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | **2196** | 1368 | 1512 | libvmx |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | **86** | 82 | 67 | libvmx |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | **86** | 82 | 66 | libvmx |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | **62** | 58 ✗ | 50 ✗ | libvmx |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | **574** | 345 | 380 | libvmx |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | **578** | 351 | 389 | libvmx |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | **577** | 345 | 379 | libvmx |

### Decode, 8 threads

| Resolution | Profile | Image | bytes/frame | libvmx | vmx-codec | vmx | Fastest |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| 1080p | OMT_LQ — Low (92) | natural | 168,577 | 1098 | 1423 | **1684** | vmx |
| 1080p | OMT_SQ — Medium (95) | natural | 418,883 | 699 | 964 | **1096** | vmx |
| 1080p | OMT_HQ — High (96) | natural | 559,463 | 561 | 786 | **907** | vmx |
| 1080p | OMT_LQ — Low (98) | bars | 22,008 | 5646 | 2567 | **6716** | vmx |
| 1080p | OMT_SQ — Medium (98) | bars | 24,303 | 5730 | 2600 | **6659** | vmx |
| 1080p | OMT_HQ — High (98) | bars | 24,303 | 5639 | 2532 | **6668** | vmx |
| 2160p | OMT_LQ — Low (92) | natural | 595,921 | 307 | 438 | **502** | vmx |
| 2160p | OMT_SQ — Medium (92) | natural | 595,921 | 313 | 439 | **503** | vmx |
| 2160p | OMT_HQ — High (94) | natural | 1,192,159 | 237 | 342 | **378** | vmx |
| 2160p | OMT_LQ — Low (98) | bars | 76,145 | 1789 | 856 | **2280** | vmx |
| 2160p | OMT_SQ — Medium (98) | bars | 76,145 | 1789 | 855 | **2290** | vmx |
| 2160p | OMT_HQ — High (98) | bars | 80,733 | 1790 | 860 | **2288** | vmx |

## wgpu — NVIDIA L4

`g2-standard-4` Spot in `asia-northeast1-c`, Ubuntu 24.04, NVIDIA driver 580.178.04, one L4. The measured binary is `e1c68a2`. Command: `OUT=/tmp/gpu.csv GPU_BENCH_ITERS=20 bash bench/scripts/run_gpu.sh`. Every process reported adapter `NVIDIA L4`, backend Vulkan, device type `DiscreteGpu`, and the run exited 0. The log shows the discarded pass and `--prime 20`. The medians exclude that warmup.

The frames are the gzip UYVY files in [`bench/images/`](bench/images/README.md): `1080p-natural.uyvy.gz`, `1080p-bars.uyvy.gz`, `2160p-natural.uyvy.gz`, and `2160p-bars.uyvy.gz`. `examples/gpu_bench.rs` converts UYVY to BGRA on the CPU before the timed loops. That conversion is not in the samples. q is the quality after 60 encode+save frames on that UYVY, then pinned with `set_quality`, the same rate-control pass as the CPU tables. The profile cell is that q. CSV: [`bench/results/gcp-gpu-l4-images.csv`](bench/results/gcp-gpu-l4-images.csv). `ms_samples` is empty. `ms_median` is already rounded to 2 decimal places and `fps_median` to an integer, so `1000/ms` does not always reproduce the fps. See [`bench/results/README.md`](bench/results/README.md).

The CPU columns are the same `vmx` build on this VM's 4 vCPU, with the codec's default thread count capped at those 4 threads. 1080p takes 20 timed samples and 2160p takes 10.

Fastest and bold compare only the GPU path with the CPU path that includes the texture transfer. The host-pixel column is a reference and is not bold. Opaque encode has no CPU-plus-transfer figure, so those rows have no Fastest name and the ratio is against host pixels.

| Resolution | Image | Profile | Operation | CPU, host pixels | CPU + texture transfer | GPU (wgpu) | GPU / CPU+transfer | Fastest |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| 1080p | natural | OMT_LQ — Low (92) | Decode → BGRA texture | 476 fps (2.10 ms) | 275 fps (3.64 ms) | **446 fps (2.24 ms)** | 1.62× | GPU (wgpu) |
| 1080p | natural | OMT_LQ — Low (92) | Preview (1/8) → texture | 1446 fps (0.69 ms) | **1400 fps (0.71 ms)** | 1252 fps (0.80 ms) | 0.89× | CPU + texture transfer |
| 1080p | natural | OMT_LQ — Low (92) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 255 fps (3.92 ms) | 166 fps (6.02 ms) | **174 fps (5.76 ms)** | 1.05× | GPU (wgpu) |
| 1080p | natural | OMT_LQ — Low (92) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 316 fps (3.17 ms) | — | 245 fps (4.08 ms) | 0.78× vs host pixels | — |
| 1080p | natural | OMT_SQ — Medium (95) | Decode → BGRA texture | 348 fps (2.87 ms) | 227 fps (4.41 ms) | **340 fps (2.94 ms)** | 1.50× | GPU (wgpu) |
| 1080p | natural | OMT_SQ — Medium (95) | Preview (1/8) → texture | 1280 fps (0.78 ms) | **1245 fps (0.80 ms)** | 1144 fps (0.87 ms) | 0.92× | CPU + texture transfer |
| 1080p | natural | OMT_SQ — Medium (95) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 216 fps (4.62 ms) | 150 fps (6.65 ms) | **154 fps (6.47 ms)** | 1.03× | GPU (wgpu) |
| 1080p | natural | OMT_SQ — Medium (95) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 258 fps (3.88 ms) | — | 208 fps (4.81 ms) | 0.81× vs host pixels | — |
| 1080p | natural | OMT_HQ — High (96) | Decode → BGRA texture | 307 fps (3.26 ms) | 210 fps (4.76 ms) | **299 fps (3.35 ms)** | 1.42× | GPU (wgpu) |
| 1080p | natural | OMT_HQ — High (96) | Preview (1/8) → texture | 1292 fps (0.77 ms) | **1260 fps (0.79 ms)** | 1172 fps (0.85 ms) | 0.93× | CPU + texture transfer |
| 1080p | natural | OMT_HQ — High (96) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 195 fps (5.12 ms) | 140 fps (7.15 ms) | **143 fps (6.99 ms)** | 1.02× | GPU (wgpu) |
| 1080p | natural | OMT_HQ — High (96) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 228 fps (4.38 ms) | — | 188 fps (5.33 ms) | 0.82× vs host pixels | — |
| 1080p | bars | OMT_LQ — Low (98) | Decode → BGRA texture | 864 fps (1.16 ms) | 375 fps (2.67 ms) | **594 fps (1.68 ms)** | 1.58× | GPU (wgpu) |
| 1080p | bars | OMT_LQ — Low (98) | Preview (1/8) → texture | 3480 fps (0.29 ms) | **3236 fps (0.31 ms)** | 2692 fps (0.37 ms) | 0.83× | CPU + texture transfer |
| 1080p | bars | OMT_LQ — Low (98) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 297 fps (3.36 ms) | 184 fps (5.42 ms) | **192 fps (5.20 ms)** | 1.04× | GPU (wgpu) |
| 1080p | bars | OMT_LQ — Low (98) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 384 fps (2.60 ms) | — | 284 fps (3.52 ms) | 0.74× vs host pixels | — |
| 1080p | bars | OMT_SQ — Medium (98) | Decode → BGRA texture | 864 fps (1.16 ms) | 365 fps (2.74 ms) | **596 fps (1.68 ms)** | 1.63× | GPU (wgpu) |
| 1080p | bars | OMT_SQ — Medium (98) | Preview (1/8) → texture | 3502 fps (0.29 ms) | **3253 fps (0.31 ms)** | 2500 fps (0.40 ms) | 0.77× | CPU + texture transfer |
| 1080p | bars | OMT_SQ — Medium (98) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 296 fps (3.38 ms) | 183 fps (5.48 ms) | **193 fps (5.18 ms)** | 1.05× | GPU (wgpu) |
| 1080p | bars | OMT_SQ — Medium (98) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 383 fps (2.61 ms) | — | 283 fps (3.53 ms) | 0.74× vs host pixels | — |
| 1080p | bars | OMT_HQ — High (98) | Decode → BGRA texture | 853 fps (1.17 ms) | 371 fps (2.69 ms) | **591 fps (1.69 ms)** | 1.59× | GPU (wgpu) |
| 1080p | bars | OMT_HQ — High (98) | Preview (1/8) → texture | 3479 fps (0.29 ms) | **3209 fps (0.31 ms)** | 2703 fps (0.37 ms) | 0.84× | CPU + texture transfer |
| 1080p | bars | OMT_HQ — High (98) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 296 fps (3.38 ms) | 183 fps (5.46 ms) | **192 fps (5.21 ms)** | 1.05× | GPU (wgpu) |
| 1080p | bars | OMT_HQ — High (98) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 382 fps (2.62 ms) | — | 282 fps (3.54 ms) | 0.74× vs host pixels | — |
| 2160p | natural | OMT_LQ — Low (92) | Decode → BGRA texture | 123 fps (8.10 ms) | 69 fps (14.41 ms) | **105 fps (9.51 ms)** | 1.52× | GPU (wgpu) |
| 2160p | natural | OMT_LQ — Low (92) | Preview (1/8) → texture | 373 fps (2.68 ms) | **361 fps (2.77 ms)** | 346 fps (2.89 ms) | 0.96× | CPU + texture transfer |
| 2160p | natural | OMT_LQ — Low (92) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 64 fps (15.54 ms) | 37 fps ✗ (27.18 ms) | **43 fps ✗ (23.12 ms)** | 1.16× | GPU (wgpu) |
| 2160p | natural | OMT_LQ — Low (92) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 80 fps (12.46 ms) | — | 61 fps (16.41 ms) | 0.76× vs host pixels | — |
| 2160p | natural | OMT_SQ — Medium (92) | Decode → BGRA texture | 124 fps (8.08 ms) | 70 fps (14.34 ms) | **97 fps (10.33 ms)** | 1.39× | GPU (wgpu) |
| 2160p | natural | OMT_SQ — Medium (92) | Preview (1/8) → texture | 373 fps (2.68 ms) | **360 fps (2.78 ms)** | 345 fps (2.90 ms) | 0.96× | CPU + texture transfer |
| 2160p | natural | OMT_SQ — Medium (92) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 65 fps (15.38 ms) | 37 fps ✗ (27.23 ms) | **42 fps ✗ (23.93 ms)** | 1.14× | GPU (wgpu) |
| 2160p | natural | OMT_SQ — Medium (92) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 81 fps (12.38 ms) | — | 59 fps ✗ (16.97 ms) | 0.73× vs host pixels | — |
| 2160p | natural | OMT_HQ — High (94) | Decode → BGRA texture | 101 fps (9.90 ms) | 62 fps (16.13 ms) | **84 fps (11.86 ms)** | 1.35× | GPU (wgpu) |
| 2160p | natural | OMT_HQ — High (94) | Preview (1/8) → texture | 334 fps (3.00 ms) | **324 fps (3.09 ms)** | 314 fps (3.18 ms) | 0.97× | CPU + texture transfer |
| 2160p | natural | OMT_HQ — High (94) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 59 fps ✗ (16.91 ms) | 35 fps ✗ (28.31 ms) | **38 fps ✗ (26.55 ms)** | 1.09× | GPU (wgpu) |
| 2160p | natural | OMT_HQ — High (94) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 72 fps (13.82 ms) | — | 52 fps ✗ (19.37 ms) | 0.72× vs host pixels | — |
| 2160p | bars | OMT_LQ — Low (98) | Decode → BGRA texture | 216 fps (4.63 ms) | 92 fps (10.84 ms) | **157 fps (6.35 ms)** | 1.71× | GPU (wgpu) |
| 2160p | bars | OMT_LQ — Low (98) | Preview (1/8) → texture | 879 fps (1.14 ms) | **816 fps (1.23 ms)** | 750 fps (1.33 ms) | 0.92× | CPU + texture transfer |
| 2160p | bars | OMT_LQ — Low (98) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 74 fps (13.48 ms) | 40 fps ✗ (24.73 ms) | **48 fps ✗ (20.63 ms)** | 1.20× | GPU (wgpu) |
| 2160p | bars | OMT_LQ — Low (98) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 96 fps (10.45 ms) | — | 71 fps (14.08 ms) | 0.74× vs host pixels | — |
| 2160p | bars | OMT_SQ — Medium (98) | Decode → BGRA texture | 218 fps (4.60 ms) | 91 fps (10.96 ms) | **160 fps (6.23 ms)** | 1.76× | GPU (wgpu) |
| 2160p | bars | OMT_SQ — Medium (98) | Preview (1/8) → texture | 860 fps (1.16 ms) | **806 fps (1.24 ms)** | 747 fps (1.34 ms) | 0.93× | CPU + texture transfer |
| 2160p | bars | OMT_SQ — Medium (98) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 74 fps (13.50 ms) | 40 fps ✗ (24.99 ms) | **49 fps ✗ (20.55 ms)** | 1.23× | GPU (wgpu) |
| 2160p | bars | OMT_SQ — Medium (98) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 95 fps (10.56 ms) | — | 71 fps (14.04 ms) | 0.75× vs host pixels | — |
| 2160p | bars | OMT_HQ — High (98) | Decode → BGRA texture | 212 fps (4.72 ms) | 90 fps (11.04 ms) | **161 fps (6.20 ms)** | 1.79× | GPU (wgpu) |
| 2160p | bars | OMT_HQ — High (98) | Preview (1/8) → texture | 863 fps (1.16 ms) | **807 fps (1.24 ms)** | 749 fps (1.33 ms) | 0.93× | CPU + texture transfer |
| 2160p | bars | OMT_HQ — High (98) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 74 fps (13.53 ms) | 40 fps ✗ (25.15 ms) | **49 fps ✗ (20.58 ms)** | 1.23× | GPU (wgpu) |
| 2160p | bars | OMT_HQ — High (98) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 95 fps (10.53 ms) | — | 71 fps (14.07 ms) | 0.75× vs host pixels | — |

Host↔GPU copies are limited by PCIe and memory bandwidth, and that transfer is the bottleneck in the CPU-plus-transfer column. On the 1080p natural OMT_SQ decode row, host pixels take 2.87 ms and the same decode plus `write_texture` takes 4.41 ms, so the upload is most of that column. The matching encode row is 4.62 ms on host pixels and 6.65 ms once `read_texture` is included. When the texture stays on the GPU (`decode_to_texture` feeding a later `encode_from_texture`, with no host round trip each frame), the GPU path does not pay that transfer and is the faster way to keep the frame on the device.

GPU preview and GPU encode match the CPU path on all 12 image × profile cells (PSNR 100 dB, max abs 0). GPU decode matches on every `bars` cell. `natural` decode does not, and the gap grows as q rises. At 1080p, PSNR falls from 45.38 dB at q 92 to 39.82 dB at q 95 and 38.62 dB at q 96. At 2160p, OMT_LQ and OMT_SQ share q 92 and the same 45.76 dB; OMT_HQ at q 94 is 41.49 dB.

| Resolution | Profile | PSNR | MAE | max abs |
| --- | --- | ---: | ---: | ---: |
| 1080p | OMT_LQ — Low (92) | 45.38 dB | 0.58 | 35 |
| 1080p | OMT_SQ — Medium (95) | 39.82 dB | 1.92 | 33 |
| 1080p | OMT_HQ — High (96) | 38.62 dB | 2.27 | 35 |
| 2160p | OMT_LQ — Low (92) | 45.76 dB | 0.57 | 38 |
| 2160p | OMT_SQ — Medium (92) | 45.76 dB | 0.57 | 38 |
| 2160p | OMT_HQ — High (94) | 41.49 dB | 1.47 | 35 |

Encode does colour conversion, FDCT, and quantisation on the GPU; Golomb coding stays on the CPU. Preview is the CPU `decode_preview_bgra` plus `queue.write_texture`.

Reference, not the table above: an earlier `OMT_SQ` synthetic gradient, filled in memory (`blue = x % 256`, `green = y % 256`, `red = 128`, `alpha = 255`; not a file under `bench/images/`), is in [`bench/results/gcp-gpu-l4.csv`](bench/results/gcp-gpu-l4.csv). The image table is the L4 run of the stored frames.

## Earlier shared-runner results

These are not the primary numbers. They were taken on shared machines, and libvmx was built with clang 19.1.7 rather than the clang 14.0.6 used on the Debian 12 GCP VMs above.

- A KVM guest, Xeon Emerald Rapids, 8 vCPU and no SMT, Debian 13: [`x86-64-default.csv`](bench/results/x86-64-default.csv) (384 rows, including LQ/SQ/HQ) and a partial `target-cpu=native` pass in [`x86-64-native.csv`](bench/results/x86-64-native.csv). Repeat runs on that host moved by about 10–20%.
- GitHub-hosted `ubuntu-24.04-arm` (Neoverse-N2, 4 vCPU): [`linux-arm64.csv`](bench/results/linux-arm64.csv).
- GitHub-hosted `macos-latest` (Apple M1, virtual, 3 vCPU): [`macos-arm64.csv`](bench/results/macos-arm64.csv). Some 3-thread rows on that VM had a seven-run spread large enough that the median is a weak summary.

LQ, SQ, and HQ (ids 33, 66, 99) appear only in those older CSVs. The dedicated-machine runs did not measure them. A later GCP run limited to the OMT profiles is what this file now reports; another dedicated pass can still replace these tables.

## Reproducing

```bash
cd bench
cargo run --release > results.csv
```

`clang++`, `git`, and `ar` are required. Filters: `--res 1080`, `--prof OMT_HQ`, `--img natural`, `--threads 1`. [`bench/scripts/run_omt.sh`](bench/scripts/run_omt.sh) is the resume script from the GCP OMT-only pass. [`bench/scripts/make_tables.py`](bench/scripts/make_tables.py) turns a harness CSV into the table shape used above. The L4 image pass is [`bench/scripts/run_gpu.sh`](bench/scripts/run_gpu.sh) (`OUT=/tmp/gpu.csv GPU_BENCH_ITERS=20`).
