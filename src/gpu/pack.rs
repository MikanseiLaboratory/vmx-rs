//! GPU coefficient packing.
//!
//! Fast (32 bytes / 8x8 block): `i16` DC + `i8` AC[1..30]. Higher AC are
//! treated as zero. This is lossy, but is the cheapest upload.
//!
//! Exact (132 bytes / 8x8 block): 64 `i16` coefficients in zigzag order, then
//! one `u32` flag. The flag is 1 for blocks where the CPU decoder takes its
//! DC-only broadcast shortcut. With an integer IDCT and the flag, the GPU
//! output is bit-identical to the CPU decoder.

pub const PACK_BYTES_FAST: usize = 32;
pub const PACK_BYTES_EXACT: usize = 132;
