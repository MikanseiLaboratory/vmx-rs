// Exact fused integer AAN IDCT + YUV 4:2:2 → BGRA.
// One workgroup (32 threads) covers a 16×8 luma tile: 2 Y blocks + 1 U + 1 V.
// The coefficient record is 64 i16 (zigzag) + a DC-only flag (132 bytes).

struct FusedParams {
    y_blocks_x: u32,
    u_blocks_x: u32,
    width: u32,
    height: u32,
    dst_stride: u32,
    yuv_y: i32,
    yuv_r: i32,
    yuv_gu: i32,
    yuv_gv: i32,
    yuv_b: i32,
    u_word_off: u32,
    v_word_off: u32,
}

@group(0) @binding(0) var<uniform> params: FusedParams;
@group(0) @binding(1) var<uniform> matrix: array<vec4<i32>, 16>;
@group(0) @binding(2) var<storage, read> coeffs: array<u32>;

const IDCT_TG1: i32 = 13036;
const IDCT_TG2: i32 = 27146;
const IDCT_TG3: i32 = -21746;
const IDCT_COS4: i32 = -19195;

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

const TAB04: array<i32, 32> = array<i32, 32>(
    16384, 21407, 16384, 8867, 16384, -8867, 16384, -21407,
    16384, 8867, -16384, -21407, -16384, 21407, 16384, -8867,
    22725, 19266, 19266, -4520, 12873, -22725, 4520, -12873,
    12873, 4520, -22725, -12873, 4520, 19266, 19266, -22725,
);
const TAB17: array<i32, 32> = array<i32, 32>(
    22725, 29692, 22725, 12299, 22725, -12299, 22725, -29692,
    22725, 12299, -22725, -29692, -22725, 29692, 22725, -12299,
    31521, 26722, 26722, -6270, 17855, -31521, 6270, -17855,
    17855, 6270, -31521, -17855, 6270, 26722, 26722, -31521,
);
const TAB26: array<i32, 32> = array<i32, 32>(
    21407, 27969, 21407, 11585, 21407, -11585, 21407, -27969,
    21407, 11585, -21407, -27969, -21407, 27969, 21407, -11585,
    29692, 25172, 25172, -5906, 16819, -29692, 5906, -16819,
    16819, 5906, -29692, -16819, 5906, 25172, 25172, -29692,
);
const TAB35: array<i32, 32> = array<i32, 32>(
    19266, 25172, 19266, 10426, 19266, -10426, 19266, -25172,
    19266, 10426, -19266, -25172, -19266, 25172, 19266, -10426,
    26722, 22654, 22654, -5315, 15137, -26722, 5315, -15137,
    15137, 5315, -26722, -15137, 5315, 22654, 22654, -26722,
);

var<workgroup> sm: array<i32, 288>;

fn load_word(plane: u32, block: u32, wi: u32) -> u32 {
    var base: u32;
    if (plane <= 1u) {
        base = 0u;
    } else if (plane == 2u) {
        base = params.u_word_off;
    } else {
        base = params.v_word_off;
    }
    return coeffs[base + block * 33u + wi];
}

fn load_flag(plane: u32, block: u32) -> u32 {
    return load_word(plane, block, 32u);
}

fn load_coeff(plane: u32, block: u32, zig: u32) -> i32 {
    let w = load_word(plane, block, zig >> 1u);
    let s = (zig & 1u) * 16u;
    return (i32(w >> s) << 16u) >> 16u;
}

fn dec_matrix(i: u32) -> i32 {
    return matrix[i / 4u][i % 4u];
}

fn wrap_add(a: i32, b: i32) -> i32 {
    return bitcast<i32>(bitcast<u32>(a) + bitcast<u32>(b));
}

fn wrap_sub(a: i32, b: i32) -> i32 {
    return bitcast<i32>(bitcast<u32>(a) - bitcast<u32>(b));
}

fn mulhi_i16(a: i32, b: i32) -> i32 {
    return (a * b) >> 16u;
}

fn sat_i16(v: i32) -> i32 {
    return clamp(v, -32768, 32767);
}

fn sat_add_i16(a: i32, b: i32) -> i32 {
    return sat_i16(a + b);
}

fn sat_sub_i16(a: i32, b: i32) -> i32 {
    return sat_i16(a - b);
}

fn madd(a: i32, ta: i32, b: i32, tb: i32) -> i32 {
    return wrap_add(a * ta, b * tb);
}

fn idct_row(x0: i32, x1: i32, x2: i32, x3: i32, x4: i32, x5: i32, x6: i32, x7: i32, tab: array<i32, 32>) -> array<i32, 8> {
    var out: array<i32, 8>;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        let even = wrap_add(
            wrap_add(madd(x0, tab[2u * i], x2, tab[2u * i + 1u]), 1024),
            madd(x4, tab[8u + 2u * i], x6, tab[8u + 2u * i + 1u]),
        );
        let odd = wrap_add(
            madd(x5, tab[24u + 2u * i], x7, tab[24u + 2u * i + 1u]),
            madd(x1, tab[16u + 2u * i], x3, tab[16u + 2u * i + 1u]),
        );
        out[i] = sat_i16(wrap_add(even, odd) >> 11u);
        out[7u - i] = sat_i16(wrap_sub(even, odd) >> 11u);
    }
    return out;
}

fn idct_column(r0: i32, r1: i32, r2: i32, r3: i32, r4: i32, r5: i32, r6: i32, r7: i32, add_val: i32) -> array<i32, 8> {
    var x0 = sat_add_i16(mulhi_i16(r5, IDCT_TG3), r5);
    let x1 = sat_add_i16(mulhi_i16(r3, IDCT_TG3), r3);
    x0 = sat_add_i16(x0, r3);
    let x2 = sat_sub_i16(r5, x1);
    let x5 = sat_sub_i16(mulhi_i16(r1, IDCT_TG1), r7);
    let x4 = sat_add_i16(mulhi_i16(r7, IDCT_TG1), r1);

    let temp7 = sat_add_i16(sat_add_i16(x0, x4), 1);
    let t4 = sat_sub_i16(x4, x0);
    let t5 = sat_add_i16(sat_sub_i16(x5, x2), 1);
    let temp3 = sat_add_i16(x5, x2);

    let s = sat_add_i16(t4, t5);
    let d = sat_sub_i16(t4, t5);
    let m4 = sat_add_i16(s, mulhi_i16(IDCT_COS4, s)) | 1;
    let m0 = sat_add_i16(mulhi_i16(IDCT_COS4, d), d) | 1;

    let e7 = sat_add_i16(mulhi_i16(r6, IDCT_TG2), r2);
    let e3 = sat_sub_i16(mulhi_i16(r2, IDCT_TG2), r6);
    let sum04 = sat_add_i16(r4, r0);
    let dif04 = sat_sub_i16(r0, r4);

    let b0 = sat_add_i16(sat_add_i16(sum04, e7), 32);
    let b3 = sat_add_i16(sat_sub_i16(sum04, e7), 31);
    let b1 = sat_add_i16(sat_add_i16(dif04, e3), 32);
    let b2 = sat_add_i16(sat_sub_i16(dif04, e3), 31);

    var out: array<i32, 8>;
    out[0] = sat_add_i16(sat_add_i16(temp7, b0) >> 6u, add_val);
    out[1] = sat_add_i16(sat_add_i16(b1, m4) >> 6u, add_val);
    out[2] = sat_add_i16(sat_add_i16(b2, m0) >> 6u, add_val);
    out[3] = sat_add_i16(sat_add_i16(temp3, b3) >> 6u, add_val);
    out[4] = sat_add_i16(sat_sub_i16(b3, temp3) >> 6u, add_val);
    out[5] = sat_add_i16(sat_sub_i16(b2, m0) >> 6u, add_val);
    out[6] = sat_add_i16(sat_sub_i16(b1, m4) >> 6u, add_val);
    out[7] = sat_add_i16(sat_sub_i16(b0, temp7) >> 6u, add_val);
    return out;
}

fn yuv_to_rgba(yy: i32, cb: i32, cr: i32) -> vec4<f32> {
    var y_sat = yy;
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
    return vec4<f32>(f32(ro), f32(go), f32(bo), 255.0) / 255.0;
}

fn yuv_to_packed(yy: i32, cb: i32, cr: i32) -> u32 {
    var y_sat = yy;
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

fn run_idct(lid: u32, tile: u32) {
    let u_bx = max(params.u_blocks_x, 1u);
    let y_bx = params.y_blocks_x;
    let cx = tile % u_bx;
    let cy = tile / u_bx;

    let plane = lid / 8u;
    let t = lid % 8u;
    let base = plane * 72u;

    var block = 0u;
    var in_range = false;
    var add_val = 0;
    if (plane == 0u) {
        block = cy * y_bx + cx * 2u;
        in_range = (cx * 2u) < y_bx;
        add_val = 128;
    } else if (plane == 1u) {
        block = cy * y_bx + cx * 2u + 1u;
        in_range = (cx * 2u + 1u) < y_bx;
        add_val = 128;
    } else {
        block = cy * u_bx + cx;
        in_range = cx < u_bx;
        add_val = 0;
    }

    var dc_only = false;
    if (in_range) {
        dc_only = load_flag(plane, block) != 0u;
        if (dc_only) {
            // Match CPU `broadcast_dc`, including its i16 wrapping adds.
            let dc = load_coeff(plane, block, 0u);
            let dc4 = ((dc + 4) << 16u) >> 16u;
            let pix = (((dc4 >> 3u) + add_val) << 16u) >> 16u;
            for (var k: u32 = 0u; k < 8u; k = k + 1u) {
                sm[base + t * 9u + k] = pix;
            }
        } else {
            for (var k: u32 = 0u; k < 8u; k = k + 1u) {
                let i = t * 8u + k;
                let c = load_coeff(plane, block, ZIGZAG_INV[i]);
                let dq = ((c * dec_matrix(i)) << 16u) >> 16u;
                sm[base + t * 9u + k] = dq >> 4u;
            }
        }
    } else {
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            sm[base + t * 9u + k] = 0;
        }
    }
    workgroupBarrier();

    if (in_range && !dc_only) {
        var tab: array<i32, 32>;
        if (t == 0u || t == 4u) { tab = TAB04; }
        else if (t == 1u || t == 7u) { tab = TAB17; }
        else if (t == 2u || t == 6u) { tab = TAB26; }
        else { tab = TAB35; }
        let row = idct_row(
            sm[base + t * 9u], sm[base + t * 9u + 1u], sm[base + t * 9u + 2u], sm[base + t * 9u + 3u],
            sm[base + t * 9u + 4u], sm[base + t * 9u + 5u], sm[base + t * 9u + 6u], sm[base + t * 9u + 7u],
            tab,
        );
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            sm[base + t * 9u + k] = row[k];
        }
    }
    workgroupBarrier();

    if (in_range && !dc_only) {
        let col = idct_column(
            sm[base + t], sm[base + 9u + t], sm[base + 18u + t], sm[base + 27u + t],
            sm[base + 36u + t], sm[base + 45u + t], sm[base + 54u + t], sm[base + 63u + t],
            add_val,
        );
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            sm[base + k * 9u + t] = col[k];
        }
    }
    workgroupBarrier();
}
