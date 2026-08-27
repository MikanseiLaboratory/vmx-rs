// Float AAN IDCT. One workgroup (64 threads) covers eight 8×8 blocks.
// Thread t in a block owns row t then column t (no redundant 8× ALU).

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
@group(0) @binding(1) var<storage, read> tables: array<i32>;
@group(0) @binding(2) var<storage, read> coeffs: array<u32>;
@group(0) @binding(3) var<storage, read> valid: array<u32>;
@group(0) @binding(4) var<storage, read_write> plane_out: array<u32>;

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

var<workgroup> sm: array<f32, 512>;

fn load_i16(block: u32, i: u32) -> i32 {
    let w = coeffs[block * 32u + (i >> 1u)];
    return (i32(w >> ((i & 1u) * 16u)) << 16u) >> 16u;
}

fn idct_tab(row: u32, i: u32) -> f32 {
    return f32(tables[row * 32u + i]);
}

fn dec_matrix(i: u32) -> i32 {
    return tables[256u + i];
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

@compute @workgroup_size(64)
fn main(
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let b_local = lid / 8u;
    let t = lid % 8u;
    let block = wid.x * 8u + b_local;
    let base = b_local * 64u;
    let add_val = f32(params.add_val);
    let in_range = block < params.block_count;
    let dc_only = in_range && valid[block] == 0u;

    if (dc_only) {
        let dc = load_i16(block, 0u);
        let v = u32(clamp(f32((dc + 4) >> 3u) + add_val, 0.0, 255.0));
        let bx = block % params.blocks_x;
        let by = block / params.blocks_x;
        let px = bx * 8u;
        let py = by * 8u;
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            plane_out[(py + t) * params.stride + (px + k)] = v;
        }
    }

    if (in_range && !dc_only) {
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            let i = t * 8u + k;
            let c = load_i16(block, ZIGZAG_INV[i]);
            let dq = ((c * dec_matrix(i)) << 16u) >> 16u;
            sm[base + i] = f32(dq) / 16.0;
        }
    }
    workgroupBarrier();

    if (in_range && !dc_only) {
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

    if (in_range && !dc_only) {
        let col = idct_column(
            sm[base + t], sm[base + 8u + t], sm[base + 16u + t], sm[base + 24u + t],
            sm[base + 32u + t], sm[base + 40u + t], sm[base + 48u + t], sm[base + 56u + t],
            add_val,
        );
        let bx = block % params.blocks_x;
        let by = block / params.blocks_x;
        let px = bx * 8u + t;
        let py = by * 8u;
        for (var k: u32 = 0u; k < 8u; k = k + 1u) {
            plane_out[(py + k) * params.stride + px] = u32(clamp(col[k], 0.0, 255.0));
        }
    }
}
