@group(0) @binding(3) var out_tex: texture_storage_2d<bgra8unorm, write>;

@compute @workgroup_size(32)
fn main_tex(
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
            let yy = u32(clamp(sm[ybase + sy * 9u + x8], 0.0, 255.0));
            let cb = i32(clamp(sm[144u + sy * 9u + ux], 0.0, 255.0)) - 128;
            let cr = i32(clamp(sm[216u + sy * 9u + ux], 0.0, 255.0)) - 128;
            textureStore(out_tex, vec2<i32>(i32(px), i32(py)), yuv_to_rgba(yy, cb, cr));
        }
    }
}
