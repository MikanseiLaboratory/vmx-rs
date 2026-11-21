@group(0) @binding(4) var<storage, read_write> bgra: array<u32>;

@compute @workgroup_size(32)
fn main_buf(
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let tile = wid.x;
    let u_bx = max(params.u_blocks_x, 1u);
    let cx = tile % u_bx;
    let cy = tile / u_bx;
    run_idct(lid, tile);
    let lx = lid % 16u;
    let row_group = lid / 16u;
    let px = cx * 16u + lx;
    let py0 = cy * 8u + row_group * 4u;
    if (px < params.width) {
        let ybase = select(0u, 72u, lx >= 8u);
        let x8 = lx % 8u;
        let ux = lx / 2u;
        for (var k: u32 = 0u; k < 4u; k = k + 1u) {
            let sy = row_group * 4u + k;
            let py = py0 + k;
            if (py >= params.height) {
                break;
            }
            let yy = clamp(sm[ybase + sy * 9u + x8], 0, 255);
            let cb = clamp(sm[144u + sy * 9u + ux], 0, 255) - 128;
            let cr = clamp(sm[216u + sy * 9u + ux], 0, 255) - 128;
            bgra[py * params.dst_stride + px] = yuv_to_packed(yy, cb, cr);
        }
    }
}
