//! Dense zigzag coefficient packing for GPU IDCT.

use crate::codec::plane::CoeffBlock;

/// GPU-uploadable dense zigzag plane (64 i16 per 8×8 block).
#[derive(Clone, Debug, Default)]
pub struct DensePlane {
    pub blocks_x: u32,
    pub blocks_y: u32,
    pub stride: u32,
    pub add_val: i32,
    pub coeffs: Vec<i16>,
    pub valid: Vec<u32>,
}

impl DensePlane {
    pub fn from_packs(
        packs: &[[Vec<CoeffBlock>; 3]],
        plane: usize,
        stride: usize,
        add_val: i32,
    ) -> Self {
        let n: usize = packs.iter().map(|s| s[plane].len()).sum();
        let blocks_x = (stride / 8) as u32;
        let mut coeffs = vec![0i16; n * 64];
        let mut valid = vec![0u32; n.max(1)];
        let mut bi = 0usize;
        for slice in packs {
            for block in &slice[plane] {
                coeffs[bi * 64..bi * 64 + 64].copy_from_slice(&block.coeffs);
                valid[bi] = u32::from(block.valid);
                bi += 1;
            }
        }
        let blocks_y = (bi as u32).checked_div(blocks_x).unwrap_or(0);
        Self {
            blocks_x,
            blocks_y,
            stride: stride as u32,
            add_val,
            coeffs,
            valid,
        }
    }

    pub fn block_count(&self) -> u32 {
        self.blocks_x.saturating_mul(self.blocks_y)
    }
}
