// Fused float AAN IDCT + YUV 4:2:2 → BGRA.
// One workgroup (32 threads) covers a 16×8 luma tile: 2 Y blocks + 1 U + 1 V.
// IDCT row tables are shader constants (no per-frame table fetch).

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
@group(0) @binding(1) var<storage, read> matrix: array<i32>;
@group(0) @binding(2) var<storage, read> coeffs: array<u32>;

const IDCT_TG1: f32 = 13036.0 / 65536.0;
const IDCT_TG2: f32 = 27146.0 / 65536.0;
const IDCT_TG3: f32 = -21746.0 / 65536.0;
const IDCT_COS4: f32 = -19195.0 / 65536.0;

const ZIGZAG_INV: array<u32, 64> = array<u32, 64>(
    0u, 1u, 5u, 6u, 14u, 15u, 27u, 28u,
    2u, 4u, 7u, 13u, 16u, 26u, 29u, 42u,
    3u, 8u, 12u, 17u, 25u, 30u, 41u, 43u,
    9u, 11u, 18u, 24u, 31u, 40u, 44u, 53u,
    10u, 19u, 23u, 32u, 39u, 45u, 52u, 54u,
    20u, 22u, 33u, 38u, 46u, 51u, 55u, 60u,
    21u, 34u, 37u, 47u, 50u, 56u, 59u, 61u,
    35u, 36u, 48u, 49u, 57u, 58u, 62u, 63u,
);

const TAB04: array<f32, 32> = array<f32, 32>(
    16384.0, 21407.0, 16384.0, 8867.0, 16384.0, -8867.0, 16384.0, -21407.0,
    16384.0, 8867.0, -16384.0, -21407.0, -16384.0, 21407.0, 16384.0, -8867.0,
    22725.0, 19266.0, 19266.0, -4520.0, 12873.0, -22725.0, 4520.0, -12873.0,
    12873.0, 4520.0, -22725.0, -12873.0, 4520.0, 19266.0, 19266.0, -22725.0,
);
const TAB17: array<f32, 32> = array<f32, 32>(
    22725.0, 29692.0, 22725.0, 12299.0, 22725.0, -12299.0, 22725.0, -29692.0,
    22725.0, 12299.0, -22725.0, -29692.0, -22725.0, 29692.0, 22725.0, -12299.0,
    31521.0, 26722.0, 26722.0, -6270.0, 17855.0, -31521.0, 6270.0, -17855.0,
    17855.0, 6270.0, -31521.0, -17855.0, 6270.0, 26722.0, 26722.0, -31521.0,
);
const TAB26: array<f32, 32> = array<f32, 32>(
    21407.0, 27969.0, 21407.0, 11585.0, 21407.0, -11585.0, 21407.0, -27969.0,
    21407.0, 11585.0, -21407.0, -27969.0, -21407.0, 27969.0, 21407.0, -11585.0,
    29692.0, 25172.0, 25172.0, -5906.0, 16819.0, -29692.0, 5906.0, -16819.0,
    16819.0, 5906.0, -29692.0, -16819.0, 5906.0, 25172.0, 25172.0, -29692.0,
);
const TAB35: array<f32, 32> = array<f32, 32>(
    19266.0, 25172.0, 19266.0, 10426.0, 19266.0, -10426.0, 19266.0, -25172.0,
    19266.0, 10426.0, -19266.0, -25172.0, -19266.0, 25172.0, 19266.0, -10426.0,
    26722.0, 22654.0, 22654.0, -5315.0, 15137.0, -26722.0, 5315.0, -15137.0,
    15137.0, 5315.0, -26722.0, -15137.0, 5315.0, 22654.0, 22654.0, -26722.0,
);

var<workgroup> sm: array<f32, 256>;

fn idct_tab(row: u32, i: u32) -> f32 {
    switch row {
        case 0u, 4u: { return TAB04[i]; }
        case 1u, 7u: { return TAB17[i]; }
        case 2u, 6u: { return TAB26[i]; }
        default: { return TAB35[i]; }
    }
}

fn load_word(plane: u32, block: u32, wi: u32) -> u32 {
    var base: u32;
    if (plane <= 1u) {
        base = 0u;
    } else if (plane == 2u) {
        base = params.u_word_off;
    } else {
        base = params.v_word_off;
    }
    return coeffs[base + block * 8u + wi];
}

fn load_coeff(plane: u32, block: u32, zig: u32) -> i32 {
    if (zig == 0u) {
        let w = load_word(plane, block, 0u);
        return (i32(w) << 16u) >> 16u;
    }
    if (zig > 30u) {
        return 0;
    }
    let bi = zig + 1u;
    let w = load_word(plane, block, bi >> 2u);
    let s = (bi & 3u) * 8u;
    return (i32(w >> s) << 24u) >> 24u;
}

fn dec_matrix(i: u32) -> i32 {
    return matrix[i];
}

fn idct_row(x0: f32, x1: f32, x2: f32, x3: f32, x4: f32, x5: f32, x6: f32, x7: f32, row: u32) -> array<f32, 8> {
    var out: array<f32, 8>;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        let even = x0 * idct_tab(row, 2u * i)
            + x2 * idct_tab(row, 2u * i + 1u)
            + 1024.0
            + x4 * idct_tab(row, 8u + 2u * i)
            + x6 * idct_tab(row, 8u + 2u * i + 1u);
        let odd = x5 * idct_tab(row, 24u + 2u * i)
            + x7 * idct_tab(row, 24u + 2u * i + 1u)
            + x1 * idct_tab(row, 16u + 2u * i)
            + x3 * idct_tab(row, 16u + 2u * i + 1u);
        out[i] = (even + odd) / 2048.0;
        out[7u - i] = (even - odd) / 2048.0;
    }
    return out;
}

fn idct_column(r0: f32, r1: f32, r2: f32, r3: f32, r4: f32, r5: f32, r6: f32, r7: f32, add_val: f32) -> array<f32, 8> {
    var x0 = IDCT_TG3 * r5 + r5;
    let x1 = IDCT_TG3 * r3 + r3;
    x0 = x0 + r3;
    let x2 = r5 - x1;
    let x5 = IDCT_TG1 * r1 - r7;
    let x4 = IDCT_TG1 * r7 + r1;

    let temp7 = x0 + x4 + 1.0;
    let t4 = x4 - x0;
    let t5 = x5 - x2 + 1.0;
    let temp3 = x5 + x2;

    let s = t4 + t5;
    let d = t4 - t5;
    let m4 = s + IDCT_COS4 * s;
    let m0 = IDCT_COS4 * d + d;

    let e7 = IDCT_TG2 * r6 + r2;
    let e3 = IDCT_TG2 * r2 - r6;
    let sum04 = r4 + r0;
    let dif04 = r0 - r4;

    let b0 = sum04 + e7 + 32.0;
    let b3 = sum04 - e7 + 31.0;
    let b1 = dif04 + e3 + 32.0;
    let b2 = dif04 - e3 + 31.0;

    var out: array<f32, 8>;
    out[0] = (temp7 + b0) / 64.0 + add_val;
    out[1] = (b1 + m4) / 64.0 + add_val;
    out[2] = (b2 + m0) / 64.0 + add_val;
    out[3] = (temp3 + b3) / 64.0 + add_val;
    out[4] = (b3 - temp3) / 64.0 + add_val;
    out[5] = (b2 - m0) / 64.0 + add_val;
    out[6] = (b1 - m4) / 64.0 + add_val;
    out[7] = (b0 - temp7) / 64.0 + add_val;
    return out;
}

fn mulhi_i16(a: i32, b: i32) -> i32 {
    return (a * b) >> 16u;
}

fn sat_add_i16(a: i32, b: i32) -> i32 {
    return clamp(a + b, -32768, 32767);
}

fn sat_sub_i16(a: i32, b: i32) -> i32 {
    return clamp(a - b, -32768, 32767);
}

fn yuv_to_rgba(yy: u32, cb: i32, cr: i32) -> vec4<f32> {
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
    return vec4<f32>(f32(ro), f32(go), f32(bo), 255.0) / 255.0;
}

fn yuv_to_packed(yy: u32, cb: i32, cr: i32) -> u32 {
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

fn run_idct(lid: u32, tile: u32) {
    let u_bx = max(params.u_blocks_x, 1u);
    let y_bx = params.y_blocks_x;
    let cx = tile % u_bx;
    let cy = tile / u_bx;

    let plane = lid / 8u;
    let t = lid % 8u;
    let base = plane * 64u;

    var block = 0u;
    var in_range = false;
    var add_val = 0.0;
    if (plane == 0u) {
        block = cy * y_bx + cx * 2u;
        in_range = (cx * 2u) < y_bx;
        add_val = 128.0;
    } else if (plane == 1u) {
        block = cy * y_bx + cx * 2u + 1u;
        in_range = (cx * 2u + 1u) < y_bx;
        add_val = 128.0;
    } else {
        block = cy * u_bx + cx;
        in_range = cx < u_bx;
        add_val = 0.0;
    }

    if (in_range) {
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            let i = t * 8u + k;
            let c = load_coeff(plane, block, ZIGZAG_INV[i]);
            let dq = ((c * dec_matrix(i)) << 16u) >> 16u;
            sm[base + i] = f32(dq) / 16.0;
        }
    } else {
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            sm[base + t * 8u + k] = 0.0;
        }
    }
    workgroupBarrier();

    if (in_range) {
        let row = idct_row(
            sm[base + t * 8u], sm[base + t * 8u + 1u], sm[base + t * 8u + 2u], sm[base + t * 8u + 3u],
            sm[base + t * 8u + 4u], sm[base + t * 8u + 5u], sm[base + t * 8u + 6u], sm[base + t * 8u + 7u],
            t,
        );
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            sm[base + t * 8u + k] = row[k];
        }
    }
    workgroupBarrier();

    if (in_range) {
        let col = idct_column(
            sm[base + t], sm[base + 8u + t], sm[base + 16u + t], sm[base + 24u + t],
            sm[base + 32u + t], sm[base + 40u + t], sm[base + 48u + t], sm[base + 56u + t],
            add_val,
        );
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            sm[base + k * 8u + t] = col[k];
        }
    }
    workgroupBarrier();
}

fn tile_px(lid: u32, tile: u32) -> vec4<u32> {
    let u_bx = max(params.u_blocks_x, 1u);
    let cx = tile % u_bx;
    let cy = tile / u_bx;
    let lx = lid;
    let px = cx * 16u + lx;
    let py0 = cy * 8u;
    return vec4<u32>(px, py0, lx, u32(px < params.width));
}
