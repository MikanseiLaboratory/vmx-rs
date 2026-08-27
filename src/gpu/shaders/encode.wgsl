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
    src_rgba: u32,
}

@group(0) @binding(0) var<uniform> params: EncodeParams;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> tables: array<i32>;
@group(0) @binding(3) var<storage, read_write> coeffs: array<u32>;

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
fn ftab(i: u32) -> i32 { return tables[i]; }
fn zigzag_inv(i: u32) -> u32 { return u32(tables[128u + i]); }
fn enc_matrix(i: u32) -> u32 { return u32(tables[192u + i]); }

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

fn fdct_quant_from_rows(rows: array<array<i32, 8>, 8>, out_base: u32) {
    var cols: array<array<i32, 8>, 8>;
    for (var x: u32 = 0u; x < 8u; x = x + 1u) {
        let c = fdct_column_one(array<i32, 8>(rows[0][x], rows[1][x], rows[2][x], rows[3][x], rows[4][x], rows[5][x], rows[6][x], rows[7][x]));
        for (var y: u32 = 0u; y < 8u; y = y + 1u) {
            cols[y][x] = c[y];
        }
    }
    let ftab_order = array<u32, 8>(0u, 1u, 2u, 3u, 0u, 3u, 2u, 1u);
    var spatial: array<i32, 64>;
    for (var y: u32 = 0u; y < 8u; y = y + 1u) {
        let row_out = fdct_row(cols[y], ftab_order[y] * 32u);
        for (var x: u32 = 0u; x < 8u; x = x + 1u) {
            spatial[y * 8u + x] = row_out[x];
        }
    }
    var zig: array<i32, 64>;
    for (var i: u32 = 0u; i < 64u; i = i + 1u) {
        zig[zigzag_inv(i)] = spatial_quant(spatial[i], i);
    }
    let word_base = out_base >> 1u;
    for (var w: u32 = 0u; w < 32u; w = w + 1u) {
        let a = u32(zig[w * 2u]) & 0xFFFFu;
        let b = u32(zig[w * 2u + 1u]) & 0xFFFFu;
        coeffs[word_base + w] = a | (b << 16u);
    }
}

@compute @workgroup_size(64)
fn fdct_y(@builtin(global_invocation_id) gid: vec3<u32>) {
    let block = gid.x;
    let count = params.y_blocks_x * params.y_blocks_y;
    if (block >= count) { return; }
    let bx = block % params.y_blocks_x;
    let by = block / params.y_blocks_x;
    let px = bx * 8u;
    let py = by * 8u;
    var rows: array<array<i32, 8>, 8>;
    for (var y: u32 = 0u; y < 8u; y = y + 1u) {
        for (var x: u32 = 0u; x < 8u; x = x + 1u) {
            rows[y][x] = sample_y(px + x, py + y);
        }
    }
    fdct_quant_from_rows(rows, block * 64u);
}

@compute @workgroup_size(64)
fn fdct_u(@builtin(global_invocation_id) gid: vec3<u32>) {
    let block = gid.x;
    let count = params.u_blocks_x * params.u_blocks_y;
    if (block >= count) { return; }
    let bx = block % params.u_blocks_x;
    let by = block / params.u_blocks_x;
    let cx = bx * 8u;
    let cy = by * 8u;
    var rows: array<array<i32, 8>, 8>;
    for (var y: u32 = 0u; y < 8u; y = y + 1u) {
        for (var x: u32 = 0u; x < 8u; x = x + 1u) {
            rows[y][x] = sample_u(cx + x, cy + y);
        }
    }
    fdct_quant_from_rows(rows, params.u_coeff_off + block * 64u);
}

@compute @workgroup_size(64)
fn fdct_v(@builtin(global_invocation_id) gid: vec3<u32>) {
    let block = gid.x;
    let count = params.u_blocks_x * params.u_blocks_y;
    if (block >= count) { return; }
    let bx = block % params.u_blocks_x;
    let by = block / params.u_blocks_x;
    let cx = bx * 8u;
    let cy = by * 8u;
    var rows: array<array<i32, 8>, 8>;
    for (var y: u32 = 0u; y < 8u; y = y + 1u) {
        for (var x: u32 = 0u; x < 8u; x = x + 1u) {
            rows[y][x] = sample_v(cx + x, cy + y);
        }
    }
    fdct_quant_from_rows(rows, params.v_coeff_off + block * 64u);
}

@compute @workgroup_size(64)
fn fdct_a(@builtin(global_invocation_id) gid: vec3<u32>) {
    let block = gid.x;
    let count = params.y_blocks_x * params.y_blocks_y;
    if (block >= count) { return; }
    let bx = block % params.y_blocks_x;
    let by = block / params.y_blocks_x;
    let px = bx * 8u;
    let py = by * 8u;
    var rows: array<array<i32, 8>, 8>;
    for (var y: u32 = 0u; y < 8u; y = y + 1u) {
        for (var x: u32 = 0u; x < 8u; x = x + 1u) {
            rows[y][x] = sample_a(px + x, py + y);
        }
    }
    fdct_quant_from_rows(rows, params.a_coeff_off + block * 64u);
}
