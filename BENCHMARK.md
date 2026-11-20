# Benchmark

CPU numbers below are from dedicated GCP Spot VMs in Tokyo (`asia-northeast1`) on 2026-11-20. They replace the earlier shared-VM and GitHub Actions tables as the primary result. Each table is one pass. A gap of a few percent is one sample, not a settled ordering.

Fps columns, left to right, are libvmx (official), vmx-codec (Saul), and vmx. The x86-64 tables insert libvmx 128-bit after the AVX2 column. Every comparison table ends with a Fastest column naming the winner on that row (`libvmx`, `libvmx 128-bit`, `vmx-codec`, `vmx`, or, in the wgpu table, the path name). **Bold** is that winner's value; a tie bolds each tied value and lists every name. In the fps tables the winner is the highest median. In the ratio tables the winner is the highest geometric mean of fps over the rows in that line; libvmx's ratio is 1 and is not printed, and the winner's ratio cell is bold when the winner is vmx or vmx-codec. In the wgpu table, Fastest and bold ignore the host-pixel reference and name the faster of the GPU path and the CPU path that includes the texture transfer. A † on a wgpu ratio is the cold first CPU-plus-transfer decode at that resolution; that ratio is not a settled GPU advantage. The profile cell is the OMT name, the sender's quality name, and the fixed q, as in `OMT_HQ — High (96)`. `✗` is a median below 60 fps.

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

`g2-standard-4` Spot in `asia-northeast1-c`, Ubuntu 24.04, NVIDIA driver 580.178.04, one L4. The measured binary is `af1f5e3`. Command: `OUT=/tmp/gpu.csv GPU_BENCH_ITERS=20 bash bench/scripts/run_gpu.sh`. Every process reported adapter `NVIDIA L4`, backend Vulkan, device type `DiscreteGpu`. 1080p takes 20 timed samples and 2160p takes 10. The median excludes the per-case warm-up, `(iters/5)` clamped to 2..=8, which is 4 samples at 1080p and 2 at 2160p. That binary did not run the longer untimed prime described below.

The frames are the gzip UYVY files in [`bench/images/`](bench/images/README.md): `1080p-natural.uyvy.gz`, `1080p-bars.uyvy.gz`, `2160p-natural.uyvy.gz`, and `2160p-bars.uyvy.gz`. `examples/gpu_bench.rs` converts UYVY to BGRA on the CPU before the timed loops. That conversion is not in the samples. CSV: [`bench/results/gcp-gpu-l4-images.csv`](bench/results/gcp-gpu-l4-images.csv). `ms_samples` is empty. The cells below are the logged `ms_median` and `fps_median`, not a fresh rounding of each other.

The CPU columns are the same `vmx` build on this VM's 4 vCPU, with the codec's default thread count capped at those 4 threads. `Codec::new` calls `set_quality(80)`. The OMT minimum is 52, so the q in the profile cell is 80. It is the starting q, not the settled rate-control q in the CPU tables. Decode uses the packet from the first `save_to`, written at that q. GPU encode does not call `save_to`, so it stays at 80. The CPU encode instance calls `save_to` once before timing, which can step q; that stepped value was not logged.

Fastest and bold compare only the GPU path with the CPU path that includes the texture transfer. The host-pixel column is a reference and is not bold. Opaque encode has no CPU-plus-transfer figure, so those rows have no Fastest name and the ratio is against host pixels.

The script measures natural before bars and `OMT_LQ` before `OMT_SQ` and `OMT_HQ`. The first profile at each resolution, natural × OMT_LQ, has a decode CPU-plus-transfer median far below the same image at the other two profiles (1080p 92.4 fps against about 330; 2160p 24.4 fps against about 84). That is a cold first transfer. The † ratios are not a settled GPU advantage. The other rows from those same processes are not marked.

| Resolution | Image | Profile | Operation | CPU, host pixels | CPU + texture transfer | GPU (wgpu) | GPU / CPU+transfer | Fastest |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| 1080p | natural | OMT_LQ — Low (80) | Decode → BGRA texture | 718.7 fps (1.391 ms) | 92.4 fps (10.823 ms) | **574.7 fps (1.740 ms)** | 6.22× † | GPU (wgpu) |
| 1080p | natural | OMT_LQ — Low (80) | Preview (1/8) → texture | 1485.7 fps (0.673 ms) | **1430.5 fps (0.699 ms)** | 1310.0 fps (0.763 ms) | 0.92× | CPU + texture transfer |
| 1080p | natural | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 286.3 fps (3.493 ms) | 177.8 fps (5.625 ms) | **187.8 fps (5.326 ms)** | 1.06× | GPU (wgpu) |
| 1080p | natural | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 363.6 fps (2.751 ms) | — | 272.5 fps (3.670 ms) | 0.75× vs host pixels | — |
| 1080p | natural | OMT_SQ — Medium (80) | Decode → BGRA texture | 682.1 fps (1.466 ms) | 329.9 fps (3.031 ms) | **555.4 fps (1.801 ms)** | 1.68× | GPU (wgpu) |
| 1080p | natural | OMT_SQ — Medium (80) | Preview (1/8) → texture | 1326.3 fps (0.754 ms) | **1274.7 fps (0.784 ms)** | 1178.2 fps (0.849 ms) | 0.92× | CPU + texture transfer |
| 1080p | natural | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 282.5 fps (3.540 ms) | 177.2 fps (5.643 ms) | **187.6 fps (5.332 ms)** | 1.06× | GPU (wgpu) |
| 1080p | natural | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 360.0 fps (2.778 ms) | — | 272.8 fps (3.665 ms) | 0.76× vs host pixels | — |
| 1080p | natural | OMT_HQ — High (80) | Decode → BGRA texture | 687.9 fps (1.454 ms) | 330.0 fps (3.031 ms) | **558.0 fps (1.792 ms)** | 1.69× | GPU (wgpu) |
| 1080p | natural | OMT_HQ — High (80) | Preview (1/8) → texture | 1324.1 fps (0.755 ms) | **1270.6 fps (0.787 ms)** | 1138.3 fps (0.878 ms) | 0.90× | CPU + texture transfer |
| 1080p | natural | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 282.8 fps (3.536 ms) | 178.9 fps (5.590 ms) | **187.0 fps (5.348 ms)** | 1.05× | GPU (wgpu) |
| 1080p | natural | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 361.3 fps (2.768 ms) | — | 273.1 fps (3.661 ms) | 0.76× vs host pixels | — |
| 1080p | bars | OMT_LQ — Low (80) | Decode → BGRA texture | 860.0 fps (1.163 ms) | 364.3 fps (2.745 ms) | **648.7 fps (1.542 ms)** | 1.78× | GPU (wgpu) |
| 1080p | bars | OMT_LQ — Low (80) | Preview (1/8) → texture | 3523.8 fps (0.284 ms) | **3234.1 fps (0.309 ms)** | 2493.3 fps (0.401 ms) | 0.77× | CPU + texture transfer |
| 1080p | bars | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 297.5 fps (3.361 ms) | 185.5 fps (5.390 ms) | **192.4 fps (5.197 ms)** | 1.04× | GPU (wgpu) |
| 1080p | bars | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 378.5 fps (2.642 ms) | — | 280.2 fps (3.568 ms) | 0.74× vs host pixels | — |
| 1080p | bars | OMT_SQ — Medium (80) | Decode → BGRA texture | 857.4 fps (1.166 ms) | 366.6 fps (2.727 ms) | **650.9 fps (1.536 ms)** | 1.78× | GPU (wgpu) |
| 1080p | bars | OMT_SQ — Medium (80) | Preview (1/8) → texture | 3494.4 fps (0.286 ms) | **3220.3 fps (0.311 ms)** | 2649.4 fps (0.377 ms) | 0.82× | CPU + texture transfer |
| 1080p | bars | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 294.8 fps (3.392 ms) | 182.4 fps (5.483 ms) | **192.3 fps (5.200 ms)** | 1.05× | GPU (wgpu) |
| 1080p | bars | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 381.0 fps (2.624 ms) | — | 282.3 fps (3.542 ms) | 0.74× vs host pixels | — |
| 1080p | bars | OMT_HQ — High (80) | Decode → BGRA texture | 851.4 fps (1.175 ms) | 366.4 fps (2.729 ms) | **653.5 fps (1.530 ms)** | 1.78× | GPU (wgpu) |
| 1080p | bars | OMT_HQ — High (80) | Preview (1/8) → texture | 3504.9 fps (0.285 ms) | **3218.2 fps (0.311 ms)** | 2501.3 fps (0.400 ms) | 0.78× | CPU + texture transfer |
| 1080p | bars | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 297.6 fps (3.361 ms) | 184.3 fps (5.425 ms) | **193.2 fps (5.176 ms)** | 1.05× | GPU (wgpu) |
| 1080p | bars | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 383.9 fps (2.605 ms) | — | 282.7 fps (3.537 ms) | 0.74× vs host pixels | — |
| 2160p | natural | OMT_LQ — Low (80) | Decode → BGRA texture | 185.5 fps (5.390 ms) | 24.4 fps ✗ (40.914 ms) | **172.4 fps (5.800 ms)** | 7.07× † | GPU (wgpu) |
| 2160p | natural | OMT_LQ — Low (80) | Preview (1/8) → texture | 376.7 fps (2.655 ms) | **365.6 fps (2.735 ms)** | 353.7 fps (2.827 ms) | 0.97× | CPU + texture transfer |
| 2160p | natural | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 72.0 fps (13.890 ms) | 39.7 fps ✗ (25.196 ms) | **48.9 fps ✗ (20.442 ms)** | 1.23× | GPU (wgpu) |
| 2160p | natural | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 91.7 fps (10.900 ms) | — | 71.5 fps (13.979 ms) | 0.78× vs host pixels | — |
| 2160p | natural | OMT_SQ — Medium (80) | Decode → BGRA texture | 187.5 fps (5.333 ms) | 84.8 fps (11.790 ms) | **170.2 fps (5.875 ms)** | 2.01× | GPU (wgpu) |
| 2160p | natural | OMT_SQ — Medium (80) | Preview (1/8) → texture | 375.5 fps (2.663 ms) | **364.8 fps (2.742 ms)** | 350.7 fps (2.852 ms) | 0.96× | CPU + texture transfer |
| 2160p | natural | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 71.5 fps (13.984 ms) | 38.7 fps ✗ (25.841 ms) | **48.8 fps ✗ (20.484 ms)** | 1.26× | GPU (wgpu) |
| 2160p | natural | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 91.6 fps (10.912 ms) | — | 71.5 fps (13.982 ms) | 0.78× vs host pixels | — |
| 2160p | natural | OMT_HQ — High (80) | Decode → BGRA texture | 178.1 fps (5.614 ms) | 83.1 fps (12.029 ms) | **164.4 fps (6.081 ms)** | 1.98× | GPU (wgpu) |
| 2160p | natural | OMT_HQ — High (80) | Preview (1/8) → texture | 341.4 fps (2.929 ms) | **326.0 fps (3.067 ms)** | 320.1 fps (3.124 ms) | 0.98× | CPU + texture transfer |
| 2160p | natural | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 71.9 fps (13.918 ms) | 38.4 fps ✗ (26.071 ms) | **48.9 fps ✗ (20.430 ms)** | 1.27× | GPU (wgpu) |
| 2160p | natural | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 92.2 fps (10.844 ms) | — | 71.5 fps (13.989 ms) | 0.78× vs host pixels | — |
| 2160p | bars | OMT_LQ — Low (80) | Decode → BGRA texture | 213.5 fps (4.684 ms) | 89.5 fps (11.171 ms) | **192.9 fps (5.184 ms)** | 2.16× | GPU (wgpu) |
| 2160p | bars | OMT_LQ — Low (80) | Preview (1/8) → texture | 875.7 fps (1.142 ms) | **816.3 fps (1.225 ms)** | 760.6 fps (1.315 ms) | 0.93× | CPU + texture transfer |
| 2160p | bars | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 74.4 fps (13.443 ms) | 39.2 fps ✗ (25.511 ms) | **49.9 fps ✗ (20.052 ms)** | 1.27× | GPU (wgpu) |
| 2160p | bars | OMT_LQ — Low (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 95.6 fps (10.464 ms) | — | 73.7 fps (13.577 ms) | 0.77× vs host pixels | — |
| 2160p | bars | OMT_SQ — Medium (80) | Decode → BGRA texture | 212.7 fps (4.701 ms) | 90.1 fps (11.097 ms) | **192.8 fps (5.186 ms)** | 2.14× | GPU (wgpu) |
| 2160p | bars | OMT_SQ — Medium (80) | Preview (1/8) → texture | 841.1 fps (1.189 ms) | **807.6 fps (1.238 ms)** | 750.8 fps (1.332 ms) | 0.93× | CPU + texture transfer |
| 2160p | bars | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 74.3 fps (13.462 ms) | 40.3 fps ✗ (24.825 ms) | **49.8 fps ✗ (20.080 ms)** | 1.24× | GPU (wgpu) |
| 2160p | bars | OMT_SQ — Medium (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 95.3 fps (10.490 ms) | — | 73.8 fps (13.548 ms) | 0.77× vs host pixels | — |
| 2160p | bars | OMT_HQ — High (80) | Decode → BGRA texture | 211.4 fps (4.730 ms) | 90.2 fps (11.091 ms) | **192.8 fps (5.187 ms)** | 2.14× | GPU (wgpu) |
| 2160p | bars | OMT_HQ — High (80) | Preview (1/8) → texture | 866.4 fps (1.154 ms) | **813.6 fps (1.229 ms)** | 754.6 fps (1.325 ms) | 0.93× | CPU + texture transfer |
| 2160p | bars | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (alpha, 4 planes) | 74.1 fps (13.497 ms) | 39.9 fps ✗ (25.084 ms) | **49.8 fps ✗ (20.076 ms)** | 1.25× | GPU (wgpu) |
| 2160p | bars | OMT_HQ — High (80) | Encode, BGRA texture → bitstream (opaque, 3 planes) | 95.4 fps (10.485 ms) | — | 73.5 fps (13.609 ms) | 0.77× vs host pixels | — |

GPU preview and GPU encode match the CPU path on all 12 image × profile cells (PSNR 100 dB, max abs 0). GPU decode matches on every `bars` cell. `natural` decode does not:

| Resolution | Profile | PSNR | MAE | max abs |
| --- | --- | ---: | ---: | ---: |
| 1080p | OMT_LQ — Low (80) | 56.08 dB | 0.02 | 30 |
| 1080p | OMT_SQ — Medium (80) | 50.79 dB | 0.25 | 30 |
| 1080p | OMT_HQ — High (80) | 50.79 dB | 0.25 | 30 |
| 2160p | OMT_LQ — Low (80) | 59.50 dB | 0.01 | 35 |
| 2160p | OMT_SQ — Medium (80) | 59.50 dB | 0.01 | 35 |
| 2160p | OMT_HQ — High (80) | 52.12 dB | 0.21 | 35 |

Encode does colour conversion, FDCT, and quantisation on the GPU; Golomb coding stays on the CPU. Preview is the CPU `decode_preview_bgra` plus `queue.write_texture`.

Reference, not the table above: an earlier `OMT_SQ` synthetic gradient, filled in memory (`blue = x % 256`, `green = y % 256`, `red = 128`, `alpha = 255`; not a file under `bench/images/`), is in [`bench/results/gcp-gpu-l4.csv`](bench/results/gcp-gpu-l4.csv). GPU decode there was 1.70× the CPU-plus-transfer path at 1080p and 1.67× at 2160p. The first attempt on that VM did not see the L4 (`libGLX_nvidia.so.0` failed to load, and wgpu used llvmpipe). `libnvidia-gl-580-server` and `libegl1` made the discrete GPU visible. The image table is the L4 run, not the llvmpipe pass.

`examples/gpu_bench.rs` now discards `--prime` untimed iterations of each GPU path before timing (default 20, `GPU_BENCH_PRIME` in [`bench/scripts/run_gpu.sh`](bench/scripts/run_gpu.sh)). The medians above were taken before that prime existed.

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
