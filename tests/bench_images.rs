//! The stored UYVY frames decompress and convert to opaque BGRA.

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

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

#[test]
fn bench_images_convert_to_bgra() {
    let cases = [
        ("1080p-natural.uyvy.gz", 1920, 1080, 4_147_200usize),
        ("1080p-bars.uyvy.gz", 1920, 1080, 4_147_200),
        ("2160p-natural.uyvy.gz", 3840, 2160, 16_588_800),
        ("2160p-bars.uyvy.gz", 3840, 2160, 16_588_800),
    ];
    for (name, width, height, nbytes) in cases {
        let uyvy = load_gz(name);
        assert_eq!(uyvy.len(), nbytes, "{name}");
        let bgra = vmx::uyvy_to_bgra(&uyvy, width, height).unwrap_or_else(|err| {
            panic!("{name}: {err}");
        });
        assert_eq!(bgra.len(), (width as usize) * (height as usize) * 4);
        assert!(bgra.iter().skip(3).step_by(4).all(|alpha| *alpha == 255));
    }
}
