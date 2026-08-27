//! Sparse coefficient packing for GPU IDCT.

use crate::codec::plane::CoeffBlock;

/// GPU-uploadable sparse coeff plane.
#[derive(Clone, Debug, Default)]
pub struct PlanePack {
    pub blocks_x: u32,
    pub blocks_y: u32,
    pub stride: u32,
    pub add_val: i32,
    pub dc: Vec<i32>,
    pub valid: Vec<u32>,
    pub nnz: Vec<u32>,
    pub ac_off: Vec<u32>,
    pub ac_idx: Vec<u32>,
    pub ac_val: Vec<i32>,
}

impl PlanePack {
    pub fn from_slice_blocks(stride: usize, add_val: i32, slices: &[Vec<CoeffBlock>]) -> Self {
        let blocks_x = (stride / 8) as u32;
        let mut blocks_y = 0u32;
        let mut dc = Vec::new();
        let mut valid = Vec::new();
        let mut nnz = Vec::new();
        let mut ac_off = Vec::new();
        let mut ac_idx = Vec::new();
        let mut ac_val = Vec::new();
        ac_off.push(0);
        for slice in slices {
            let slice_by = if blocks_x == 0 {
                0
            } else {
                (slice.len() as u32) / blocks_x.max(1)
            };
            blocks_y += slice_by;
            for block in slice {
                dc.push(i32::from(block.coeffs[0]));
                valid.push(u32::from(block.valid));
                let mut n = 0u32;
                for (i, &c) in block.coeffs.iter().enumerate().skip(1) {
                    if c != 0 {
                        ac_idx.push(i as u32);
                        ac_val.push(i32::from(c));
                        n += 1;
                    }
                }
                nnz.push(n);
                ac_off.push(ac_idx.len() as u32);
            }
        }
        if ac_idx.is_empty() {
            ac_idx.push(0);
            ac_val.push(0);
        }
        Self {
            blocks_x,
            blocks_y,
            stride: stride as u32,
            add_val,
            dc,
            valid,
            nnz,
            ac_off,
            ac_idx,
            ac_val,
        }
    }

    pub fn block_count(&self) -> u32 {
        self.blocks_x.saturating_mul(self.blocks_y)
    }
}
