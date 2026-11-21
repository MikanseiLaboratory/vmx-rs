// Sample BGRA/RGBA texture → integer FDCT + quant + zigzag.
// YUV planes stay on chip; only dense i16 coefficients are written out.

struct EncodeParams {
    width: u32,
    height: u32,
    src_stride: u32,
    y_stride: u32,
    u_stride: u32,
    v_stride: u32,
    y_blocks_x: u32,
    y_blocks_y: u32,
    u_blocks_x: u32,
    u_blocks_y: u32,
    add_y: i32,
    add_uv: i32,
    u_plane_off: u32,
    v_plane_off: u32,
    u_coeff_off: u32,
    v_coeff_off: u32,
    u_r: i32,
    u_g: i32,
    u_b: i32,
    v_r: i32,
    v_g: i32,
    v_b: i32,
    y_r: i32,
    y_g: i32,
    y_b: i32,
    a_plane_off: u32,
    a_coeff_off: u32,
    include_alpha: u32,
}

@group(0) @binding(0) var<uniform> params: EncodeParams;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> enc_matrix_data: array<vec4<i32>, 48>;
@group(0) @binding(3) var<storage, read_write> coeffs: array<vec4<u32>>;

const FTAB_ALL: array<i32, 128> = array<i32, 128>(
    16384, 16384, 22725, 19266, -8867, -21407, -22725, -12873,
    16384, 16384, 12873, 4520, 21407, 8867, 19266, -4520,
    16384, -16384, 12873, -22725, 21407, -8867, 19266, -22725,
    -16384, 16384, 4520, 19266, 8867, -21407, 4520, -12873,
    22725, 22725, 31521, 26722, -12299, -29692, -31521, -17855,
    22725, 22725, 17855, 6270, 29692, 12299, 26722, -6270,
    22725, -22725, 17855, -31521, 29692, -12299, 26722, -31521,
    -22725, 22725, 6270, 26722, 12299, -29692, 6270, -17855,
    21407, 21407, 29692, 25172, -11585, -27969, -29692, -16819,
    21407, 21407, 16819, 5906, 27969, 11585, 25172, -5906,
    21407, -21407, 16819, -29692, 27969, -11585, 25172, -29692,
    -21407, 21407, 5906, 25172, 11585, -27969, 5906, -16819,
    19266, 19266, 26722, 22654, -10426, -25172, -26722, -15137,
    19266, 19266, 15137, 5315, 25172, 10426, 22654, -5315,
    19266, -19266, 15137, -26722, 25172, -10426, 22654, -26722,
    -19266, 19266, 5315, 22654, 10426, -25172, 5315, -15137,
);
const ZIGZAG_INV: array<u32, 64> = array<u32, 64>(
    0, 1, 5, 6, 14, 15, 27, 28,
    2, 4, 7, 13, 16, 26, 29, 42,
    3, 8, 12, 17, 25, 30, 41, 43,
    9, 11, 18, 24, 31, 40, 44, 53,
    10, 19, 23, 32, 39, 45, 52, 54,
    20, 22, 33, 38, 46, 51, 55, 60,
    21, 34, 37, 47, 50, 56, 59, 61,
    35, 36, 48, 49, 57, 58, 62, 63,
);

const SHIFT_FRW_COL: i32 = 3;
const SHIFT_FRW_ROW: i32 = 16;
const RND_FRW_ROW: i32 = 32768;
const FDCT_ROUND1: i32 = 1;
const FDCT_TAN1: i32 = 13036;
const FDCT_TAN2: i32 = 27146;
const FDCT_TAN3: i32 = -21746;
const FDCT_SQRT2: i32 = 23170;

fn sat_i16(v: i32) -> i32 { return clamp(v, -32768, 32767); }
fn to_i16(v: i32) -> i32 { return (v << 16u) >> 16u; }
fn sat_add_i16(a: i32, b: i32) -> i32 { return sat_i16(a + b); }
fn sat_sub_i16(a: i32, b: i32) -> i32 { return sat_i16(a - b); }
fn mulhi_i16(a: i32, b: i32) -> i32 { return (a * b) >> 16u; }
fn mulhi_u16(a: u32, b: u32) -> u32 { return (a * b) >> 16u; }
fn ftab(i: u32) -> i32 { return FTAB_ALL[i]; }
fn zigzag_inv(i: u32) -> u32 { return ZIGZAG_INV[i]; }
fn enc_matrix(i: u32) -> u32 { return u32(enc_matrix_data[i / 4u][i % 4u]); }

fn unorm_u8(v: f32) -> i32 {
    return i32(clamp(v, 0.0, 1.0) * 255.0 + 0.5);
}

fn load_rgba(px: u32, py: u32) -> vec4<i32> {
    let x = i32(min(px, params.width - 1u));
    let y = i32(min(py, params.height - 1u));
    let c = textureLoad(src_tex, vec2<i32>(x, y), 0);
    return vec4<i32>(unorm_u8(c.r), unorm_u8(c.g), unorm_u8(c.b), unorm_u8(c.a));
}

fn rgb_to_y(r: i32, g: i32, b: i32) -> i32 {
    let y = (params.y_r * r + params.y_g * g + params.y_b * b + 128) >> 8u;
    return clamp(y + 16, 0, 255);
}

fn rgb_to_chroma(r: i32, g: i32, b: i32, cr: i32, cg: i32, cb: i32) -> i32 {
    return ((cr * r + cg * g + cb * b + 128) >> 8u) + 128;
}

fn sample_y(px: u32, py: u32) -> i32 {
    let p = load_rgba(px, py);
    return sat_add_i16(rgb_to_y(p.x, p.y, p.z), params.add_y);
}

fn sample_a(px: u32, py: u32) -> i32 {
    let p = load_rgba(px, py);
    return sat_add_i16(p.w, params.add_y);
}

fn sample_u(cx: u32, cy: u32) -> i32 {
    let p0 = load_rgba(cx * 2u, cy);
    let p1 = load_rgba(cx * 2u + 1u, cy);
    let u0 = rgb_to_chroma(p0.x, p0.y, p0.z, params.u_r, params.u_g, params.u_b);
    let u1 = rgb_to_chroma(p1.x, p1.y, p1.z, params.u_r, params.u_g, params.u_b);
    return sat_add_i16(clamp((u0 + u1) >> 1u, 0, 255), params.add_uv);
}

fn sample_v(cx: u32, cy: u32) -> i32 {
    let p0 = load_rgba(cx * 2u, cy);
    let p1 = load_rgba(cx * 2u + 1u, cy);
    let v0 = rgb_to_chroma(p0.x, p0.y, p0.z, params.v_r, params.v_g, params.v_b);
    let v1 = rgb_to_chroma(p1.x, p1.y, p1.z, params.v_r, params.v_g, params.v_b);
    return sat_add_i16(clamp((v0 + v1) >> 1u, 0, 255), params.add_uv);
}

fn fdct_row(input: array<i32, 8>, ftab_base: u32) -> array<i32, 8> {
    let s0 = sat_add_i16(input[0], input[7]);
    let d0 = sat_sub_i16(input[0], input[7]);
    let s1 = sat_add_i16(input[1], input[6]);
    let d1 = sat_sub_i16(input[1], input[6]);
    let s2 = sat_add_i16(input[2], input[5]);
    let d2 = sat_sub_i16(input[2], input[5]);
    let s3 = sat_add_i16(input[3], input[4]);
    let d3 = sat_sub_i16(input[3], input[4]);
    let full = array<i32, 8>(s0, s1, d0, d1, s2, s3, d2, d3);
    let shuf = array<i32, 8>(s2, s3, d2, d3, s0, s1, d0, d1);
    var temp4: array<i32, 4>;
    var temp1: array<i32, 4>;
    var temp2: array<i32, 4>;
    var temp3: array<i32, 4>;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        temp4[i] = full[2u * i] * ftab(ftab_base + 2u * i) + full[2u * i + 1u] * ftab(ftab_base + 2u * i + 1u);
        temp1[i] = shuf[2u * i] * ftab(ftab_base + 8u + 2u * i) + shuf[2u * i + 1u] * ftab(ftab_base + 8u + 2u * i + 1u);
        temp2[i] = full[2u * i] * ftab(ftab_base + 16u + 2u * i) + full[2u * i + 1u] * ftab(ftab_base + 16u + 2u * i + 1u);
        temp3[i] = shuf[2u * i] * ftab(ftab_base + 24u + 2u * i) + shuf[2u * i + 1u] * ftab(ftab_base + 24u + 2u * i + 1u);
    }
    var lo: array<i32, 4>;
    var hi: array<i32, 4>;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        lo[i] = (temp4[i] + temp1[i] + RND_FRW_ROW) >> u32(SHIFT_FRW_ROW);
        hi[i] = (temp3[i] + temp2[i] + RND_FRW_ROW) >> u32(SHIFT_FRW_ROW);
    }
    return array<i32, 8>(
        sat_i16(lo[0]), sat_i16(lo[1]), sat_i16(lo[2]), sat_i16(lo[3]),
        sat_i16(hi[0]), sat_i16(hi[1]), sat_i16(hi[2]), sat_i16(hi[3]),
    );
}

fn fdct_column_one(c: array<i32, 8>) -> array<i32, 8> {
    var xmm0 = c[0];
    var xmm2 = c[2];
    var xmm7 = c[7];
    var xmm5 = c[5];
    let xmm3s = xmm0;
    let xmm4s = xmm2;
    xmm0 = sat_sub_i16(xmm0, xmm7);
    xmm7 = sat_add_i16(xmm7, xmm3s);
    xmm2 = sat_sub_i16(xmm2, xmm5);
    xmm5 = sat_add_i16(xmm5, xmm4s);

    var xmm3 = c[3];
    var xmm4 = c[4];
    let xmm1s = xmm3;
    xmm3 = sat_sub_i16(xmm3, xmm4);
    xmm4 = sat_add_i16(xmm4, xmm1s);

    var xmm6 = c[6];
    var xmm1 = c[1];
    let tmp = xmm1;
    xmm1 = sat_sub_i16(xmm1, xmm6);
    xmm6 = sat_add_i16(xmm6, tmp);

    var tm03 = sat_sub_i16(xmm7, xmm4);
    var tm12 = sat_sub_i16(xmm6, xmm5);
    xmm4 = sat_add_i16(xmm4, xmm4);
    xmm5 = sat_add_i16(xmm5, xmm5);
    var tp03 = sat_add_i16(xmm4, tm03);
    var tp12 = sat_add_i16(xmm5, tm12);

    xmm2 = to_i16(xmm2 << u32(SHIFT_FRW_COL + 1));
    xmm1 = to_i16(xmm1 << u32(SHIFT_FRW_COL + 1));
    tp03 = to_i16(tp03 << u32(SHIFT_FRW_COL));
    tp12 = to_i16(tp12 << u32(SHIFT_FRW_COL));
    tm03 = to_i16(tm03 << u32(SHIFT_FRW_COL));
    tm12 = to_i16(tm12 << u32(SHIFT_FRW_COL));
    xmm3 = to_i16(xmm3 << u32(SHIFT_FRW_COL));
    xmm0 = to_i16(xmm0 << u32(SHIFT_FRW_COL));

    let in4 = sat_sub_i16(tp03, tp12);
    let diff = sat_sub_i16(xmm1, xmm2);
    tp12 = sat_add_i16(tp12, tp12);
    let xmm2b = sat_add_i16(xmm2, xmm2);
    let in0 = sat_add_i16(tp12, in4);
    let sum = sat_add_i16(xmm2b, diff);

    let tmp1 = mulhi_i16(FDCT_TAN2, tm03);
    var in6 = sat_sub_i16(tmp1, tm12);
    let tmp2 = mulhi_i16(FDCT_TAN2, tm12);
    var in2 = sat_add_i16(tmp2, tm03);

    var tp65 = mulhi_i16(sum, FDCT_SQRT2);
    in2 = in2 | FDCT_ROUND1;
    in6 = in6 | FDCT_ROUND1;
    let tm65 = mulhi_i16(diff, FDCT_SQRT2);
    tp65 = tp65 | FDCT_ROUND1;

    let tm465 = sat_sub_i16(xmm3, tm65);
    let tm765 = sat_sub_i16(xmm0, tp65);
    let tp765 = sat_add_i16(tp65, xmm0);
    let tp465 = sat_add_i16(tm65, xmm3);

    var tmp3 = mulhi_i16(tm465, FDCT_TAN3);
    let tmp4 = mulhi_i16(tp465, FDCT_TAN1);
    tmp3 = sat_add_i16(tmp3, tm465);
    var tmp5 = mulhi_i16(tm765, FDCT_TAN3);
    tmp5 = sat_add_i16(tmp5, tm765);
    let tmp6 = mulhi_i16(tp765, FDCT_TAN1);

    let in1 = sat_add_i16(tmp4, tp765);
    let in3 = sat_sub_i16(tm765, tmp3);
    let in5 = sat_add_i16(tm465, tmp5);
    let in7 = sat_sub_i16(tmp6, tp465);
    return array<i32, 8>(in0, in1, in2, in3, in4, in5, in6, in7);
}

fn spatial_quant(v: i32, i: u32) -> i32 {
    if (v == 0) { return 0; }
    var abs_v: u32;
    if (v < 0) { abs_v = u32(-v); } else { abs_v = u32(v); }
    let c = enc_matrix(i);
    let recip = enc_matrix(i + 64u);
    let scale = enc_matrix(i + 128u);
    var q = abs_v + c;
    q = mulhi_u16(q, recip);
    q = mulhi_u16(q, scale);
    if (v < 0) { return -i32(q); }
    return i32(q);
}

var<workgroup> sPlane: array<i32, 384>;
var<workgroup> sTmp: array<i32, 384>;
var<workgroup> sZig: array<i32, 384>;

@compute @workgroup_size(64)
fn encode_tile(
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let tiles_x = (params.y_blocks_x + 1u) / 2u;
    let cx = wid.x % tiles_x;
    let cy = wid.x / tiles_x;

    // Load phase: 64 threads each load one horizontal pair (2 pixels).
    let pair = lid;
    let row = pair / 8u;
    let pair_x = pair % 8u;
    let px = pair_x * 2u;
    let x0 = cx * 16u + px;
    let y = cy * 8u + row;
    let p0 = load_rgba(x0, y);
    let p1 = load_rgba(x0 + 1u, y);
    let y0 = sat_add_i16(rgb_to_y(p0.x, p0.y, p0.z), params.add_y);
    let y1 = sat_add_i16(rgb_to_y(p1.x, p1.y, p1.z), params.add_y);
    let bx = px / 8u;
    let bxx = px % 8u;
    sPlane[bx * 64u + row * 8u + bxx] = y0;
    sPlane[bx * 64u + row * 8u + bxx + 1u] = y1;
    let u0 = rgb_to_chroma(p0.x, p0.y, p0.z, params.u_r, params.u_g, params.u_b);
    let u1 = rgb_to_chroma(p1.x, p1.y, p1.z, params.u_r, params.u_g, params.u_b);
    let v0 = rgb_to_chroma(p0.x, p0.y, p0.z, params.v_r, params.v_g, params.v_b);
    let v1 = rgb_to_chroma(p1.x, p1.y, p1.z, params.v_r, params.v_g, params.v_b);
    sPlane[128u + row * 8u + pair_x] = sat_add_i16(clamp((u0 + u1) >> 1u, 0, 255), params.add_uv);
    sPlane[192u + row * 8u + pair_x] = sat_add_i16(clamp((v0 + v1) >> 1u, 0, 255), params.add_uv);
    if (params.include_alpha != 0u) {
        sPlane[256u + bx * 64u + row * 8u + bxx] = sat_add_i16(p0.w, params.add_y);
        sPlane[256u + bx * 64u + row * 8u + bxx + 1u] = sat_add_i16(p1.w, params.add_y);
    }
    workgroupBarrier();

    // FDCT phase: 8 threads per block, 6 blocks (Y0,Y1,U,V,A0,A1).
    let b = lid / 8u;
    let t = lid % 8u;
    let block_y = cy * params.y_blocks_x + cx * 2u;
    let block_u = cy * params.u_blocks_x + cx;
    var valid = false;
    var out_base = 0u;
    if (b == 0u) {
        valid = (cx * 2u) < params.y_blocks_x;
        out_base = block_y * 64u;
    } else if (b == 1u) {
        valid = (cx * 2u + 1u) < params.y_blocks_x;
        out_base = (block_y + 1u) * 64u;
    } else if (b == 2u) {
        valid = cx < params.u_blocks_x;
        out_base = params.u_coeff_off + block_u * 64u;
    } else if (b == 3u) {
        valid = cx < params.u_blocks_x;
        out_base = params.v_coeff_off + block_u * 64u;
    } else if (b == 4u) {
        valid = (cx * 2u) < params.y_blocks_x && params.include_alpha != 0u;
        out_base = params.a_coeff_off + block_y * 64u;
    } else if (b == 5u) {
        valid = (cx * 2u + 1u) < params.y_blocks_x && params.include_alpha != 0u;
        out_base = params.a_coeff_off + (block_y + 1u) * 64u;
    }

    let src = b * 64u;
    if (valid) {
        let c = fdct_column_one(array<i32, 8>(
            sPlane[src + 0u * 8u + t], sPlane[src + 1u * 8u + t],
            sPlane[src + 2u * 8u + t], sPlane[src + 3u * 8u + t],
            sPlane[src + 4u * 8u + t], sPlane[src + 5u * 8u + t],
            sPlane[src + 6u * 8u + t], sPlane[src + 7u * 8u + t],
        ));
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            sTmp[src + k * 8u + t] = c[k];
        }
    }
    workgroupBarrier();

    let ftab_order = array<u32, 8>(0u, 1u, 2u, 3u, 0u, 3u, 2u, 1u);
    if (valid) {
        let row_out = fdct_row(array<i32, 8>(
            sTmp[src + t * 8u], sTmp[src + t * 8u + 1u],
            sTmp[src + t * 8u + 2u], sTmp[src + t * 8u + 3u],
            sTmp[src + t * 8u + 4u], sTmp[src + t * 8u + 5u],
            sTmp[src + t * 8u + 6u], sTmp[src + t * 8u + 7u],
        ), ftab_order[t] * 32u);
        for (var x: u32 = 0u; x < 8u; x = x + 1u) {
            sZig[src + zigzag_inv(t * 8u + x)] = spatial_quant(row_out[x], t * 8u + x);
        }
    }
    workgroupBarrier();

    if (valid) {
        let word_base = out_base >> 1u;
        let w = t * 4u;
        let a0 = u32(sZig[src + w * 2u]) & 0xFFFFu;
        let b0 = u32(sZig[src + w * 2u + 1u]) & 0xFFFFu;
        let a1 = u32(sZig[src + (w + 1u) * 2u]) & 0xFFFFu;
        let b1 = u32(sZig[src + (w + 1u) * 2u + 1u]) & 0xFFFFu;
        let a2 = u32(sZig[src + (w + 2u) * 2u]) & 0xFFFFu;
        let b2 = u32(sZig[src + (w + 2u) * 2u + 1u]) & 0xFFFFu;
        let a3 = u32(sZig[src + (w + 3u) * 2u]) & 0xFFFFu;
        let b3 = u32(sZig[src + (w + 3u) * 2u + 1u]) & 0xFFFFu;
        coeffs[(word_base / 4u) + t] = vec4<u32>(
            a0 | (b0 << 16u),
            a1 | (b1 << 16u),
            a2 | (b2 << 16u),
            a3 | (b3 << 16u),
        );
    }
}
