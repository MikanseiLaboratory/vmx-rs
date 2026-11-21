/-
  vmx-rs wgpu path — machine-checked facts used by the 2026-11 audit.

  Scope
  -----
  Lean proves *equivalence*, not speed.  The performance claims in the audit
  come from `examples/gpu_bench.rs` and stage traces; the theorems below cover
  the rewrites that the audit proposes:

    * the zigzag permutation used to pack/unpack coefficients,
    * exactly when the current 32-byte GPU coefficient pack is lossless,
    * why the float IDCT's `/16.0` is not the CPU's arithmetic `>>4`,
    * why the CPU's i16 saturation cannot be dropped,
    * why the proposed 9-float shared-memory stride removes bank conflicts,
    * the texture-load reduction of a fused 16x8 encode tile.

  Everything here is core Lean 4 (no Mathlib).
-/

namespace VmxGpu

/-! ## Zigzag scan and its inverse

`zigzag` is libvmx `ZIGZAG`; `zigzagInv` is `ZIGZAG_INV`.  The decode shader
embeds the same 64 entries as `ZIGZAG_INV`. -/

def zigzag : List Nat :=
  [0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5,
   12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28,
   35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
   58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63]

def zigzagInv : List Nat :=
  [0, 1, 5, 6, 14, 15, 27, 28, 2, 4, 7, 13, 16, 26, 29, 42,
   3, 8, 12, 17, 25, 30, 41, 43, 9, 11, 18, 24, 31, 40, 44, 53,
   10, 19, 23, 32, 39, 45, 52, 54, 20, 22, 33, 38, 46, 51, 55, 60,
   21, 34, 37, 47, 50, 56, 59, 61, 35, 36, 48, 49, 57, 58, 62, 63]

def nth (l : List Nat) (i : Nat) : Nat := l.getD i 0

def zigzagAt (i : Nat) : Nat := nth zigzag i

def zigzagInvAt (i : Nat) : Nat := nth zigzagInv i

theorem zigzag_length : zigzag.length = 64 := by decide

theorem zigzagInv_length : zigzagInv.length = 64 := by decide

/-- The scan has no repeated position, so it is a permutation of `0..63`. -/
theorem zigzag_nodup : zigzag.Nodup := by decide

theorem zigzagInv_nodup : zigzagInv.Nodup := by decide

/-- `zigzagInv` is a left inverse of `zigzag` on the whole 0..63 range. -/
theorem zigzagInv_zigzag (i : Fin 64) : zigzagInvAt (zigzagAt i.val) = i.val := by
  decide +revert

/-- It is also a right inverse. -/
theorem zigzag_zigzagInv (i : Fin 64) : zigzagAt (zigzagInvAt i.val) = i.val := by
  decide +revert

/-- Encode: for each raster index `i`, write `quant i` at scan position
`zigzagInv i`.  In Lean: `encodePack q p = q (zigzag p)`. -/
def encodePack (q : Nat → Int) : Nat → Int := fun p => q (zigzagAt p)

/-- Decode: read raster index `i` from scan position `zigzagInv i`. -/
def decodeLoad (z : Nat → Int) (i : Nat) : Int := z (zigzagInvAt i)

/-- The GPU encode/store + decode/load pair is the identity for every one of
the 64 raster indices.  This is the property that lets the shader pack the
quantized block in zigzag order and read it back without a raster array. -/
theorem zigzag_roundtrip (q : Nat → Int) (i : Fin 64) :
    decodeLoad (encodePack q) i.val = q i.val := by
  unfold decodeLoad encodePack
  rw [zigzag_zigzagInv i]

/-! ## The current 32-byte coefficient pack

`decode_plane_coeffs` writes an i16 DC at bytes 0..1 and `AC[1..30]` as i8 at
bytes 2..31.  Coefficients 31..63 are dropped.  This models the value that
actually reaches the shader. -/

def clampI8 (x : Int) : Int := max (-128) (min 127 x)

/-- The only 32-byte records are `i16 dc + i8[30]`, i.e. 31 coefficients. -/
def packedCoeffCount : Nat := 31

theorem packed_coeff_count_lt_64 : packedCoeffCount < 64 := by decide

theorem clampI8_eq_self {x : Int} (h1 : -128 ≤ x) (h2 : x ≤ 127) :
    clampI8 x = x := by
  unfold clampI8
  omega

theorem clampI8_of_gt (x : Int) (h : 127 < x) : clampI8 x = 127 := by
  unfold clampI8
  omega

theorem clampI8_of_lt (x : Int) (h : x < -128) : clampI8 x = -128 := by
  unfold clampI8
  omega

/-- What the shader reads for coefficient `i` after `decode_plane_coeffs`. -/
def gpuCoeff (c : Nat → Int) (i : Nat) : Int :=
  if i = 0 then c 0
  else if i ≤ 30 then clampI8 (c i)
  else 0

/-- The pack is lossless for a coefficient iff it is the DC or an AC in
`1..30` that fits in i8. -/
theorem gpuCoeff_exact_of_in_range (c : Nat → Int) (i : Nat)
    (h : i = 0 ∨ (1 ≤ i ∧ i ≤ 30 ∧ -128 ≤ c i ∧ c i ≤ 127)) :
    gpuCoeff c i = c i := by
  rcases h with rfl | ⟨h1, h30, hlo, hhi⟩
  · simp [gpuCoeff]
  · have h0 : i ≠ 0 := by omega
    simp [gpuCoeff, h0, h30, clampI8_eq_self hlo hhi]

/-- Conversely, a non-zero coefficient above index 30 is lost. -/
theorem gpuCoeff_drops_high_ac (c : Nat → Int) (i : Nat) (hi : 30 < i)
    (hc : c i ≠ 0) : gpuCoeff c i ≠ c i := by
  have h0 : i ≠ 0 := by omega
  have h30 : ¬ i ≤ 30 := by omega
  simp [gpuCoeff, h0, h30]
  omega

/-- A positive AC above 127 is clamped to 127. -/
theorem gpuCoeff_saturates_high (c : Nat → Int) (i : Nat) (hi : 1 ≤ i)
    (h30 : i ≤ 30) (h : 127 < c i) : gpuCoeff c i ≠ c i := by
  have h0 : i ≠ 0 := by omega
  simp [gpuCoeff, h0, h30, clampI8_of_gt (c i) h]
  omega

/-- A negative AC below -128 is clamped to -128. -/
theorem gpuCoeff_saturates_low (c : Nat → Int) (i : Nat) (hi : 1 ≤ i)
    (h30 : i ≤ 30) (h : c i < -128) : gpuCoeff c i ≠ c i := by
  have h0 : i ≠ 0 := by omega
  simp [gpuCoeff, h0, h30, clampI8_of_lt (c i) h]
  omega

/-- The exact alternative: a 128-byte record (64 i16) preserves every
coefficient, including high AC and values outside i8. -/
def fullCoeff (c : Nat → Int) (i : Nat) : Int := c i

theorem fullCoeff_exact (c : Nat → Int) (i : Nat) : fullCoeff c i = c i := rfl

/-! ## Float division vs. the CPU's arithmetic shift

The float IDCT does `dq / 16.0`; the CPU does `dq >> 4` on an i16.  For
negative non-multiples of 16 these differ by one: truncation toward zero
versus floor.  This is one reason the float path cannot be bit-exact. -/

def truncDiv16 (x : Int) : Int := Int.tdiv x 16

def shiftDiv16 (x : Int) : Int := x / 16

theorem float_div16_ne_shift_for_negative (x : Int) (hx : x < 0)
    (hr : x % 16 ≠ 0) : truncDiv16 x ≠ shiftDiv16 x := by
  have hnotdvd : ¬ (16 : Int) ∣ x := by
    intro hd
    exact hr (Int.emod_eq_zero_of_dvd hd)
  have h0 : ¬ (0 ≤ x ∨ (16 : Int) ∣ x) := by omega
  unfold truncDiv16 shiftDiv16
  rw [Int.tdiv_eq_ediv, ite_eq_right h0]
  have hsign : Int.sign 16 = 1 := by decide
  rw [hsign]
  omega

/-- A concrete instance: `-1 / 16.0` truncates to 0, `-1 >> 4` is -1. -/
example : truncDiv16 (-1) = 0 := by decide

example : shiftDiv16 (-1) = -1 := by decide

/-! ## i16 saturation cannot be dropped

The CPU kernels saturate after every i16 add/sub and after the row shift.
The float IDCT keeps f32 until the final clamp, so an intermediate outside
i16 changes the result. -/

def satI16 (x : Int) : Int := max (-32768) (min 32767 x)

theorem satI16_of_gt (x : Int) (h : 32767 < x) : satI16 x = 32767 := by
  unfold satI16
  omega

theorem satI16_of_lt (x : Int) (h : x < -32768) : satI16 x = -32768 := by
  unfold satI16
  omega

theorem saturation_changes_value (x : Int) (h : 32767 < x) : satI16 x ≠ x := by
  rw [satI16_of_gt x h]
  omega

/-! ## Shared-memory bank conflicts

The current decode tile uses a stride of 8 f32 between rows.  With 32 banks,
`t * 8 mod 32` has only four distinct values, so eight threads writing one
column conflict.  A stride of 9 is coprime with 32: `t * 9 + k mod 32` is
injective, so the same access is conflict-free. -/

theorem padded_stride_injective (k : Nat) :
    Function.Injective (fun t : Fin 32 => (t.val * 9 + k) % 32) := by
  intro a b h
  have hval : (a.val * 9 + k) % 32 = (b.val * 9 + k) % 32 := h
  have : a.val = b.val := by omega
  exact Fin.ext this

theorem unpadded_stride_not_injective :
    ¬ Function.Injective (fun t : Fin 32 => (t.val * 8) % 32) := by
  intro h
  have h0 : (0 : Fin 32) ≠ 4 := by decide
  have h4 : ((0 : Fin 32).val * 8) % 32 = ((4 : Fin 32).val * 8) % 32 := by
    decide
  exact h0 (h h4)

/-! ## Fused 16x8 encode tile

A 16x8 luma tile is 2 Y blocks, 1 U block, 1 V block and 2 alpha blocks.
The current four dispatches load:

  Y: 2 * 64 = 128
  U: 8*8 chroma samples * 2 source pixels = 128
  V: 8*8 chroma samples * 2 source pixels = 128
  A: 2 * 64 = 128
  total = 512 texture reads

Loading the 16x8 source once, computing Y/U/V/A from it, is 128 reads.
Pair fusion (Y+A, U+V) is 256 reads. -/

def tileLoadsCurrent : Nat := 128 + 128 + 128 + 128

def tileLoadsPairFused : Nat := 128 + 128

def tileLoadsFullyFused : Nat := 128

theorem pair_fusion_halves_loads : tileLoadsCurrent = 2 * tileLoadsPairFused := by
  decide

theorem full_fusion_quarters_loads : tileLoadsCurrent = 4 * tileLoadsFullyFused := by
  decide

theorem full_fusion_is_less : tileLoadsFullyFused < tileLoadsCurrent := by
  decide

end VmxGpu
