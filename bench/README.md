# vmx-bench

Comparison harness for three VMX1 implementations. It is a separate crate so it is not part of the published `vmx` package. Numbers from the runs described in the repository root [`BENCHMARK.md`](../BENCHMARK.md) live in [`results/`](results/).

What it builds:

- this repo, as a path dependency, with the same release profile as the root `Cargo.toml` (opt-level 3, thin LTO, one codegen unit)
- [libvmx](https://github.com/openmediatransport/libvmx) `31724bb96530325a989bc157c8e07538d6f1c4db`, fetched and compiled by `build.rs` with the flags in that file
- [vmx-codec](https://github.com/Saul-Punybz/open-media-transport) `fbde53cccabb1ebfff7ba773a72510ab48a64f3a`

Needs `git`, `clang++` (the libvmx sources use `__declspec`), and `ar`. On a machine where `clang++` is pointed at a GCC install that has no C++ headers, `build.rs` tries `--gcc-install-dir`. `CXX` overrides the compiler. `VMX_LIBVMX_SRC` uses a local libvmx checkout instead of cloning.

```bash
cd bench
cargo run --release > results.csv
```

CSV goes to stdout. A short host description goes to stderr. `VMX_BENCH_META=host.txt` also writes that description to a file.

```text
--quick                 3 repeats, 0.1 s per repeat (default is 7 and 0.3 s)
--res 1080|2160
--prof LQ|SQ|HQ|OMT_LQ|OMT_SQ|OMT_HQ
--img natural|bars
--impl libvmx|libvmx-128bit|saul-vmx-codec|mikansei-vmx
--threads N             measure only this thread count
--thread-counts 1,8     thread counts to measure (default: 1 and the CPU count)
--reps N
--secs SECONDS
```

On aarch64 there is one libvmx column. The official ARM sources do not compile the 256-bit path, so `libvmx-128bit` is not a separate build there. `vmx-codec` uses its compile-time NEON code on aarch64 and SSE2 on x86-64.

10-bit P216 / PA16 is not measured. `vmx` 0.2.1 returns an error for those entry points; the harness prints that on stderr and continues with 8-bit UYVY.
