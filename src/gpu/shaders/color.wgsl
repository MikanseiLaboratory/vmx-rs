// YUV 4:2:2 planar (u32-per-pixel) → packed BGRA8, matching `yuv_to_bgra_pixel`.

struct ColorParams {
    width: u32,
    height: u32,
    y_stride: u32,
    u_stride: u32,
    v_stride: u32,
    dst_stride: u32, // pixels (padded)
    yuv_y: i32,
    yuv_r: i32,
    yuv_gu: i32,
    yuv_gv: i32,
    yuv_b: i32,
    _pad: i32,
}

@group(0) @binding(0) var<uniform> params: ColorParams;
@group(0) @binding(1) var<storage, read> y_plane: array<u32>;
@group(0) @binding(2) var<storage, read> u_plane: array<u32>;
@group(0) @binding(3) var<storage, read> v_plane: array<u32>;
@group(0) @binding(4) var<storage, read_write> bgra: array<u32>;

fn mulhi_i16(a: i32, b: i32) -> i32 {
    return (a * b) >> 16u;
}

fn sat_add_i16(a: i32, b: i32) -> i32 {
    return clamp(a + b, -32768, 32767);
}

fn sat_sub_i16(a: i32, b: i32) -> i32 {
    return clamp(a - b, -32768, 32767);
}

fn yuv_to_bgra(yy: u32, cb: i32, cr: i32) -> u32 {
    var y_sat = i32(yy);
    if (y_sat < 16) {
        y_sat = 0;
    } else {
        y_sat = y_sat - 16;
    }
    let y0 = mulhi_i16(y_sat << 6u, params.yuv_y);
    let r = sat_add_i16(y0, mulhi_i16(cr << 6u, params.yuv_r));
    let b = sat_add_i16(y0, mulhi_i16(cb << 7u, params.yuv_b));
    var g = sat_sub_i16(y0, mulhi_i16(cb << 6u, params.yuv_gu));
    g = sat_sub_i16(g, mulhi_i16(cr << 6u, params.yuv_gv));
    let ro = clamp((sat_add_i16(r, 8)) >> 4u, 0, 255);
    let go = clamp((sat_add_i16(g, 8)) >> 4u, 0, 255);
    let bo = clamp((sat_add_i16(b, 8)) >> 4u, 0, 255);
    return u32(bo) | (u32(go) << 8u) | (u32(ro) << 16u) | 0xFF000000u;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x_pair = gid.x;
    let y = gid.y;
    let px = x_pair * 2u;
    if (y >= params.height || px + 1u >= params.width) {
        return;
    }
    let cb = i32(u_plane[y * params.u_stride + x_pair]) - 128;
    let cr = i32(v_plane[y * params.v_stride + x_pair]) - 128;
    let y0 = y_plane[y * params.y_stride + px];
    let y1 = y_plane[y * params.y_stride + px + 1u];
    bgra[y * params.dst_stride + px] = yuv_to_bgra(y0, cb, cr);
    bgra[y * params.dst_stride + px + 1u] = yuv_to_bgra(y1, cb, cr);
}
