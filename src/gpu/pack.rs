//! Compact zigzag packing for GPU IDCT (32 bytes / 8×8 block).
//!
//! Layout: `i16` DC + `i8` AC[1..30] (zigzag). Higher AC are treated as 0.

pub const PACK_BYTES: usize = 32;
