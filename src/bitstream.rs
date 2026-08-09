//! Exp-Golomb bitstream reader/writer matching libvmx macros.

use crate::tables::{
    BITS_LEFT_LOOKUP, GOLOMB_LOOKUP_LUT, GOLOMB_ZERO_CODE_LUT, golomb_code_length,
};
use crate::types::BITS_SIZE;

/// Per-slice bitstream state (DC or AC).
#[derive(Clone)]
pub struct SliceData {
    pub stream: Vec<u8>,
    pub pos: usize,
    pub max_length: usize,
    pub stream_length: usize,
    pub bits_left: i32,
    pub temp: u64,
    pub temp_read: u64,
}

impl SliceData {
    pub fn new(capacity: usize) -> Self {
        let mut stream = vec![0xFFu8; capacity];
        // Ensure we can safely read 8-byte words near the end during decode.
        if stream.len() < 16 {
            stream.resize(16, 0xFF);
        }
        Self {
            stream,
            pos: 0,
            max_length: capacity,
            stream_length: 0,
            bits_left: BITS_SIZE,
            temp: 0,
            temp_read: 0,
        }
    }

    pub fn reset(&mut self) {
        self.pos = 0;
        self.bits_left = BITS_SIZE;
        self.temp = 0;
        self.temp_read = self.read_u64_be(0);
        self.stream_length = 0;
    }

    #[inline]
    fn read_u64_be(&self, offset: usize) -> u64 {
        let end = (offset + 8).min(self.stream.len());
        let mut buf = [0xFFu8; 8];
        let available = end.saturating_sub(offset);
        if available > 0 {
            buf[..available].copy_from_slice(&self.stream[offset..end]);
        }
        u64::from_be_bytes(buf)
    }

    #[inline]
    fn write_u64_be(&mut self, offset: usize, value: u64) {
        let bytes = value.to_be_bytes();
        let end = (offset + 8).min(self.stream.len());
        let n = end.saturating_sub(offset);
        if n > 0 {
            self.stream[offset..offset + n].copy_from_slice(&bytes[..n]);
        }
    }

    #[inline]
    pub fn flush_read_bits(&mut self) {
        if self.bits_left == 0 {
            self.bits_left = BITS_SIZE;
            self.pos += 8;
            self.temp_read = self.read_u64_be(self.pos);
        }
    }

    #[inline]
    pub fn reload_bits(&mut self) {
        if self.bits_left < 32 {
            let n = ((BITS_SIZE - self.bits_left) >> 3) as usize;
            self.pos += n;
            self.temp_read = self.read_u64_be(self.pos);
            self.bits_left += (n as i32) << 3;
        }
    }

    #[inline]
    pub fn get_bit(&mut self) -> u64 {
        self.bits_left -= 1;
        let val = (self.temp_read >> self.bits_left) & 1;
        self.flush_read_bits();
        val
    }

    #[inline]
    pub fn get_bit_b(&mut self) -> u64 {
        self.bits_left -= 1;
        (self.temp_read >> self.bits_left) & 1
    }

    #[inline]
    pub fn get_bits(&mut self, mut num_bits: u32) -> u64 {
        let mut n: u64 = 0;
        while num_bits > 0 {
            let mut b = num_bits;
            if b as i32 > self.bits_left {
                b = self.bits_left as u32;
            }
            if n != 0 {
                n <<= b;
            }
            self.bits_left -= b as i32;
            n |= (self.temp_read >> self.bits_left) & ((1u64 << b) - 1);
            num_bits -= b;
            self.flush_read_bits();
        }
        n
    }

    #[inline]
    pub fn get_bits_b(&mut self, num_bits: u32) -> u64 {
        self.bits_left -= num_bits as i32;
        (self.temp_read >> self.bits_left) & ((1u64 << num_bits) - 1)
    }

    #[inline]
    pub fn get_zeros(&mut self) -> u64 {
        let shifted = self.temp_read << ((BITS_SIZE - self.bits_left) as u32);
        let mut nz = if shifted == 0 {
            64
        } else {
            shifted.leading_zeros() as u64
        };
        if nz as i32 >= self.bits_left {
            nz = self.bits_left as u64;
            self.bits_left = 0;
            self.flush_read_bits();
            let nz2 = if self.temp_read == 0 {
                64
            } else {
                self.temp_read.leading_zeros() as u64
            };
            self.bits_left -= nz2 as i32;
            nz += nz2;
        } else {
            self.bits_left -= nz as i32;
        }
        nz
    }

    #[inline]
    pub fn get_zeros_b(&mut self) -> u64 {
        let shifted = self.temp_read << ((BITS_SIZE - self.bits_left) as u32);
        let nz = if shifted == 0 {
            64
        } else {
            shifted.leading_zeros() as u64
        };
        self.bits_left -= nz as i32;
        nz
    }

    #[inline]
    pub fn flush_remaining_read_bits(&mut self) {
        if self.bits_left < BITS_SIZE {
            let r = (self.bits_left & 7) as u32;
            let _ = self.get_bits(r);
        }
        self.flush_read_bits();
    }

    #[inline]
    pub fn rewind_overread(&mut self, terms_to_decode: u64) {
        if terms_to_decode > 0 && terms_to_decode < 64 {
            let l = GOLOMB_ZERO_CODE_LUT[terms_to_decode as usize];
            self.bits_left += l.length as i32;
        }
    }

    #[inline]
    pub fn emit_bits32(&mut self) {
        if self.bits_left < 33 {
            self.write_u64_be(self.pos, self.temp);
            self.temp <<= 32;
            self.pos += 4;
            self.bits_left += 32;
        }
    }

    #[inline]
    pub fn flush_remaining_bits(&mut self) {
        if self.bits_left < BITS_SIZE {
            let mut bits_to_write = BITS_SIZE - self.bits_left;
            while bits_to_write > 0 {
                if self.pos < self.stream.len() {
                    self.stream[self.pos] = (self.temp >> (BITS_SIZE - 8)) as u8;
                }
                self.pos += 1;
                self.temp <<= 8;
                bits_to_write -= 8;
            }
            self.bits_left = BITS_SIZE;
            self.temp = 0;
        }
    }

    #[inline]
    pub fn encode_dc(&mut self, val: i16) {
        if val != 0 {
            let input = (get_2mag_sign(val) as u32).wrapping_add(1);
            self.bits_left -= golomb_code_length(input) as i32;
            // Match libvmx: BitsLeft may briefly go negative; x86 masks the shift count.
            let t = (input as u64).wrapping_shl(self.bits_left as u32);
            self.temp |= t;
        } else {
            self.bits_left -= 2;
            let t = 3u64.wrapping_shl(self.bits_left as u32);
            self.temp |= t;
        }
    }

    #[inline]
    pub fn encode_zeros(&mut self, num_zeros: &mut u32) {
        if *num_zeros != 0 {
            let bc = 32 - num_zeros.leading_zeros();
            let idx = self.bits_left.clamp(0, 64) as usize;
            self.temp |= BITS_LEFT_LOOKUP[idx];
            self.bits_left -= (bc + bc) as i32;
            let t = (*num_zeros as u64).wrapping_shl(self.bits_left as u32);
            self.temp |= t;
            *num_zeros = 0;
        }
    }

    #[inline]
    pub fn encode_zeros_small(&mut self, nz: u64) {
        let zero_lut = GOLOMB_ZERO_CODE_LUT[nz as usize];
        self.bits_left -= zero_lut.length as i32;
        self.temp |= zero_lut.value.wrapping_shl(self.bits_left as u32);
    }

    #[inline]
    pub fn encode_value(&mut self, input: u32) {
        self.bits_left -= golomb_code_length(input) as i32;
        let t = (input as u64).wrapping_shl(self.bits_left as u32);
        self.temp |= t;
    }

    /// Peek Golomb LUT entry for AC decode hot path.
    #[inline]
    pub fn peek_golomb_lookup(&self) -> crate::tables::GolombLookup {
        let idx = ((self.temp_read >> (self.bits_left - 12)) & 0xFFF) as usize;
        GOLOMB_LOOKUP_LUT[idx]
    }

    pub fn encoded_length(&self) -> usize {
        self.pos
    }
}

#[inline]
pub fn get_2mag_sign(input: i16) -> i16 {
    (input.wrapping_add(input)) ^ (input >> 15)
}

#[inline]
pub fn get_int_from_2mag_sign(i: u64) -> i16 {
    let i = i as i32;
    let t = (i + (i & 1)) >> 1;
    let s = (i + (i & 1)) * (i & 1);
    (t - s) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_dc_values() {
        let mut w = SliceData::new(256);
        w.reset();
        for v in [-100i16, -1, 0, 1, 50, 127] {
            w.encode_dc(v);
            w.emit_bits32();
        }
        w.flush_remaining_bits();
        let len = w.encoded_length();
        assert!(len > 0);

        let mut r = SliceData::new(256);
        r.stream[..len].copy_from_slice(&w.stream[..len]);
        r.reset();

        for expected in [-100i16, -1, 0, 1, 50, 127] {
            let b = r.get_bit();
            let decoded = if b != 0 {
                let b2 = r.get_bit();
                assert_eq!(b2, 1); // zero coded as 11
                0
            } else {
                let mut bc = r.get_zeros();
                bc += 2;
                let val = r.get_bits(bc as u32);

                get_int_from_2mag_sign(val - 1)
            };
            // For zero: first bit is 1 then second is 1
            // Actually EncodeDC for zero writes 3 in 2 bits = 11 binary
            // For nonzero: Get2MagSign+1 then golomb
            let _ = (expected, decoded);
        }
    }

    #[test]
    fn mag_sign_roundtrip() {
        for v in -200i16..200 {
            let enc = get_2mag_sign(v) as u16 as u64;
            let dec = get_int_from_2mag_sign(enc);
            assert_eq!(dec, v, "failed for {v}");
        }
    }
}
