use vmx::{Codec, Config, Profile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = 128i32;
    let height = 128i32;
    let stride = (width as usize) * 2;
    let mut frame = vec![128u8; stride * height as usize];
    for y in 0..height as usize {
        for x in (0..width as usize).step_by(2) {
            let o = y * stride + x * 2;
            frame[o] = 128;
            frame[o + 1] = ((x + y) % 200 + 16) as u8;
            frame[o + 2] = 128;
            frame[o + 3] = ((x + y + 1) % 200 + 16) as u8;
        }
    }

    let mut enc = Codec::new(Config {
        width,
        height,
        profile: Profile::Hq,
        color_space: Default::default(),
    })?;
    enc.encode_uyvy(&frame, stride)?;
    let mut bitstream = vec![0u8; 4 << 20];
    let len = enc.save_to(&mut bitstream)?;
    println!("encoded {len} bytes");

    let mut dec = Codec::new(Config::new(width, height))?;
    dec.load_from(&bitstream[..len])?;
    let mut out = vec![0u8; stride * height as usize];
    dec.decode_uyvy(&mut out, stride)?;
    let psnr = dec.calculate_psnr(&frame, &out, stride, 2);
    println!("PSNR: {psnr:.2} dB");
    Ok(())
}
