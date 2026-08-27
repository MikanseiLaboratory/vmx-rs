@group(0) @binding(4) var<storage, read_write> bgra: array<u32>;

@compute @workgroup_size(32)
fn main_buf(
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let tile = wid.x;
    run_idct(lid, tile);
    if (lid < 16u) {
        let t = tile_px(lid, tile);
        if (t.w != 0u) {
            let px = t.x;
            let py0 = t.y;
            let lx = t.z;
            let ybase = select(0u, 64u, lx >= 8u);
            let x8 = lx % 8u;
            let ux = lx / 2u;
            for (var k: u32 = 0u; k < 8u; k = k + 1u) {
                let py = py0 + k;
                if (py >= params.height) {
                    break;
                }
                let yy = u32(clamp(sm[ybase + k * 8u + x8], 0.0, 255.0));
                let cb = i32(clamp(sm[128u + k * 8u + ux], 0.0, 255.0)) - 128;
                let cr = i32(clamp(sm[192u + k * 8u + ux], 0.0, 255.0)) - 128;
                bgra[py * params.dst_stride + px] = yuv_to_packed(yy, cb, cr);
            }
        }
    }
}
