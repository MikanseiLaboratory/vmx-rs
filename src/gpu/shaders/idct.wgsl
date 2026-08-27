// Integer AAN IDCT matching vmx-rs scalar `zig_invquant_idct` / `broadcast_dc`.
// Storage buffers packed to stay within 8 bindings (downlevel adapters).

struct DecodeParams {
    blocks_x: u32,
    blocks_y: u32,
    stride: u32,
    add_val: i32,
    block_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> params: DecodeParams;
// tables: [0..64) zigzag_inv, [64..320) idct_row_tables, [320..384) decode_matrix
@group(0) @binding(1) var<storage, read> tables: array<i32>;
// headers: dc (bitcast), valid, nnz, ac_off
@group(0) @binding(2) var<storage, read> headers: array<vec4<u32>>;
// packed AC: x=index, y=value
@group(0) @binding(3) var<storage, read> packed_ac: array<vec2<i32>>;
@group(0) @binding(4) var<storage, read_write> plane_out: array<u32>;

const SHIFT_INV_ROW: i32 = 11;
const SHIFT_INV_COL: i32 = 6;
const IRND_INV_ROW: i32 = 1024;
const IRND_INV_COL: i32 = 32;
const IRND_INV_CORR: i32 = 31;
const IDCT_TG1: i32 = 13036;
const IDCT_TG2: i32 = 27146;
const IDCT_TG3: i32 = -21746;
const IDCT_COS4: i32 = -19195;

fn sat_i16(v: i32) -> i32 { return clamp(v, -32768, 32767); }
fn sat_add_i16(a: i32, b: i32) -> i32 { return sat_i16(a + b); }
fn sat_sub_i16(a: i32, b: i32) -> i32 { return sat_i16(a - b); }
fn mulhi_i16(a: i32, b: i32) -> i32 { return (a * b) >> 16u; }
fn to_i16(v: i32) -> i32 { return (v << 16u) >> 16u; }

fn zigzag_inv(i: u32) -> u32 { return u32(tables[i]); }
fn idct_tab(i: u32) -> i32 { return tables[64u + i]; }
fn dec_matrix(i: u32) -> i32 { return tables[320u + i]; }

fn idct_row(x0: i32, x1: i32, x2: i32, x3: i32, x4: i32, x5: i32, x6: i32, x7: i32, tab_base: u32) -> array<i32, 8> {
    var out: array<i32, 8>;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        let even = (x0 * idct_tab(tab_base + 2u * i)
            + x2 * idct_tab(tab_base + 2u * i + 1u)
            + IRND_INV_ROW
            + x4 * idct_tab(tab_base + 8u + 2u * i)
            + x6 * idct_tab(tab_base + 8u + 2u * i + 1u));
        let odd = (x5 * idct_tab(tab_base + 24u + 2u * i)
            + x7 * idct_tab(tab_base + 24u + 2u * i + 1u)
            + x1 * idct_tab(tab_base + 16u + 2u * i)
            + x3 * idct_tab(tab_base + 16u + 2u * i + 1u));
        out[i] = sat_i16((even + odd) >> u32(SHIFT_INV_ROW));
        out[7u - i] = sat_i16((even - odd) >> u32(SHIFT_INV_ROW));
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

    let b0 = sat_add_i16(sat_add_i16(sum04, e7), IRND_INV_COL);
    let b3 = sat_add_i16(sat_sub_i16(sum04, e7), IRND_INV_CORR);
    let b1 = sat_add_i16(sat_add_i16(dif04, e3), IRND_INV_COL);
    let b2 = sat_add_i16(sat_sub_i16(dif04, e3), IRND_INV_CORR);

    var out: array<i32, 8>;
    out[0] = sat_add_i16(sat_add_i16(temp7, b0) >> u32(SHIFT_INV_COL), add_val);
    out[1] = sat_add_i16(sat_add_i16(b1, m4) >> u32(SHIFT_INV_COL), add_val);
    out[2] = sat_add_i16(sat_add_i16(b2, m0) >> u32(SHIFT_INV_COL), add_val);
    out[3] = sat_add_i16(sat_add_i16(temp3, b3) >> u32(SHIFT_INV_COL), add_val);
    out[4] = sat_add_i16(sat_sub_i16(b3, temp3) >> u32(SHIFT_INV_COL), add_val);
    out[5] = sat_add_i16(sat_sub_i16(b2, m0) >> u32(SHIFT_INV_COL), add_val);
    out[6] = sat_add_i16(sat_sub_i16(b1, m4) >> u32(SHIFT_INV_COL), add_val);
    out[7] = sat_add_i16(sat_sub_i16(b0, temp7) >> u32(SHIFT_INV_COL), add_val);
    return out;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let block = gid.x;
    if (block >= params.block_count) { return; }

    var coeffs: array<i32, 64>;
    for (var i: u32 = 0u; i < 64u; i = i + 1u) { coeffs[i] = 0; }
    let hdr = headers[block];
    let dc = bitcast<i32>(hdr.x);
    coeffs[0] = dc;
    let n = hdr.z;
    let off = hdr.w;
    for (var k: u32 = 0u; k < n; k = k + 1u) {
        let ac = packed_ac[off + k];
        coeffs[u32(ac.x)] = ac.y;
    }

    let bx = block % params.blocks_x;
    let by = block / params.blocks_x;
    let px = bx * 8u;
    let py = by * 8u;
    let stride = params.stride;

    if (hdr.y == 0u) {
        let v = sat_i16((dc + 4) >> 3u) + params.add_val;
        let pix = u32(clamp(v, 0, 255));
        for (var y: u32 = 0u; y < 8u; y = y + 1u) {
            for (var x: u32 = 0u; x < 8u; x = x + 1u) {
                plane_out[(py + y) * stride + (px + x)] = pix;
            }
        }
        return;
    }

    var rows: array<array<i32, 8>, 8>;
    for (var i: u32 = 0u; i < 64u; i = i + 1u) {
        let c = coeffs[zigzag_inv(i)];
        let m = dec_matrix(i);
        rows[i / 8u][i % 8u] = to_i16(c * m) >> 4u;
    }

    for (var y: u32 = 0u; y < 8u; y = y + 1u) {
        let r = idct_row(rows[y][0], rows[y][1], rows[y][2], rows[y][3], rows[y][4], rows[y][5], rows[y][6], rows[y][7], y * 32u);
        rows[y] = r;
    }

    for (var x: u32 = 0u; x < 8u; x = x + 1u) {
        let col = idct_column(rows[0][x], rows[1][x], rows[2][x], rows[3][x], rows[4][x], rows[5][x], rows[6][x], rows[7][x], params.add_val);
        for (var y: u32 = 0u; y < 8u; y = y + 1u) {
            plane_out[(py + y) * stride + (px + x)] = u32(clamp(col[y], 0, 255));
        }
    }
}
