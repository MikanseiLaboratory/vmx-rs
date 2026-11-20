//! Same-condition benchmark of three VMX implementations:
//!   libvmx (official C++), vmx-codec (Saul-Punybz, fbde53c), vmx (this repo).
//! One CSV row per measurement on stdout. See README.md.

use std::io::Write;
use std::os::raw::c_int;
use std::time::Instant;

mod ffi {
    use std::os::raw::c_int;
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct VmxSize {
        pub width: c_int,
        pub height: c_int,
    }
    #[repr(C)]
    pub struct Inst {
        _p: [u8; 0],
    }
    unsafe extern "C" {
        pub fn VMX_Create(d: VmxSize, profile: c_int, cs: c_int) -> *mut Inst;
        pub fn VMX_SetQuality(i: *mut Inst, q: c_int);
        pub fn VMX_GetQuality(i: *mut Inst) -> c_int;
        pub fn VMX_EncodeUYVY(i: *mut Inst, src: *mut u8, stride: c_int, il: c_int) -> c_int;
        pub fn VMX_SaveTo(i: *mut Inst, dst: *mut u8, max: c_int) -> c_int;
        pub fn VMX_LoadFrom(i: *mut Inst, data: *mut u8, len: c_int) -> c_int;
        pub fn VMX_DecodeUYVY(i: *mut Inst, dst: *mut u8, stride: c_int) -> c_int;
        pub fn vmxref_set_avx2(i: *mut Inst, e: c_int);
        pub fn vmxref_get_avx2(i: *mut Inst) -> c_int;
        pub fn vmxref_set_threads(i: *mut Inst, n: c_int);
        pub fn vmxref_destroy(i: *mut Inst);
    }
}

struct CRef {
    i: *mut ffi::Inst,
}
impl CRef {
    fn new(w: usize, h: usize, prof: i32, threads: i32, avx2: bool) -> Self {
        // SAFETY: VMX_Create returns a heap instance or null. The size is one
        // of the two resolutions this harness encodes, both legal for libvmx.
        let i = unsafe {
            ffi::VMX_Create(
                ffi::VmxSize {
                    width: w as c_int,
                    height: h as c_int,
                },
                prof,
                0,
            )
        };
        assert!(!i.is_null(), "VMX_Create failed for {w}x{h}");
        // SAFETY: `i` is the instance just created. set_avx2 only stores the
        // flag; on the ARM build libvmx does not read it.
        unsafe {
            ffi::vmxref_set_threads(i, threads);
            if !avx2 {
                ffi::vmxref_set_avx2(i, 0);
            }
        }
        CRef { i }
    }
    fn avx2(&self) -> bool {
        // SAFETY: `i` is live until Drop.
        unsafe { ffi::vmxref_get_avx2(self.i) == 1 }
    }
    fn set_q(&mut self, q: i32) {
        unsafe { ffi::VMX_SetQuality(self.i, q) }
    }
    fn q(&self) -> i32 {
        unsafe { ffi::VMX_GetQuality(self.i) }
    }
    fn encode(&mut self, src: &[u8], stride: usize, out: &mut [u8]) -> usize {
        unsafe {
            assert_eq!(
                ffi::VMX_EncodeUYVY(self.i, src.as_ptr() as *mut u8, stride as c_int, 0),
                0
            );
            let n = ffi::VMX_SaveTo(self.i, out.as_mut_ptr(), out.len() as c_int);
            assert!(n > 0, "VMX_SaveTo failed");
            n as usize
        }
    }
    fn decode(&mut self, pkt: &[u8], dst: &mut [u8], stride: usize) {
        unsafe {
            assert_eq!(
                ffi::VMX_LoadFrom(self.i, pkt.as_ptr() as *mut u8, pkt.len() as c_int),
                0
            );
            assert_eq!(
                ffi::VMX_DecodeUYVY(self.i, dst.as_mut_ptr(), stride as c_int),
                0
            );
        }
    }
}
impl Drop for CRef {
    fn drop(&mut self) {
        unsafe { ffi::vmxref_destroy(self.i) }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// Smooth gradients, low-frequency texture, about ±6 LSB of noise, a few hard edges.
fn img_natural(w: usize, h: usize) -> Vec<u8> {
    let mut r = Rng(0x1234_5678);
    let mut out = vec![0u8; w * 2 * h];
    let noise = |r: &mut Rng| -> i32 { ((r.next() & 7) as i32 + (r.next() & 7) as i32) - 7 };
    for y in 0..h {
        let fy = y as f32 / h as f32;
        for x in 0..w / 2 {
            let fx = (2 * x) as f32 / w as f32;
            let tex = ((fx * 37.0).sin() * (fy * 23.0).cos() * 18.0) as i32;
            let shape = if ((fx - 0.3).powi(2) + (fy - 0.5).powi(2)) < 0.02 {
                50
            } else if fx > 0.7 && fx < 0.85 && fy > 0.2 && fy < 0.6 {
                -40
            } else {
                0
            };
            let base = 40.0 + 150.0 * (0.6 * fx + 0.4 * fy);
            let yv = |r: &mut Rng| (base as i32 + tex + shape + noise(r)).clamp(16, 235) as u8;
            let o = y * w * 2 + 4 * x;
            out[o] = (128 + ((fx - 0.5) * 70.0) as i32 + tex / 3 + noise(&mut r) / 2).clamp(16, 240)
                as u8;
            out[o + 1] = yv(&mut r);
            out[o + 2] = (128 + ((fy - 0.5) * 60.0) as i32 - shape / 2 + noise(&mut r) / 2)
                .clamp(16, 240) as u8;
            out[o + 3] = yv(&mut r);
        }
    }
    out
}

/// 75% colour bars (BT.709 8-bit), eight flat vertical bars, no noise.
fn img_bars(w: usize, h: usize) -> Vec<u8> {
    let bars: [(u8, u8, u8); 8] = [
        (180, 128, 128),
        (168, 44, 136),
        (145, 147, 44),
        (133, 63, 52),
        (63, 193, 204),
        (51, 109, 212),
        (28, 212, 120),
        (16, 128, 128),
    ];
    let mut out = vec![0u8; w * 2 * h];
    for y in 0..h {
        for x in 0..w / 2 {
            let b = bars[(2 * x * 8 / w).min(7)];
            let o = y * w * 2 + 4 * x;
            out[o] = b.1;
            out[o + 1] = b.0;
            out[o + 2] = b.2;
            out[o + 3] = b.0;
        }
    }
    out
}

/// `reps` timed runs after `warm` frames. Each run lasts at least `min_secs`.
fn measure(mut f: impl FnMut(), warm: usize, reps: usize, min_secs: f64) -> Vec<f64> {
    for _ in 0..warm {
        f();
    }
    let t = Instant::now();
    let mut n = 0usize;
    while t.elapsed().as_secs_f64() < min_secs / 2.0 || n < 2 {
        f();
        n += 1;
    }
    let per = t.elapsed().as_secs_f64() / n as f64;
    let frames = ((min_secs / per).ceil() as usize).max(3);
    (0..reps)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..frames {
                f();
            }
            frames as f64 / t.elapsed().as_secs_f64()
        })
        .collect()
}

fn stats(mut v: Vec<f64>) -> (f64, f64, f64) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[v.len() / 2], v[0], v[v.len() - 1])
}

#[derive(Clone, Copy, PartialEq)]
enum Imp {
    Libvmx,
    #[cfg(target_arch = "x86_64")]
    Libvmx128,
    Saul,
    Vmx,
}
impl Imp {
    fn name(self) -> &'static str {
        match self {
            Imp::Libvmx => "libvmx",
            #[cfg(target_arch = "x86_64")]
            Imp::Libvmx128 => "libvmx-128bit",
            Imp::Saul => "saul-vmx-codec",
            Imp::Vmx => "mikansei-vmx",
        }
    }
}

fn implementations() -> Vec<Imp> {
    vec![
        Imp::Libvmx,
        #[cfg(target_arch = "x86_64")]
        Imp::Libvmx128,
        Imp::Saul,
        Imp::Vmx,
    ]
}

fn arg_value(args: &[String], keys: &[&str]) -> Option<String> {
    let i = args.iter().position(|a| keys.iter().any(|k| a == k))?;
    Some(
        args.get(i + 1)
            .unwrap_or_else(|| panic!("missing value after {}", args[i]))
            .clone(),
    )
}

fn host_notes() -> String {
    let mut lines = Vec::new();
    lines.push(format!("uname: {}", cmd_out("uname", &["-a"])));
    if let Ok(cpu) = std::fs::read_to_string("/proc/cpuinfo") {
        for key in ["model name", "CPU implementer", "CPU part", "CPU revision"] {
            if let Some(line) = cpu.lines().find(|l| l.starts_with(key)) {
                lines.push(line.trim().to_string());
            }
        }
    }
    if let Some(brand) = cmd_out_opt("sysctl", &["-n", "machdep.cpu.brand_string"]) {
        lines.push(format!("machdep.cpu.brand_string: {brand}"));
    }
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    lines.push(format!("parallelism: {parallelism}"));
    lines.push(format!("rustc: {}", env!("VMX_BENCH_RUSTC")));
    lines.push(format!("target: {}", std::env::consts::ARCH));
    lines.push(format!("libvmx: {}", env!("VMX_BENCH_LIBVMX")));
    lines.push(format!("cxx: {}", env!("VMX_BENCH_CXX")));
    lines.push(format!("cxxflags: {}", env!("VMX_BENCH_CXXFLAGS")));
    lines.push("vmx-codec: fbde53cccabb1ebfff7ba773a72510ab48a64f3a".to_string());
    // CARGO_PKG_VERSION is this harness, not the path dependency.
    lines.push("vmx: in-tree path dependency (../Cargo.toml)".to_string());
    lines.join("\n")
}

fn cmd_out(bin: &str, args: &[&str]) -> String {
    cmd_out_opt(bin, args).unwrap_or_else(|| format!("{bin} unavailable"))
}

fn cmd_out_opt(bin: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.trim().to_string())
}

fn note_10bit() -> String {
    let mut c = vmx::Codec::new(vmx::Config {
        width: 32,
        height: 16,
        profile: vmx::Profile::Hq,
        color_space: vmx::ColorSpace::Undefined,
    })
    .expect("codec");
    let y = vec![0u8; 32 * 2 * 16];
    let uv = vec![0u8; 32 * 2 * 16];
    let a = vec![0u8; 32 * 2 * 16];
    let mut dy = y.clone();
    let mut duv = uv.clone();
    let mut da = a.clone();
    let results = [
        (
            "encode_p216",
            c.encode_p216(&y, 64, &uv, 64).err().map(|e| e.to_string()),
        ),
        (
            "decode_p216",
            c.decode_p216(&mut dy, 64, &mut duv, 64)
                .err()
                .map(|e| e.to_string()),
        ),
        (
            "encode_pa16",
            c.encode_pa16(&y, 64, &uv, 64, &a, 64)
                .err()
                .map(|e| e.to_string()),
        ),
        (
            "decode_pa16",
            c.decode_pa16(&mut dy, 64, &mut duv, 64, &mut da, 64)
                .err()
                .map(|e| e.to_string()),
        ),
    ];
    let mut msg = String::from("10-bit not measured.");
    for (name, err) in results {
        match err {
            Some(err) => msg.push_str(&format!(" {name}={err}.")),
            None => msg.push_str(&format!(" {name}=ok (unexpected).")),
        }
    }
    msg
}

fn saul_simd() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "SSE2 (compile-time)",
        "aarch64" => "NEON (compile-time)",
        other => panic!("no vmx-codec ISA label for {other}"),
    }
}

fn libvmx_simd(avx2: bool) -> String {
    if cfg!(target_arch = "aarch64") {
        "NEON (sse2neon)".to_string()
    } else if avx2 {
        "AVX2(+BMI)".to_string()
    } else {
        "SSE(128bit)".to_string()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let quick = args.iter().any(|a| a == "--quick");
    let fres = arg_value(&args, &["--res", "--filter-res"]);
    let fprof = arg_value(&args, &["--prof", "--filter-prof"]);
    let fimg = arg_value(&args, &["--img"]);
    let fimpl = arg_value(&args, &["--impl"]);
    let fth: Option<usize> =
        arg_value(&args, &["--threads"]).map(|s| s.parse().expect("--threads"));
    let reps: usize = arg_value(&args, &["--reps"])
        .map(|s| s.parse().expect("--reps"))
        .unwrap_or(if quick { 3 } else { 7 });
    let min_secs: f64 = arg_value(&args, &["--secs"])
        .map(|s| s.parse().expect("--secs"))
        .unwrap_or(if quick { 0.1 } else { 0.3 });
    assert!(reps >= 1, "--reps must be at least 1");
    let all_threads = std::thread::available_parallelism()
        .expect("parallelism")
        .get();
    let thread_set: Vec<usize> = match arg_value(&args, &["--thread-counts"]) {
        Some(s) => s
            .split(',')
            .map(|p| p.parse().expect("--thread-counts"))
            .collect(),
        None => vec![1, all_threads],
    };

    let notes = format!(
        "{}\nthreads: {}\n{}\n",
        host_notes(),
        thread_set
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(","),
        note_10bit()
    );
    eprintln!("{notes}");
    if let Ok(path) = std::env::var("VMX_BENCH_META") {
        std::fs::write(&path, &notes).unwrap_or_else(|err| panic!("write {path}: {err}"));
    }

    let profiles: [(&str, i32, vmx_codec::Profile, vmx::Profile); 6] = [
        ("LQ", 33, vmx_codec::Profile::Lq, vmx::Profile::Lq),
        ("SQ", 66, vmx_codec::Profile::Sq, vmx::Profile::Sq),
        ("HQ", 99, vmx_codec::Profile::Hq, vmx::Profile::Hq),
        (
            "OMT_LQ",
            133,
            vmx_codec::Profile::OmtLq,
            vmx::Profile::OmtLq,
        ),
        (
            "OMT_SQ",
            166,
            vmx_codec::Profile::OmtSq,
            vmx::Profile::OmtSq,
        ),
        (
            "OMT_HQ",
            199,
            vmx_codec::Profile::OmtHq,
            vmx::Profile::OmtHq,
        ),
    ];
    println!(
        "res,profile,image,q,impl,simd,op,threads,fps_median,fps_min,fps_max,bytes_per_frame,bitexact_vs_libvmx128"
    );
    let _stdout = std::io::stdout();
    for (w, h) in [(1920usize, 1080usize), (3840, 2160)] {
        if let Some(r) = &fres {
            if r != &h.to_string() {
                continue;
            }
        }
        let stride = w * 2;
        for (iname, img) in [("natural", img_natural(w, h)), ("bars", img_bars(w, h))] {
            if let Some(f) = &fimg {
                if f != iname {
                    continue;
                }
            }
            for &(pname, pid, sprof, mprof) in &profiles {
                if let Some(f) = &fprof {
                    if f != pname {
                        continue;
                    }
                }
                let q = {
                    let mut c = CRef::new(w, h, pid, all_threads as i32, true);
                    let mut buf = vec![0u8; 64 << 20];
                    for _ in 0..60 {
                        c.encode(&img, stride, &mut buf);
                    }
                    c.q()
                };
                let mut big = vec![0u8; 64 << 20];
                let refpkt = {
                    let mut c = CRef::new(w, h, pid, 1, false);
                    c.set_q(q);
                    let n = c.encode(&img, stride, &mut big);
                    big[..n].to_vec()
                };
                let mut refdec = vec![0u8; stride * h];
                CRef::new(w, h, pid, 1, false).decode(&refpkt, &mut refdec, stride);

                for imp in implementations() {
                    for &th in &thread_set {
                        if let Some(f) = &fimpl {
                            if f != imp.name() {
                                continue;
                            }
                        }
                        if let Some(t) = fth {
                            if t != th {
                                continue;
                            }
                        }
                        let (simd, enc, pkt): (String, Vec<f64>, Vec<u8>) = match imp {
                            Imp::Libvmx => {
                                encode_libvmx(w, h, pid, th, true, q, &img, stride, reps, min_secs)
                            }
                            #[cfg(target_arch = "x86_64")]
                            Imp::Libvmx128 => {
                                encode_libvmx(w, h, pid, th, false, q, &img, stride, reps, min_secs)
                            }
                            Imp::Saul => {
                                let mut cfg = vmx_codec::EncoderConfig::new(w, h);
                                cfg.profile = sprof;
                                cfg.threads = th;
                                let mut e = vmx_codec::Encoder::new(cfg).unwrap();
                                let frame = vmx_codec::Frame::from_planes(
                                    w,
                                    h,
                                    vmx_codec::PixelFormat::Uyvy,
                                    vec![vmx_codec::Plane {
                                        data: img.clone(),
                                        stride,
                                    }],
                                )
                                .unwrap();
                                let mut out: Vec<u8> = Vec::with_capacity(64 << 20);
                                let v = measure(
                                    || {
                                        e.set_quality(q);
                                        out.clear();
                                        e.encode_into(&frame, &mut out).unwrap();
                                    },
                                    3,
                                    reps,
                                    min_secs,
                                );
                                (saul_simd().to_string(), v, out)
                            }
                            Imp::Vmx => {
                                let mut c = vmx::Codec::new(vmx::Config {
                                    width: w as i32,
                                    height: h as i32,
                                    profile: mprof,
                                    color_space: vmx::ColorSpace::Undefined,
                                })
                                .unwrap();
                                c.set_threads(th);
                                let simd = format!(
                                    "dct={:?} color={:?}",
                                    c.simd_path(),
                                    c.color_simd_path()
                                );
                                let mut out = vec![0u8; 64 << 20];
                                let mut n = 0;
                                let v = measure(
                                    || {
                                        c.set_quality(q);
                                        c.encode_uyvy(&img, stride).unwrap();
                                        n = c.save_to(&mut out).unwrap();
                                    },
                                    3,
                                    reps,
                                    min_secs,
                                );
                                (simd, v, out[..n].to_vec())
                            }
                        };
                        let exact = if pkt == refpkt { "yes" } else { "NO" };
                        let (m, lo, hi) = stats(enc);
                        println!(
                            "{h}p,{pname},{iname},{q},{},{simd},encode,{th},{m:.1},{lo:.1},{hi:.1},{},{exact}",
                            imp.name(),
                            pkt.len()
                        );
                        let _ = _stdout.lock().flush();

                        let mut dst = vec![0u8; stride * h];
                        let dec = match imp {
                            Imp::Libvmx => {
                                let mut c = CRef::new(w, h, pid, th as i32, true);
                                measure(|| c.decode(&refpkt, &mut dst, stride), 3, reps, min_secs)
                            }
                            #[cfg(target_arch = "x86_64")]
                            Imp::Libvmx128 => {
                                let mut c = CRef::new(w, h, pid, th as i32, false);
                                measure(|| c.decode(&refpkt, &mut dst, stride), 3, reps, min_secs)
                            }
                            Imp::Saul => {
                                let mut d = vmx_codec::Decoder::new(w, h).unwrap();
                                d.set_threads(th);
                                let mut f =
                                    vmx_codec::Frame::new(w, h, vmx_codec::PixelFormat::Uyvy);
                                let v = measure(
                                    || d.decode_into(&refpkt, &mut f).unwrap(),
                                    3,
                                    reps,
                                    min_secs,
                                );
                                dst.copy_from_slice(&f.planes[0].data);
                                v
                            }
                            Imp::Vmx => {
                                let mut c = vmx::Codec::new(vmx::Config {
                                    width: w as i32,
                                    height: h as i32,
                                    profile: mprof,
                                    color_space: vmx::ColorSpace::Undefined,
                                })
                                .unwrap();
                                c.set_threads(th);
                                measure(
                                    || {
                                        c.load_from(&refpkt).unwrap();
                                        c.decode_uyvy(&mut dst, stride).unwrap();
                                    },
                                    3,
                                    reps,
                                    min_secs,
                                )
                            }
                        };
                        let exact = if dst == refdec { "yes" } else { "NO" };
                        let (m, lo, hi) = stats(dec);
                        println!(
                            "{h}p,{pname},{iname},{q},{},{simd},decode,{th},{m:.1},{lo:.1},{hi:.1},{},{exact}",
                            imp.name(),
                            refpkt.len()
                        );
                        let _ = _stdout.lock().flush();
                    }
                }
            }
        }
    }
}

fn encode_libvmx(
    w: usize,
    h: usize,
    pid: i32,
    th: usize,
    avx2: bool,
    q: i32,
    img: &[u8],
    stride: usize,
    reps: usize,
    min_secs: f64,
) -> (String, Vec<f64>, Vec<u8>) {
    let mut c = CRef::new(w, h, pid, th as i32, avx2);
    let simd = libvmx_simd(c.avx2());
    let mut out = vec![0u8; 64 << 20];
    let mut n = 0;
    let v = measure(
        || {
            c.set_q(q);
            n = c.encode(img, stride, &mut out);
        },
        3,
        reps,
        min_secs,
    );
    (simd, v, out[..n].to_vec())
}
