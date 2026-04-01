# HFE Read/Write Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `read_hfe` and `write_hfe` to `sd1disk` and wire two CLI subcommands (`hfe-to-img`, `img-to-hfe`) so the tool can round-trip Ensoniq SD-1 disks in HFE v1 format.

**Architecture:** Single new module `crates/sd1disk/src/hfe.rs` containing all MFM encode/decode logic as private functions plus two public entry-points. Three new error variants in `error.rs`. Two new clap subcommands in `sd1cli/src/main.rs` delegate to the library functions.

**Tech Stack:** Rust, `clap` (already in use), standard library only — no new crate dependencies.

**Design spec:** `docs/superpowers/specs/2026-03-30-hfe-support-design.md`

---

## Key domain facts (read before touching code)

- HFE stores bits **LSB-first** per byte. Bit 0 of a byte = oldest bit in time.
- A1\* sync mark = `[0x22, 0x91]` in the HFE byte stream.
- MFM encoding: for each data bit `d` (MSB first), clock `c = !(prev_bit | d)`. Emit `(c, d)` time-ordered; pack 8 time-bits per HFE byte, bit-0 = oldest.
- Block mapping: `block = track×20 + side×10 + sector`. Sectors are **0–9** (not 1–10).
- CRC16-CCITT: poly `0x1021`, init `0xFFFF`. IDAM covers `[0xA1,0xA1,0xA1,0xFE,track,side,sector,0x02]`; DAM covers `[0xA1,0xA1,0xA1,0xFB,data[512]]`.
- Side length = 12,522 MFM bytes. Track interleave: 256-byte chunks alternating side 0 / side 1 within each 512-byte HFE block.

---

## File map

| File | Action |
|---|---|
| `crates/sd1disk/src/error.rs` | Add 3 new `Error` variants + `Display` arms |
| `crates/sd1disk/src/hfe.rs` | **Create** — all HFE/MFM logic |
| `crates/sd1disk/src/lib.rs` | Add `pub mod hfe; pub use hfe::{read_hfe, write_hfe};` |
| `crates/sd1cli/src/main.rs` | Add `HfeToImg`, `ImgToHfe` variants + handler functions |

---

## Task 1 — Add 3 error variants to `error.rs`

**Files:**
- Modify: `crates/sd1disk/src/error.rs`

- [ ] **Step 1.1 — Add the variants to the `Error` enum**

  Insert after the `InvalidName` variant (before `Io`):

  ```rust
  /// HFE file has a bad or unsupported header
  InvalidHfe(&'static str),
  /// CRC mismatch detected while decoding an HFE sector
  HfeCrcMismatch { track: u8, side: u8, sector: u8 },
  /// A sector was not found in the HFE track data
  HfeMissingSector { track: u8, side: u8, sector: u8 },
  ```

- [ ] **Step 1.2 — Add `Display` arms**

  Insert into the `fmt` match (before the `Io` arm):

  ```rust
  Error::InvalidHfe(msg) => write!(f, "Invalid HFE file: {}", msg),
  Error::HfeCrcMismatch { track, side, sector } => write!(
      f, "HFE CRC mismatch at track {} side {} sector {}", track, side, sector
  ),
  Error::HfeMissingSector { track, side, sector } => write!(
      f, "HFE missing sector at track {} side {} sector {}", track, side, sector
  ),
  ```

- [ ] **Step 1.3 — Write the test**

  Append to the `#[cfg(test)]` block in `error.rs`:

  ```rust
  #[test]
  fn hfe_errors_display() {
      let e = Error::InvalidHfe("bad signature");
      assert!(format!("{}", e).contains("bad signature"));

      let e = Error::HfeCrcMismatch { track: 3, side: 1, sector: 7 };
      let s = format!("{}", e);
      assert!(s.contains("3") && s.contains("1") && s.contains("7"));

      let e = Error::HfeMissingSector { track: 0, side: 0, sector: 5 };
      assert!(format!("{}", e).contains("5"));
  }
  ```

- [ ] **Step 1.4 — Run the test**

  ```bash
  cargo test -p sd1disk hfe_errors_display
  ```

  Expected: **PASS**

- [ ] **Step 1.5 — Commit**

  ```bash
  git add crates/sd1disk/src/error.rs
  git commit -m "feat(hfe): add InvalidHfe, HfeCrcMismatch, HfeMissingSector error variants"
  ```

---

## Task 2 — CRC16-CCITT and MFM encode helpers

**Files:**
- Create: `crates/sd1disk/src/hfe.rs`

- [ ] **Step 2.1 — Create `hfe.rs` with stubs + failing tests**

  Create `crates/sd1disk/src/hfe.rs`:

  ```rust
  // crates/sd1disk/src/hfe.rs
  use std::path::Path;
  use crate::{DiskImage, Error, Result};

  // ── private helpers ──────────────────────────────────────────────────────────

  fn crc16_ccitt(_data: &[u8]) -> u16 { 0 }

  fn encode_byte(_byte: u8, _prev_bit: &mut u8) -> [u8; 2] { [0, 0] }

  fn encode_a1_sync() -> [u8; 2] { [0, 0] }

  // ── public API ───────────────────────────────────────────────────────────────

  pub fn read_hfe(_path: &Path) -> Result<DiskImage> {
      Err(Error::InvalidHfe("not yet implemented"))
  }

  pub fn write_hfe(_image: &DiskImage, _path: &Path) -> Result<()> {
      Err(Error::InvalidHfe("not yet implemented"))
  }

  // ── tests ────────────────────────────────────────────────────────────────────

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn crc16_known_vector() {
          // CRC16-CCITT of "123456789" = 0x29B1
          assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
      }

      #[test]
      fn encode_byte_gap_byte() {
          // 0x4E with prev_bit=0 encodes to [0x49, 0x2A]
          assert_eq!(encode_byte(0x4E, &mut 0), [0x49, 0x2A]);
      }

      #[test]
      fn encode_byte_sync_zero() {
          // 0x00 with prev_bit=0 encodes to [0x55, 0x55]
          assert_eq!(encode_byte(0x00, &mut 0), [0x55, 0x55]);
      }

      #[test]
      fn encode_byte_updates_prev_bit() {
          // 0x01 has last data bit = 1; prev_bit should be 1 afterwards
          let mut prev = 0u8;
          encode_byte(0x01, &mut prev);
          assert_eq!(prev, 1);
      }

      #[test]
      fn encode_a1_sync_is_correct() {
          assert_eq!(encode_a1_sync(), [0x22, 0x91]);
      }
  }
  ```

- [ ] **Step 2.2 — Add `pub mod hfe;` to `lib.rs` so it compiles**

  Append to `crates/sd1disk/src/lib.rs`:

  ```rust
  pub mod hfe;
  pub use hfe::{read_hfe, write_hfe};
  ```

- [ ] **Step 2.3 — Run the tests to confirm they fail**

  ```bash
  cargo test -p sd1disk crc16_known_vector encode_byte_gap_byte encode_byte_sync_zero encode_a1_sync_is_correct
  ```

  Expected: all **FAIL** (stubs return wrong values).

- [ ] **Step 2.4 — Implement `crc16_ccitt`**

  Replace the stub in `hfe.rs`:

  ```rust
  fn crc16_ccitt(data: &[u8]) -> u16 {
      let mut crc: u16 = 0xFFFF;
      for &byte in data {
          let mut x = (crc >> 8) ^ byte as u16;
          x ^= x >> 4;
          crc = (crc << 8) ^ (x << 12) ^ (x << 5) ^ x;
      }
      crc
  }
  ```

- [ ] **Step 2.5 — Implement `encode_byte`**

  Replace the stub:

  ```rust
  /// Encode one data byte to 2 MFM-encoded HFE bytes (LSB-first bit storage).
  ///
  /// For each data bit `d` (MSB first): clock `c = !(prev_bit | d)`.
  /// Emit (c, d) in time order; pack 8 time-bits per output byte, bit-0 = oldest.
  fn encode_byte(byte: u8, prev_bit: &mut u8) -> [u8; 2] {
      let mut out = [0u8; 2];
      for i in 0..8usize {
          let d = (byte >> (7 - i)) & 1;
          let c = if (*prev_bit == 0) && (d == 0) { 1u8 } else { 0u8 };
          let pos_c = 2 * i;
          let pos_d = 2 * i + 1;
          out[pos_c / 8] |= c << (pos_c % 8);
          out[pos_d / 8] |= d << (pos_d % 8);
          *prev_bit = d;
      }
      out
  }
  ```

- [ ] **Step 2.6 — Implement `encode_a1_sync`**

  Replace the stub:

  ```rust
  /// Emit the special A1* sync mark (0x4489, missing clock bit) as raw HFE bytes.
  /// Sets prev_bit = 1 after emission.
  fn encode_a1_sync() -> [u8; 2] {
      [0x22, 0x91]
  }
  ```

  *(Note: callers must set `prev_bit = 1` after emitting a sync mark.)*

- [ ] **Step 2.7 — Run the tests**

  ```bash
  cargo test -p sd1disk crc16_known_vector encode_byte_gap_byte encode_byte_sync_zero encode_byte_updates_prev_bit encode_a1_sync_is_correct
  ```

  Expected: all **PASS**

- [ ] **Step 2.8 — Commit**

  ```bash
  git add crates/sd1disk/src/hfe.rs crates/sd1disk/src/lib.rs
  git commit -m "feat(hfe): add crc16_ccitt, encode_byte, encode_a1_sync helpers"
  ```

---

## Task 3 — `encode_track_side`

**Files:**
- Modify: `crates/sd1disk/src/hfe.rs`

- [ ] **Step 3.1 — Write failing tests**

  Add to the `tests` module in `hfe.rs`:

  ```rust
  #[test]
  fn encode_track_side_is_12522_bytes() {
      let img = DiskImage::create();
      let data = encode_track_side(&img, 0, 0);
      assert_eq!(data.len(), 12522, "side must be exactly 12,522 MFM bytes");
  }

  #[test]
  fn encode_track_side_side1_is_12522_bytes() {
      let img = DiskImage::create();
      let data = encode_track_side(&img, 0, 1);
      assert_eq!(data.len(), 12522);
  }

  #[test]
  fn encode_track_side_last_track_is_12522_bytes() {
      let img = DiskImage::create();
      let data = encode_track_side(&img, 79, 1);
      assert_eq!(data.len(), 12522);
  }
  ```

- [ ] **Step 3.2 — Add stub**

  ```rust
  fn encode_track_side(_img: &DiskImage, _track: u8, _side: u8) -> Vec<u8> {
      vec![0u8; 12522]
  }
  ```

- [ ] **Step 3.3 — Run tests to confirm they pass the length check (they will)**

  ```bash
  cargo test -p sd1disk encode_track_side
  ```

  These pass with the stub. The real test will be the round-trip in Task 7.

- [ ] **Step 3.4 — Implement `encode_track_side`**

  Replace the stub:

  ```rust
  /// Encode one side of one track into exactly 12,522 MFM bytes.
  ///
  /// Layout (per side):
  ///   Gap4a(80×0x4E) + Sync(12×0x00) + Gap1(50×0x4E) +
  ///   ×10: [Sync(12×0x00) + A1*×3 + IDAM(7) + Gap2(22×0x4E) +
  ///          Sync(12×0x00) + A1*×3 + DAM(515) + Gap3(~37×0x4E)]
  ///
  /// Sectors 0-8 get 37 bytes of Gap3 (74 MFM bytes each).
  /// Sector 9 gets enough Gap3 bytes to reach exactly 12,522 total.
  fn encode_track_side(img: &DiskImage, track: u8, side: u8) -> Vec<u8> {
      let mut out: Vec<u8> = Vec::with_capacity(12522);
      let mut prev: u8 = 0;

      macro_rules! enc {
          ($b:expr) => {{
              let e = encode_byte($b, &mut prev);
              out.extend_from_slice(&e);
          }};
      }
      macro_rules! sync_mark {
          () => {{
              out.extend_from_slice(&encode_a1_sync());
              prev = 1;
          }};
      }

      // Gap 4a: 80 × 0x4E
      for _ in 0..80 { enc!(0x4E); }
      // Sync: 12 × 0x00
      for _ in 0..12 { enc!(0x00); }
      // Gap 1: 50 × 0x4E
      for _ in 0..50 { enc!(0x4E); }

      for sector in 0u8..10 {
          // Sync: 12 × 0x00
          for _ in 0..12 { enc!(0x00); }
          // 3 × A1* sync mark
          for _ in 0..3 { sync_mark!(); }

          // IDAM: 0xFE, track, side, sector, 0x02, CRC(2)
          let idam_crc_input = [0xA1u8, 0xA1, 0xA1, 0xFE, track, side, sector, 0x02];
          let idam_crc = crc16_ccitt(&idam_crc_input);
          for &b in &[0xFEu8, track, side, sector, 0x02,
                      (idam_crc >> 8) as u8, (idam_crc & 0xFF) as u8] {
              enc!(b);
          }

          // Gap 2: 22 × 0x4E
          for _ in 0..22 { enc!(0x4E); }
          // Sync: 12 × 0x00
          for _ in 0..12 { enc!(0x00); }
          // 3 × A1* sync mark
          for _ in 0..3 { sync_mark!(); }

          // DAM: 0xFB, sector data (512), CRC(2)
          let block = track as u16 * 20 + side as u16 * 10 + sector as u16;
          let sector_data = img.block(block).expect("block index always valid for 80-track disk");

          let mut dam_crc_input = vec![0xA1u8, 0xA1, 0xA1, 0xFB];
          dam_crc_input.extend_from_slice(sector_data);
          let dam_crc = crc16_ccitt(&dam_crc_input);

          enc!(0xFB);
          // Encode 512 data bytes — clone slice to avoid borrow conflict
          let sector_bytes: Vec<u8> = sector_data.to_vec();
          for &b in &sector_bytes { enc!(b); }
          enc!((dam_crc >> 8) as u8);
          enc!((dam_crc & 0xFF) as u8);

          // Gap 3: sectors 0-8 get 37 data bytes; sector 9 fills to 12,522
          if sector < 9 {
              for _ in 0..37 { enc!(0x4E); }
          } else {
              while out.len() < 12522 { enc!(0x4E); }
          }
      }

      debug_assert_eq!(out.len(), 12522);
      out
  }
  ```

- [ ] **Step 3.5 — Run tests**

  ```bash
  cargo test -p sd1disk encode_track_side
  ```

  Expected: all **PASS**

- [ ] **Step 3.6 — Commit**

  ```bash
  git add crates/sd1disk/src/hfe.rs
  git commit -m "feat(hfe): implement encode_track_side (MFM layout, 12,522 bytes per side)"
  ```

---

## Task 4 — MFM decode helpers

**Files:**
- Modify: `crates/sd1disk/src/hfe.rs`

- [ ] **Step 4.1 — Write failing tests**

  Add to the `tests` module:

  ```rust
  #[test]
  fn hfe_to_bits_lsb_first() {
      // 0x01 = 0b00000001; LSB first means bit 0 (=1) comes out as bits[0]
      let bits = hfe_to_bits(&[0x01]);
      assert_eq!(bits[0], 1);
      assert_eq!(&bits[1..8], &[0u8; 7]);
  }

  #[test]
  fn decode_mfm_byte_roundtrip() {
      // encode 0x4E with prev=0, then decode the resulting bits
      let encoded = encode_byte(0x4E, &mut 0);
      let bits = hfe_to_bits(&encoded);
      assert_eq!(decode_mfm_byte(&bits, 0), 0x4E);
  }

  #[test]
  fn find_sync_finds_a1_mark() {
      // encode_a1_sync produces [0x22, 0x91] which decodes to the A1* pattern
      let mark = encode_a1_sync();
      let bits = hfe_to_bits(&mark);
      let pos = find_sync(&bits, 0);
      assert_eq!(pos, Some(0), "A1* sync mark must be found at position 0");
  }

  #[test]
  fn is_a1_pattern_true_for_sync() {
      let mark = encode_a1_sync();
      let bits = hfe_to_bits(&mark);
      assert!(is_a1_pattern(&bits));
  }

  #[test]
  fn is_a1_pattern_false_for_gap() {
      let gap = encode_byte(0x4E, &mut 0);
      let bits = hfe_to_bits(&gap);
      assert!(!is_a1_pattern(&bits));
  }
  ```

- [ ] **Step 4.2 — Add stubs (they will fail the round-trip test)**

  ```rust
  fn hfe_to_bits(_raw: &[u8]) -> Vec<u8> { vec![] }
  fn decode_mfm_byte(_bits: &[u8], _offset: usize) -> u8 { 0 }
  fn find_sync(_bits: &[u8], _start: usize) -> Option<usize> { None }
  fn is_a1_pattern(_bits: &[u8]) -> bool { false }
  ```

- [ ] **Step 4.3 — Run to confirm failures**

  ```bash
  cargo test -p sd1disk hfe_to_bits_lsb_first decode_mfm_byte_roundtrip find_sync_finds_a1_mark is_a1_pattern
  ```

  Expected: all **FAIL**

- [ ] **Step 4.4 — Implement `hfe_to_bits`**

  ```rust
  /// Convert a slice of HFE bytes to a time-ordered bit array.
  /// Each byte is unpacked LSB-first: bit 0 is the oldest bit.
  fn hfe_to_bits(raw: &[u8]) -> Vec<u8> {
      let mut bits = Vec::with_capacity(raw.len() * 8);
      for &byte in raw {
          for bit in 0..8 {
              bits.push((byte >> bit) & 1);
          }
      }
      bits
  }
  ```

- [ ] **Step 4.5 — Implement `decode_mfm_byte`**

  ```rust
  /// Extract one data byte from 16 time-ordered MFM bits starting at `offset`.
  /// Data bits are at odd positions (1, 3, 5, 7, 9, 11, 13, 15).
  /// Position 1 = MSB of the decoded byte.
  fn decode_mfm_byte(bits: &[u8], offset: usize) -> u8 {
      let mut byte = 0u8;
      for i in 0..8 {
          byte = (byte << 1) | bits[offset + 2 * i + 1];
      }
      byte
  }
  ```

- [ ] **Step 4.6 — Implement `is_a1_pattern` and `find_sync`**

  ```rust
  /// Return true if the first 16 elements of `bits` match the A1* sync pattern.
  fn is_a1_pattern(bits: &[u8]) -> bool {
      const PATTERN: [u8; 16] = [0,1,0,0,0,1,0,0,1,0,0,0,1,0,0,1];
      bits.len() >= 16 && bits[..16] == PATTERN
  }

  /// Find the next A1* sync mark starting at `start`. Returns the bit offset.
  fn find_sync(bits: &[u8], start: usize) -> Option<usize> {
      for i in start..bits.len().saturating_sub(15) {
          if is_a1_pattern(&bits[i..]) {
              return Some(i);
          }
      }
      None
  }
  ```

- [ ] **Step 4.7 — Run tests**

  ```bash
  cargo test -p sd1disk hfe_to_bits_lsb_first decode_mfm_byte_roundtrip find_sync_finds_a1_mark is_a1_pattern_true_for_sync is_a1_pattern_false_for_gap
  ```

  Expected: all **PASS**

- [ ] **Step 4.8 — Commit**

  ```bash
  git add crates/sd1disk/src/hfe.rs
  git commit -m "feat(hfe): add hfe_to_bits, decode_mfm_byte, find_sync, is_a1_pattern"
  ```

---

## Task 5 — `extract_side` and `decode_track_side`

**Files:**
- Modify: `crates/sd1disk/src/hfe.rs`

- [ ] **Step 5.1 — Write failing tests**

  Add to the `tests` module:

  ```rust
  #[test]
  fn extract_side_produces_correct_length() {
      // Simulate a 25,044-byte interleaved track: 49 × 512 bytes.
      // After extraction, each side should be 12,522 bytes.
      let img = DiskImage::create();
      let s0 = encode_track_side(&img, 0, 0);
      let s1 = encode_track_side(&img, 0, 1);
      let interleaved = interleave_sides(&s0, &s1);
      assert_eq!(interleaved.len(), 49 * 512);

      let extracted_s0 = extract_side(&interleaved, 0, 25044);
      let extracted_s1 = extract_side(&interleaved, 1, 25044);
      assert_eq!(extracted_s0.len(), 12522);
      assert_eq!(extracted_s1.len(), 12522);
  }

  #[test]
  fn extract_side_roundtrips_data() {
      let img = DiskImage::create();
      let s0 = encode_track_side(&img, 0, 0);
      let s1 = encode_track_side(&img, 0, 1);
      let interleaved = interleave_sides(&s0, &s1);

      let extracted_s0 = extract_side(&interleaved, 0, 25044);
      let extracted_s1 = extract_side(&interleaved, 1, 25044);
      assert_eq!(extracted_s0, s0);
      assert_eq!(extracted_s1, s1);
  }

  #[test]
  fn decode_track_side_roundtrip() {
      let img = DiskImage::create();
      let encoded = encode_track_side(&img, 5, 1);
      let sectors = decode_track_side(&encoded, 5, 1).expect("decode must succeed");
      for (i, maybe) in sectors.iter().enumerate() {
          assert!(maybe.is_some(), "sector {} must be present", i);
          let block = 5u16 * 20 + 1 * 10 + i as u16;
          let expected = img.block(block).unwrap();
          assert_eq!(maybe.as_ref().unwrap(), expected,
              "sector {} data mismatch", i);
      }
  }
  ```

- [ ] **Step 5.2 — Add stubs**

  ```rust
  fn interleave_sides(_s0: &[u8], _s1: &[u8]) -> Vec<u8> { vec![0u8; 49 * 512] }
  fn extract_side(_raw: &[u8], _side: u8, _track_len: usize) -> Vec<u8> { vec![0u8; 12522] }
  fn decode_track_side(_raw: &[u8], _track: u8, _side: u8) -> Result<[Option<[u8; 512]>; 10]> {
      Ok([None; 10])
  }
  ```

- [ ] **Step 5.3 — Run to confirm failures**

  ```bash
  cargo test -p sd1disk extract_side decode_track_side
  ```

  Expected: `extract_side_roundtrips_data` and `decode_track_side_roundtrip` **FAIL**.

- [ ] **Step 5.4 — Implement `interleave_sides`**

  ```rust
  /// Interleave side-0 and side-1 data in 256-byte chunks.
  /// Input: each side is 12,522 bytes.
  /// Output: 49 × 512 = 25,088 bytes (last partial chunks padded with 0xFF).
  fn interleave_sides(s0: &[u8], s1: &[u8]) -> Vec<u8> {
      const CHUNK: usize = 256;
      const NUM_CHUNKS: usize = 49; // ceil(12522/256) = 49
      let mut out = Vec::with_capacity(NUM_CHUNKS * 512);
      for chunk in 0..NUM_CHUNKS {
          let start = chunk * CHUNK;
          // Side 0 chunk
          let s0_end = (start + CHUNK).min(s0.len());
          out.extend_from_slice(&s0[start..s0_end]);
          out.extend(std::iter::repeat(0xFF).take(CHUNK - (s0_end - start)));
          // Side 1 chunk
          let s1_end = (start + CHUNK).min(s1.len());
          out.extend_from_slice(&s1[start..s1_end]);
          out.extend(std::iter::repeat(0xFF).take(CHUNK - (s1_end - start)));
      }
      out
  }
  ```

- [ ] **Step 5.5 — Implement `extract_side`**

  ```rust
  /// Un-interleave 256-byte chunks to extract the bitstream for one side.
  ///
  /// In each 512-byte HFE block: bytes 0–255 = side 0, bytes 256–511 = side 1.
  /// Only returns bytes up to `track_len` (the actual track byte count from the TLT).
  fn extract_side(raw: &[u8], side: u8, track_len: usize) -> Vec<u8> {
      const CHUNK: usize = 256;
      let raw = &raw[..track_len.min(raw.len())];
      let mut out = Vec::new();
      let base = if side == 0 { 0 } else { CHUNK };
      let mut offset = base;
      while offset + CHUNK <= raw.len() {
          out.extend_from_slice(&raw[offset..offset + CHUNK]);
          offset += 512;
      }
      // Handle final partial chunk
      if offset < raw.len() {
          let remaining = raw.len() - offset;
          if remaining > 0 {
              let copy = remaining.min(CHUNK);
              out.extend_from_slice(&raw[offset..offset + copy]);
          }
      }
      out
  }
  ```

- [ ] **Step 5.6 — Implement `decode_track_side`**

  ```rust
  /// Decode all 10 sectors from one side's 12,522-byte MFM bitstream.
  ///
  /// Returns an array of 10 `Option<[u8; 512]>`. Each `None` means that sector
  /// was not found. The caller turns `None` into `HfeMissingSector`.
  fn decode_track_side(raw: &[u8], track_ctx: u8, side_ctx: u8)
      -> Result<[Option<[u8; 512]>; 10]>
  {
      let bits = hfe_to_bits(raw);
      let mut sectors: [Option<[u8; 512]>; 10] = [None; 10];
      let mut current_sector: Option<u8> = None;
      let mut pos = 0;

      loop {
          let sync_pos = match find_sync(&bits, pos) {
              Some(p) => p,
              None => break,
          };

          // Count consecutive A1* marks
          let mut after = sync_pos;
          let mut count = 0;
          while after + 16 <= bits.len() && is_a1_pattern(&bits[after..]) {
              count += 1;
              after += 16;
          }

          if count < 3 || after + 16 > bits.len() {
              pos = sync_pos + 1;
              continue;
          }

          let marker = decode_mfm_byte(&bits, after);
          after += 16;

          match marker {
              0xFE => {
                  // IDAM: track(1) side(1) sector(1) size_code(1) crc_hi(1) crc_lo(1)
                  if after + 6 * 16 > bits.len() {
                      pos = sync_pos + 1;
                      continue;
                  }
                  let idam_track  = decode_mfm_byte(&bits, after); after += 16;
                  let idam_side   = decode_mfm_byte(&bits, after); after += 16;
                  let idam_sector = decode_mfm_byte(&bits, after); after += 16;
                  let _size_code  = decode_mfm_byte(&bits, after); after += 16;
                  let crc_hi      = decode_mfm_byte(&bits, after); after += 16;
                  let crc_lo      = decode_mfm_byte(&bits, after); after += 16;

                  let actual_crc   = ((crc_hi as u16) << 8) | crc_lo as u16;
                  let expected_crc = crc16_ccitt(&[
                      0xA1, 0xA1, 0xA1, 0xFE,
                      idam_track, idam_side, idam_sector, 0x02,
                  ]);
                  if actual_crc != expected_crc {
                      return Err(Error::HfeCrcMismatch {
                          track: idam_track,
                          side:  idam_side,
                          sector: idam_sector,
                      });
                  }
                  if idam_sector < 10 {
                      current_sector = Some(idam_sector);
                  }
                  pos = after;
              }

              0xFB => {
                  // DAM: data[512] + crc(2)
                  if after + 514 * 16 > bits.len() {
                      pos = sync_pos + 1;
                      continue;
                  }
                  let mut data = [0u8; 512];
                  for byte in &mut data {
                      *byte = decode_mfm_byte(&bits, after);
                      after += 16;
                  }
                  let crc_hi = decode_mfm_byte(&bits, after); after += 16;
                  let crc_lo = decode_mfm_byte(&bits, after); after += 16;

                  let actual_crc = ((crc_hi as u16) << 8) | crc_lo as u16;
                  let mut crc_input = vec![0xA1u8, 0xA1, 0xA1, 0xFB];
                  crc_input.extend_from_slice(&data);
                  let expected_crc = crc16_ccitt(&crc_input);

                  if actual_crc != expected_crc {
                      let sec = current_sector.unwrap_or(0xFF);
                      return Err(Error::HfeCrcMismatch {
                          track: track_ctx,
                          side:  side_ctx,
                          sector: sec,
                      });
                  }

                  if let Some(sec) = current_sector.take() {
                      if (sec as usize) < 10 {
                          sectors[sec as usize] = Some(data);
                      }
                  }
                  pos = after;
              }

              _ => {
                  pos = sync_pos + 1;
              }
          }
      }

      Ok(sectors)
  }
  ```

- [ ] **Step 5.7 — Run tests**

  ```bash
  cargo test -p sd1disk extract_side decode_track_side
  ```

  Expected: all **PASS**

- [ ] **Step 5.8 — Commit**

  ```bash
  git add crates/sd1disk/src/hfe.rs
  git commit -m "feat(hfe): implement interleave_sides, extract_side, decode_track_side"
  ```

---

## Task 6 — `write_hfe` and `read_hfe`

**Files:**
- Modify: `crates/sd1disk/src/hfe.rs`

- [ ] **Step 6.1 — Write a write-then-verify test**

  Add to the `tests` module:

  ```rust
  #[test]
  fn write_hfe_produces_correct_file_size() {
      let img = DiskImage::create();
      let path = std::env::temp_dir().join("sd1_hfe_size_test.hfe");
      write_hfe(&img, &path).unwrap();
      let meta = std::fs::metadata(&path).unwrap();
      // 2 blocks header/TLT + 80 tracks × 49 blocks × 512 bytes
      let expected = (2 + 80 * 49) * 512;
      assert_eq!(meta.len() as usize, expected,
          "HFE file size must be {} bytes", expected);
      std::fs::remove_file(&path).ok();
  }
  ```

- [ ] **Step 6.2 — Run to confirm it fails**

  ```bash
  cargo test -p sd1disk write_hfe_produces_correct_file_size
  ```

  Expected: **FAIL** (`write_hfe` still returns an error).

- [ ] **Step 6.3 — Implement `write_hfe`**

  Replace the stub:

  ```rust
  /// Write a DiskImage to an HFE v1 file.
  ///
  /// File layout:
  ///   Block 0 (512 B): HFE header
  ///   Block 1 (512 B): Track Lookup Table (TLT), 80 × 4 bytes
  ///   Blocks 2+:       Track data, 49 blocks (25,088 B) per track
  ///
  /// Written atomically via a `.hfe.tmp` rename.
  pub fn write_hfe(image: &DiskImage, path: &Path) -> Result<()> {
      const TRACK_BLOCKS: usize = 49;
      const TRACK_BYTES_STORED: u16 = 25044; // actual content; rest is 0xFF padding

      // ── Header (512 bytes, unused fields padded to 0xFF) ──────────────────
      let mut header = [0xFFu8; 512];
      header[0..8].copy_from_slice(b"HXCPICFE");
      header[8]  = 0;    // format revision
      header[9]  = 80;   // number of tracks
      header[10] = 2;    // number of sides
      header[11] = 0;    // ISOIBM_MFM
      header[12..14].copy_from_slice(&250u16.to_le_bytes()); // bit rate kbps
      header[14..16].copy_from_slice(&0u16.to_le_bytes());   // RPM (unspecified)
      header[16] = 7;    // GENERIC_SHUGART_DD
      header[17] = 0xFF; // dnu
      header[18..20].copy_from_slice(&1u16.to_le_bytes());   // TLT at block 1
      header[20] = 0xFF; // write_allowed
      header[21] = 0xFF; // single_step
      header[22] = 0xFF; header[23] = 0xFF; // track0s0 alt/enc
      header[24] = 0xFF; header[25] = 0xFF; // track0s1 alt/enc

      // ── Track Lookup Table (512 bytes) ────────────────────────────────────
      let mut tlt = [0xFFu8; 512];
      for track in 0u8..80 {
          let block_offset: u16 = 2 + track as u16 * TRACK_BLOCKS as u16;
          let entry = track as usize * 4;
          tlt[entry..entry + 2].copy_from_slice(&block_offset.to_le_bytes());
          tlt[entry + 2..entry + 4].copy_from_slice(&TRACK_BYTES_STORED.to_le_bytes());
      }

      // ── Track data ────────────────────────────────────────────────────────
      let mut track_blocks: Vec<u8> = Vec::with_capacity(80 * TRACK_BLOCKS * 512);
      for track in 0u8..80 {
          let s0 = encode_track_side(image, track, 0);
          let s1 = encode_track_side(image, track, 1);
          let mut interleaved = interleave_sides(&s0, &s1);
          // interleaved is 49×512 = 25,088 bytes already
          debug_assert_eq!(interleaved.len(), TRACK_BLOCKS * 512);
          track_blocks.append(&mut interleaved);
      }

      // ── Assemble and atomic-write ─────────────────────────────────────────
      let mut file_data = Vec::with_capacity(header.len() + tlt.len() + track_blocks.len());
      file_data.extend_from_slice(&header);
      file_data.extend_from_slice(&tlt);
      file_data.extend_from_slice(&track_blocks);

      let tmp = path.with_extension("hfe.tmp");
      std::fs::write(&tmp, &file_data)?;
      std::fs::rename(&tmp, path)?;
      Ok(())
  }
  ```

- [ ] **Step 6.4 — Run write test**

  ```bash
  cargo test -p sd1disk write_hfe_produces_correct_file_size
  ```

  Expected: **PASS**

- [ ] **Step 6.5 — Write a read-back test**

  Add to the `tests` module:

  ```rust
  #[test]
  fn read_hfe_reads_written_file() {
      let img = DiskImage::create();
      let path = std::env::temp_dir().join("sd1_hfe_read_test.hfe");
      write_hfe(&img, &path).unwrap();
      let decoded = read_hfe(&path).expect("read_hfe must succeed on a freshly written HFE");
      assert_eq!(decoded.data.len(), 1600 * 512);
      std::fs::remove_file(&path).ok();
  }
  ```

- [ ] **Step 6.6 — Run to confirm it fails**

  ```bash
  cargo test -p sd1disk read_hfe_reads_written_file
  ```

  Expected: **FAIL** (`read_hfe` still returns an error).

- [ ] **Step 6.7 — Implement `read_hfe`**

  Replace the stub:

  ```rust
  /// Read an HFE v1 file and return a DiskImage.
  ///
  /// Fails fast on bad signature or unsupported revision.
  /// Returns `HfeCrcMismatch` or `HfeMissingSector` on per-sector problems.
  pub fn read_hfe(path: &Path) -> Result<DiskImage> {
      let data = std::fs::read(path)?;

      // ── Validate header ───────────────────────────────────────────────────
      if data.len() < 512 {
          return Err(Error::InvalidHfe("file too short for HFE header"));
      }
      if &data[0..8] != b"HXCPICFE" {
          return Err(Error::InvalidHfe("missing HXCPICFE signature"));
      }
      if data[8] != 0 {
          return Err(Error::InvalidHfe("unsupported HFE format revision (only v0 supported)"));
      }

      let num_tracks        = data[9] as usize;
      let track_list_block  = u16::from_le_bytes([data[18], data[19]]) as usize;
      let tlt_offset        = track_list_block * 512;

      if tlt_offset + num_tracks * 4 > data.len() {
          return Err(Error::InvalidHfe("track lookup table truncated"));
      }

      // ── Decode all tracks ─────────────────────────────────────────────────
      let mut img = DiskImage::create();

      for track in 0..num_tracks {
          let entry        = tlt_offset + track * 4;
          let block_offset = u16::from_le_bytes([data[entry], data[entry + 1]]) as usize;
          let byte_length  = u16::from_le_bytes([data[entry + 2], data[entry + 3]]) as usize;
          let track_start  = block_offset * 512;

          if track_start + byte_length > data.len() {
              return Err(Error::InvalidHfe("track data truncated"));
          }
          let track_raw = &data[track_start..track_start + byte_length];

          for side in 0..2u8 {
              let side_data = extract_side(track_raw, side, byte_length);
              let sectors   = decode_track_side(&side_data, track as u8, side)?;

              for (sector_idx, maybe_data) in sectors.iter().enumerate() {
                  let block = track as u16 * 20 + side as u16 * 10 + sector_idx as u16;
                  match maybe_data {
                      Some(sector_bytes) => {
                          img.block_mut(block)?.copy_from_slice(sector_bytes);
                      }
                      None => {
                          return Err(Error::HfeMissingSector {
                              track:  track as u8,
                              side,
                              sector: sector_idx as u8,
                          });
                      }
                  }
              }
          }
      }

      Ok(img)
  }
  ```

- [ ] **Step 6.8 — Run tests**

  ```bash
  cargo test -p sd1disk write_hfe_produces_correct_file_size read_hfe_reads_written_file
  ```

  Expected: both **PASS**

- [ ] **Step 6.9 — Commit**

  ```bash
  git add crates/sd1disk/src/hfe.rs
  git commit -m "feat(hfe): implement write_hfe and read_hfe"
  ```

---

## Task 7 — Round-trip and spec tests

**Files:**
- Modify: `crates/sd1disk/src/hfe.rs`

- [ ] **Step 7.1 — Add all 5 spec tests**

  Add to the `tests` module:

  ```rust
  #[test]
  fn round_trip_blank_image() {
      let original = DiskImage::create();
      let path = std::env::temp_dir().join("sd1_rt_blank.hfe");
      write_hfe(&original, &path).unwrap();
      let decoded = read_hfe(&path).unwrap();
      assert_eq!(original.data, decoded.data,
          "round-tripped blank image must be byte-for-byte identical");
      std::fs::remove_file(&path).ok();
  }

  #[test]
  fn crc_mismatch_returns_error() {
      let img = DiskImage::create();
      let path = std::env::temp_dir().join("sd1_rt_crc.hfe");
      write_hfe(&img, &path).unwrap();

      // Corrupt the IDAM CRC-high byte for track 0, side 0, sector 0.
      // Layout: header(512) + TLT(512) + track0_data.
      // Track 0 side-0 byte 324 = IDAM CRC-high.
      // Byte 324 is in interleaved chunk 1 (bytes 256–511), offset 324-256=68.
      // File offset: 1024 + 512*1 + 68 = 1604.
      let mut hfe_data = std::fs::read(&path).unwrap();
      hfe_data[1604] ^= 0xFF;
      std::fs::write(&path, &hfe_data).unwrap();

      let result = read_hfe(&path);
      assert!(
          matches!(result, Err(Error::HfeCrcMismatch { .. })),
          "expected HfeCrcMismatch, got {:?}", result
      );
      std::fs::remove_file(&path).ok();
  }

  #[test]
  fn block_sentinel_survives_round_trip() {
      let mut img = DiskImage::create();
      // Write a sentinel pattern to block 42
      let sentinel = [0xDE, 0xAD, 0xBE, 0xEF];
      img.block_mut(42).unwrap()[0..4].copy_from_slice(&sentinel);

      let path = std::env::temp_dir().join("sd1_rt_sentinel.hfe");
      write_hfe(&img, &path).unwrap();
      let decoded = read_hfe(&path).unwrap();

      assert_eq!(&decoded.block(42).unwrap()[0..4], &sentinel,
          "sentinel at block 42 must survive the round-trip");
      std::fs::remove_file(&path).ok();
  }

  #[test]
  fn reserved_blocks_survive_round_trip() {
      let img = DiskImage::create();
      let path = std::env::temp_dir().join("sd1_rt_reserved.hfe");
      write_hfe(&img, &path).unwrap();
      let decoded = read_hfe(&path).unwrap();

      // Blocks 0–22 are OS-reserved. Verify all match the original.
      for b in 0u16..23 {
          assert_eq!(
              img.block(b).unwrap(),
              decoded.block(b).unwrap(),
              "reserved block {} must survive the round-trip", b
          );
      }
      std::fs::remove_file(&path).ok();
  }

  #[test]
  fn sector_numbering_is_zero_based() {
      let img = DiskImage::create();
      let side_data = encode_track_side(&img, 0, 0);
      let bits = hfe_to_bits(&side_data);

      let mut sectors_seen = std::collections::HashSet::new();
      let mut pos = 0;

      loop {
          let Some(sync_pos) = find_sync(&bits, pos) else { break };
          let mut after = sync_pos;
          let mut count = 0;
          while after + 16 <= bits.len() && is_a1_pattern(&bits[after..]) {
              count += 1;
              after += 16;
          }
          if count >= 3 && after + 5 * 16 <= bits.len() {
              let marker = decode_mfm_byte(&bits, after); after += 16;
              if marker == 0xFE {
                  let _t  = decode_mfm_byte(&bits, after); after += 16;
                  let _s  = decode_mfm_byte(&bits, after); after += 16;
                  let sec = decode_mfm_byte(&bits, after);
                  assert!(sec < 10, "sector must be 0–9, got {}", sec);
                  sectors_seen.insert(sec);
              }
          }
          pos = sync_pos + 16;
      }

      assert_eq!(sectors_seen.len(), 10, "all 10 sectors (0–9) must appear");
      for s in 0u8..10 {
          assert!(sectors_seen.contains(&s), "sector {} missing", s);
      }
  }
  ```

- [ ] **Step 7.2 — Run all spec tests**

  ```bash
  cargo test -p sd1disk round_trip_blank_image crc_mismatch_returns_error block_sentinel_survives_round_trip reserved_blocks_survive_round_trip sector_numbering_is_zero_based
  ```

  Expected: all **PASS**

- [ ] **Step 7.3 — Run full test suite to check no regressions**

  ```bash
  cargo test -p sd1disk
  ```

  Expected: all existing tests **PASS**, all new tests **PASS**.

- [ ] **Step 7.4 — Commit**

  ```bash
  git add crates/sd1disk/src/hfe.rs
  git commit -m "test(hfe): add 5 spec round-trip tests (blank, sentinel, reserved, CRC, sector numbering)"
  ```

---

## Task 8 — CLI subcommands and `lib.rs` export

**Files:**
- Modify: `crates/sd1disk/src/lib.rs`
- Modify: `crates/sd1cli/src/main.rs`

- [ ] **Step 8.1 — Verify `lib.rs` already has the export (added in Task 2)**

  ```bash
  grep 'pub mod hfe\|pub use hfe' crates/sd1disk/src/lib.rs
  ```

  Expected output:
  ```
  pub mod hfe;
  pub use hfe::{read_hfe, write_hfe};
  ```

  If missing, add them now.

- [ ] **Step 8.2 — Add the two `Command` variants to `main.rs`**

  In the `Command` enum, after `InspectSysex { ... }`, add:

  ```rust
  /// Convert an HFE floppy image to a flat .img file
  #[command(name = "hfe-to-img", long_about = "\
  Convert an HFE v1 floppy disk image to a flat SD-1 .img file.

  HFE files are produced by the HxC floppy emulator and the Sojus VST3 plugin.
  Unlike .img files, HFE files are not affected by the Sojus MAME off-by-one
  sector bug, making this the recommended import path for Sojus-origin disks.

  Use 'sd1cli list' on the output .img to verify the converted disk.")]
  HfeToImg {
      /// Path to the input HFE file
      hfe: PathBuf,
      /// Path to write the output .img file
      img: PathBuf,
  },

  /// Convert a flat .img file to HFE v1 format
  #[command(name = "img-to-hfe", long_about = "\
  Convert a flat SD-1 .img file to an HFE v1 floppy disk image.

  HFE files can be loaded into the Sojus VST3 plugin or written to a real floppy
  via the HxC floppy emulator. The output HFE file is fully MFM-encoded with
  correct CRC16-CCITT checksums on every sector.")]
  ImgToHfe {
      /// Path to the input .img file
      img: PathBuf,
      /// Path to write the output HFE file
      hfe: PathBuf,
  },
  ```

- [ ] **Step 8.3 — Add match arms to `run()`**

  In the `run()` function match block, add:

  ```rust
  Command::HfeToImg { hfe, img } => cmd_hfe_to_img(&hfe, &img),
  Command::ImgToHfe { img, hfe } => cmd_img_to_hfe(&img, &hfe),
  ```

- [ ] **Step 8.4 — Add the two handler functions**

  After `cmd_inspect_sysex` (or at end of file before `main`):

  ```rust
  fn cmd_hfe_to_img(hfe_path: &Path, img_path: &Path) -> sd1disk::Result<()> {
      let image = sd1disk::read_hfe(hfe_path)?;
      image.save(img_path)?;
      println!("Converted {} → {}", hfe_path.display(), img_path.display());
      Ok(())
  }

  fn cmd_img_to_hfe(img_path: &Path, hfe_path: &Path) -> sd1disk::Result<()> {
      let image = DiskImage::open(img_path)?;
      sd1disk::write_hfe(&image, hfe_path)?;
      println!("Converted {} → {}", img_path.display(), hfe_path.display());
      Ok(())
  }
  ```

- [ ] **Step 8.5 — Build and smoke-test the CLI**

  ```bash
  cargo build -p sd1cli 2>&1
  ```

  Expected: compiles cleanly.

  ```bash
  cargo run -p sd1cli -- hfe-to-img --help
  cargo run -p sd1cli -- img-to-hfe --help
  ```

  Expected: help text prints without error.

- [ ] **Step 8.6 — Commit**

  ```bash
  git add crates/sd1disk/src/lib.rs crates/sd1cli/src/main.rs
  git commit -m "feat(cli): add hfe-to-img and img-to-hfe subcommands"
  ```

---

## Task 9 — Integration test against `Ensoniq.hfe`

**Files:**
- No source changes; this is a manual validation step.

The real hardware-written `Ensoniq.hfe` file is at `/Users/joemcmahon/Downloads/Ensoniq.hfe`. It contains OMNIVERSE, SOPRANO-SAX, and 60-PRG-FILE.

- [ ] **Step 9.1 — Convert HFE to img**

  ```bash
  cargo run -p sd1cli -- hfe-to-img /Users/joemcmahon/Downloads/Ensoniq.hfe /tmp/verify.img
  ```

  Expected: `Converted /Users/joemcmahon/Downloads/Ensoniq.hfe → /tmp/verify.img`

- [ ] **Step 9.2 — List the converted image**

  ```bash
  cargo run -p sd1cli -- list /tmp/verify.img
  ```

  Expected output (order may vary):

  ```
  NAME         TYPE                   BLOCKS  BYTES  SLOT
  --------------------------------------------------------
  OMNIVERSE    ...                    ...     ...    ...
  SOPRANO-SAX  ...                    ...     ...    ...
  60-PRG-FILE  SixtySequences         ...     ...    ...
  ```

  All 3 files must be present. The free block count must be 1510.

- [ ] **Step 9.3 — Round-trip back to HFE**

  ```bash
  cargo run -p sd1cli -- img-to-hfe /tmp/verify.img /tmp/verify_rt.hfe
  cargo run -p sd1cli -- hfe-to-img /tmp/verify_rt.hfe /tmp/verify_rt.img
  cargo run -p sd1cli -- list /tmp/verify_rt.img
  ```

  Expected: same 3 files, same free block count.

- [ ] **Step 9.4 — Final commit**

  If integration test passes without code changes:

  ```bash
  git tag hfe-integration-verified
  ```

  If any fixes were needed, commit them first with a descriptive message, then tag.

---

## Summary of commits expected

| Task | Message |
|---|---|
| 1 | `feat(hfe): add InvalidHfe, HfeCrcMismatch, HfeMissingSector error variants` |
| 2 | `feat(hfe): add crc16_ccitt, encode_byte, encode_a1_sync helpers` |
| 3 | `feat(hfe): implement encode_track_side (MFM layout, 12,522 bytes per side)` |
| 4 | `feat(hfe): add hfe_to_bits, decode_mfm_byte, find_sync, is_a1_pattern` |
| 5 | `feat(hfe): implement interleave_sides, extract_side, decode_track_side` |
| 6 | `feat(hfe): implement write_hfe and read_hfe` |
| 7 | `test(hfe): add 5 spec round-trip tests` |
| 8 | `feat(cli): add hfe-to-img and img-to-hfe subcommands` |
