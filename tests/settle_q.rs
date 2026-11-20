//! The q gpu_bench pins is the one `bench/` records.
//!
//! `bench/src/main.rs` lets libvmx run 60 encode+save frames, then reads that
//! quality and pins it. These cases are the q column of
//! `bench/results/gcp-x86-c4.csv` for the same UYVY frames.

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use vmx::{Codec, ColorSpace, Config, Profile};

fn load_gz(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bench/images")
        .join(name);
    let file = File::open(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let mut dec = flate2::read::GzDecoder::new(file);
    let mut buf = Vec::new();
    dec.read_to_end(&mut buf)
        .unwrap_or_else(|err| panic!("gunzip {}: {err}", path.display()));
    buf
}

fn settle(width: i32, height: i32, profile: Profile, uyvy: &[u8]) -> i32 {
    let mut codec = Codec::new(Config {
        width,
        height,
        profile,
        color_space: ColorSpace::Undefined,
    })
    .unwrap_or_else(|err| panic!("{width}x{height} {profile:?}: {err}"));
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    codec.set_threads(threads);
    let mut packet = vec![0u8; 64 << 20];
    let stride = width as usize * 2;
    for _ in 0..60 {
        codec
            .encode_uyvy(uyvy, stride)
            .unwrap_or_else(|err| panic!("encode: {err}"));
        codec
            .save_to(&mut packet)
            .unwrap_or_else(|err| panic!("save: {err}"));
    }
    codec.quality()
}

#[test]
fn uyvy_rate_control_matches_bench_q() {
    let cases = [
        ("1080p-natural.uyvy.gz", 1920, 1080, Profile::OmtLq, 92),
        ("1080p-natural.uyvy.gz", 1920, 1080, Profile::OmtSq, 95),
        ("1080p-natural.uyvy.gz", 1920, 1080, Profile::OmtHq, 96),
        ("1080p-bars.uyvy.gz", 1920, 1080, Profile::OmtLq, 98),
        ("1080p-bars.uyvy.gz", 1920, 1080, Profile::OmtSq, 98),
        ("1080p-bars.uyvy.gz", 1920, 1080, Profile::OmtHq, 98),
        ("2160p-natural.uyvy.gz", 3840, 2160, Profile::OmtLq, 92),
        ("2160p-natural.uyvy.gz", 3840, 2160, Profile::OmtSq, 92),
        ("2160p-natural.uyvy.gz", 3840, 2160, Profile::OmtHq, 94),
        ("2160p-bars.uyvy.gz", 3840, 2160, Profile::OmtLq, 98),
        ("2160p-bars.uyvy.gz", 3840, 2160, Profile::OmtSq, 98),
        ("2160p-bars.uyvy.gz", 3840, 2160, Profile::OmtHq, 98),
    ];
    for (name, width, height, profile, expected) in cases {
        let uyvy = load_gz(name);
        let q = settle(width, height, profile, &uyvy);
        assert_eq!(
            q, expected,
            "{name} {profile:?} settled at {q}, bench table has {expected}"
        );
    }
}
