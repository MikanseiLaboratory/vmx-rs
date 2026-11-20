//! Fetch libvmx at the commit the x86 tables were measured against and
//! compile it the way that run did.
//!
//! x86-64 matches `build/buildlinuxx64.sh`:
//!   clang++ -O3 -std=c++17 -fdeclspec -mlzcnt -mavx2 -mbmi
//! aarch64 matches `build/buildlinuxarm64.sh` (no AVX2 flags; `vmxcodec_arm.cpp`).
//!
//! `CXX` overrides the compiler. `VMX_LIBVMX_SRC` skips the clone and uses
//! that checkout instead.

use std::path::{Path, PathBuf};
use std::process::Command;

const LIBVMX_SHA: &str = "31724bb96530325a989bc157c8e07538d6f1c4db";
const LIBVMX_URL: &str = "https://github.com/openmediatransport/libvmx.git";

fn main() {
    let target = std::env::var("TARGET").expect("TARGET");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=native/vmxref.cpp");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=VMX_LIBVMX_SRC");

    let src = match std::env::var("VMX_LIBVMX_SRC") {
        Ok(path) => PathBuf::from(path),
        Err(_) => fetch_libvmx(&out_dir),
    };
    let header = src.join("src").join("vmxcodec.h");
    if !header.is_file() {
        panic!("libvmx checkout at {} has no src/vmxcodec.h", src.display());
    }

    let arch = if target.starts_with("x86_64") {
        "x86_64"
    } else if target.starts_with("aarch64") {
        "aarch64"
    } else {
        panic!("vmx-bench links libvmx only on x86_64 and aarch64 (target {target})");
    };

    let (cxx, extra) = find_compiler();
    if let Some(dir) = stdlib_dir(&cxx, &extra) {
        println!("cargo:rustc-link-search=native={dir}");
    }
    let mut flags = vec![
        "-O3".to_string(),
        "-std=c++17".to_string(),
        "-fdeclspec".to_string(),
        "-fPIC".to_string(),
    ];
    flags.extend(extra);
    if arch == "x86_64" {
        flags.extend(
            ["-mlzcnt", "-mavx2", "-mbmi"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        flags.push("-pthread".to_string());
    }

    let include = src.join("src");
    let sources: Vec<PathBuf> = if arch == "x86_64" {
        ["vmxcodec.cpp", "vmxcodec_x86.cpp", "vmxcodec_avx2.cpp"]
            .into_iter()
            .map(|name| include.join(name))
            .collect()
    } else {
        ["vmxcodec.cpp", "vmxcodec_arm.cpp"]
            .into_iter()
            .map(|name| include.join(name))
            .collect()
    };

    let mut objects = Vec::new();
    for source in &sources {
        let obj = out_dir.join(format!(
            "{}.o",
            source.file_stem().unwrap().to_string_lossy()
        ));
        compile(&cxx, &flags, &include, source, &obj);
        objects.push(obj);
    }
    let lib = out_dir.join("libvmx.a");
    archive(&lib, &objects);

    let wrap_src = PathBuf::from("native").join("vmxref.cpp");
    let wrap_obj = out_dir.join("vmxref.o");
    compile(&cxx, &flags, &include, &wrap_src, &wrap_obj);
    let wrap_lib = out_dir.join("libvmxref.a");
    archive(&wrap_lib, &[wrap_obj]);

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=vmxref");
    println!("cargo:rustc-link-lib=static=vmx");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") | Ok("ios") => {
            println!("cargo:rustc-link-lib=c++");
        }
        _ => {
            println!("cargo:rustc-link-lib=stdc++");
            println!("cargo:rustc-link-lib=pthread");
        }
    }

    println!("cargo:rustc-env=VMX_BENCH_LIBVMX={LIBVMX_SHA}");
    println!("cargo:rustc-env=VMX_BENCH_CXX={cxx}");
    println!("cargo:rustc-env=VMX_BENCH_CXXFLAGS={}", flags.join(" "));
    println!("cargo:rustc-env=VMX_BENCH_RUSTC={}", rustc_version());
}

fn rustc_version() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let output = Command::new(&rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|err| panic!("failed to run {rustc}: {err}"));
    if !output.status.success() {
        panic!("{rustc} -vV failed");
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("unknown")
        .to_string()
}

fn fetch_libvmx(out_dir: &Path) -> PathBuf {
    let dest = out_dir.join("libvmx");
    let stamp = dest.join(".vmx-bench-sha");
    if stamp.is_file() && std::fs::read_to_string(&stamp).unwrap_or_default().trim() == LIBVMX_SHA {
        return dest;
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest).unwrap_or_else(|err| {
            panic!("remove {}: {err}", dest.display());
        });
    }
    std::fs::create_dir_all(&dest).unwrap();
    git(&["init"], &dest);
    git(&["remote", "add", "origin", LIBVMX_URL], &dest);
    git(&["fetch", "--depth", "1", "origin", LIBVMX_SHA], &dest);
    git(&["checkout", "--detach", "FETCH_HEAD"], &dest);
    std::fs::write(&stamp, LIBVMX_SHA).unwrap();
    dest
}

fn git(args: &[&str], dir: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|err| panic!("failed to run git {}: {err}", args.join(" ")));
    if !status.success() {
        panic!("git {} failed in {}", args.join(" "), dir.display());
    }
}

fn find_compiler() -> (String, Vec<String>) {
    if let Ok(cxx) = std::env::var("CXX") {
        if let Some(extra) = probe(&cxx, &[]) {
            return (cxx, extra);
        }
        panic!("CXX={cxx} cannot compile a C++17 program that uses __declspec(align)");
    }
    let mut candidates = vec!["clang++".to_string()];
    candidates.extend(
        gcc_install_dirs()
            .into_iter()
            .map(|dir| format!("clang++|{dir}")),
    );
    for candidate in candidates {
        if let Some((bin, dir)) = candidate.split_once('|') {
            let extra = vec![format!("--gcc-install-dir={dir}")];
            if probe(bin, &extra).is_some() {
                return (bin.to_string(), extra);
            }
        } else if probe(&candidate, &[]).is_some() {
            return (candidate, Vec::new());
        }
    }
    panic!(
        "need clang++ with a working C++ standard library (libvmx uses __declspec, which is why the official build uses clang++ -fdeclspec). Set CXX if it is not on PATH."
    );
}

fn probe(cxx: &str, extra: &[String]) -> Option<Vec<String>> {
    let dir = std::env::temp_dir().join(format!("vmx-bench-probe-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let src = dir.join("probe.cpp");
    let bin = dir.join("probe");
    std::fs::write(
        &src,
        "__declspec(align(16)) static int aligned_value = 1;\nint main(){return aligned_value;}\n",
    )
    .ok()?;
    let mut cmd = Command::new(cxx);
    cmd.args(["-std=c++17", "-fdeclspec"])
        .args(extra)
        .arg(&src)
        .arg("-o")
        .arg(&bin);
    let ok = cmd.status().ok().map(|s| s.success()).unwrap_or(false);
    let _ = std::fs::remove_dir_all(&dir);
    if ok { Some(extra.to_vec()) } else { None }
}

/// Directory that holds `libstdc++` or `libc++`, for lld (it does not always
/// search the GCC install clang just used).
fn stdlib_dir(cxx: &str, extra: &[String]) -> Option<String> {
    for name in [
        "libstdc++.so",
        "libstdc++.dylib",
        "libc++.dylib",
        "libc++.so",
    ] {
        let output = Command::new(cxx)
            .args(extra)
            .arg(format!("-print-file-name={name}"))
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8(output.stdout).ok()?;
        let path = PathBuf::from(text.trim());
        if path.is_file() {
            return path.parent().map(|dir| dir.display().to_string());
        }
    }
    None
}

fn gcc_install_dirs() -> Vec<String> {
    let mut found = Vec::new();
    let root = Path::new("/usr/lib/gcc");
    let Ok(targets) = std::fs::read_dir(root) else {
        return found;
    };
    for target in targets.flatten() {
        let Ok(versions) = std::fs::read_dir(target.path()) else {
            continue;
        };
        for version in versions.flatten() {
            if version.path().is_dir() {
                found.push(version.path().display().to_string());
            }
        }
    }
    found.sort();
    found.reverse();
    found
}

fn compile(cxx: &str, flags: &[String], include: &Path, source: &Path, object: &Path) {
    let status = Command::new(cxx)
        .args(flags)
        .arg("-I")
        .arg(include)
        .arg("-c")
        .arg(source)
        .arg("-o")
        .arg(object)
        .status()
        .unwrap_or_else(|err| panic!("failed to run {cxx}: {err}"));
    if !status.success() {
        panic!("compile failed: {cxx} {}", source.display());
    }
}

fn archive(lib: &Path, objects: &[PathBuf]) {
    let status = Command::new("ar")
        .arg("crs")
        .arg(lib)
        .args(objects)
        .status()
        .unwrap_or_else(|err| panic!("failed to run ar: {err}"));
    if !status.success() {
        panic!("ar failed for {}", lib.display());
    }
}
