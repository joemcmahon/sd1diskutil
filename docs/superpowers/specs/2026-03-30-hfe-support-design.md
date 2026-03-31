# HFE File Read/Write Support

**Date:** 2026-03-30
**Status:** Approved
**Scope:** `crates/sd1disk` + `crates/sd1cli`

## Overview

Add HFE v1 file support to sd1diskutil: read an HFE into a `DiskImage` and write a `DiskImage` out as HFE. HFE is a raw MFM flux image format used by the HxC floppy emulator and the Sojus VST3 plugin. It is unaffected by the Sojus MAME off-by-one sector bug that corrupts `.img` files.

## Architecture

Single new module: `crates/sd1disk/src/hfe.rs`. No new crates. MFM encode/decode and CRC live as private functions inside this module — they are not reused elsewhere and do not warrant a separate file.

Exported from `sd1disk::lib.rs`:

```rust
pub fn read_hfe(path: &Path) -> Result<DiskImage>
pub fn write_hfe(image: &DiskImage, path: &Path) -> Result<()>
```

Two new subcommands in `sd1cli`:

```
sd1cli hfe-to-img <input.hfe> <output.img>
sd1cli img-to-hfe <input.img> <output.hfe>
```

## Data Flow

### read_hfe

1. Read file; verify 8-byte signature `HXCPICFE` and format revision `0`.
2. Parse 512-byte header block (track count, side count, track list block offset).
3. Parse track lookup table (4 bytes per track: `u16` block offset + `u16` byte length).
4. For each track (0–79), for each side (0–1):
   - Un-interleave 256-byte chunks to extract raw side bitstream.
   - Scan for A1* sync marks (`[0x22, 0x91]` in LSB-first HFE storage).
   - Decode IDAM (track, side, sector, size) and DAM (512 bytes data) for each of 10 sectors.
   - Verify CRC16-CCITT for each IDAM and DAM; return `HfeCrcMismatch` on failure.
5. Map each decoded sector to its flat block: `block = track × 20 + side × 10 + sector`.
6. Assemble and return `DiskImage` (819,200 bytes).

### write_hfe

1. Create `DiskImage` → HFE header block (all constants hardcoded, see below).
2. For each track (0–79), for each side (0–1):
   - Encode 10 sectors as MFM into exactly 12,522 raw bytes (see track geometry).
3. Interleave side 0 and side 1 in 256-byte chunks → 25,044 bytes per track.
4. Build track lookup table (block offset + length per track).
5. Atomic write: `.hfe.tmp` then rename.

## Format Constants

### HFE Header (write)

| Field | Value |
|---|---|
| Signature | `HXCPICFE` |
| Format revision | 0 |
| Num tracks | 80 |
| Num sides | 2 |
| Track encoding | 0 (ISOIBM_MFM) |
| Bit rate | 250 kbps |
| RPM | 0 (unspecified) |
| Interface mode | 7 (GENERIC_SHUGART_DD) |
| Track list block | 1 (offset 0x200) |
| Write allowed | 0xFF |
| Single step | 0xFF |
| Track0 alt encoding fields | 0xFF |

### Track Geometry

| Parameter | Value |
|---|---|
| Track data length | 25,044 bytes (both sides) |
| Track block stride | 49 blocks (49 × 512; last 44 unused) |
| Side length | 12,522 bytes |
| Sectors per track | 10 |
| Sector numbering | 0–9 (Ensoniq, not PC 1–10) |
| Sector size code | 2 (= 512 bytes) |

### MFM Write Layout (per side, padded to 12,522 bytes)

```
Gap 4a:   80 × 0x4E
Sync:     12 × 0x00
Gap 1:    50 × 0x4E
[No index mark — Ensoniq disks do not use it]

× 10 sectors:
  Sync:   12 × 0x00
  3× A1* sync mark  [special: emit [0x22, 0x91] directly, not via encode_byte]
  IDAM:   0xFE + track + side + sector + 0x02 + CRC16(2 bytes)
  Gap 2:  22 × 0x4E
  Sync:   12 × 0x00
  3× A1* sync mark
  DAM:    0xFB + data[512] + CRC16(2 bytes)
  Gap 3:  ~75 × 0x4E  [adjusted so total = 12,522 bytes exactly]
```

### CRC16-CCITT

Polynomial 0x1021, initial value 0xFFFF.

- **IDAM CRC** covers: `[0xA1, 0xA1, 0xA1, 0xFE, track, side, sector, 0x02]`
- **DAM CRC** covers: `[0xA1, 0xA1, 0xA1, 0xFB, data[0], …, data[511]]`

### MFM Encoding

Each data byte encodes to 2 HFE bytes (LSB-first bit storage):

```rust
fn encode_byte(byte: u8, prev_bit: &mut u8) -> [u8; 2]
```

Rule: for each data bit `d` (MSB first), clock bit `c = !(prev | d)`. Emit `(c, d)` in
time order, pack 8 time-bits per HFE byte with bit-0 = oldest bit.

A1* sync mark is a special 16-bit pattern (`0x4489` in MSB-first notation) with a missing
clock bit; it cannot be produced by `encode_byte`. Emit as `[0x22, 0x91]` directly and
set `prev_bit = 1` afterwards.

### Block ↔ Sector Mapping

```
block  = track × 20 + side × 10 + sector
track  = block / 20
side   = (block / 10) % 2
sector = block % 10
```

Verified against `Ensoniq.hfe` (real hardware-written disk, confirmed 2026-03-30).

## Error Handling

New `Error` variants in `sd1disk::error`:

```rust
InvalidHfe(&'static str)
HfeCrcMismatch { track: u8, side: u8, sector: u8 }
HfeMissingSector { track: u8, side: u8, sector: u8 }
```

- `read_hfe`: fail fast on bad signature or unsupported revision; fail per-sector on CRC
  mismatch or missing sector (no silent data corruption).
- `write_hfe`: atomic write (`.hfe.tmp` → rename), same as `DiskImage::save`.

## Testing

Unit tests inside `hfe.rs`:

| Test | What it verifies |
|---|---|
| `round_trip_blank_image` | `write_hfe` → `read_hfe` on blank DiskImage; byte-for-byte match |
| `crc_mismatch_returns_error` | Corrupt one byte in written HFE; `read_hfe` returns `HfeCrcMismatch` |
| `block_sentinel_survives_round_trip` | Sentinel at block 42 survives write → read intact |
| `reserved_blocks_survive_round_trip` | OS data (blocks 0–22) survives HFE round-trip |
| `sector_numbering_is_zero_based` | IDAM sector field is 0–9, not 1–10 |

Integration test (in `sd1cli` or as a doc-test):

- `read_hfe("Ensoniq.hfe")` → run list logic → verify OMNIVERSE, SOPRANO-SAX,
  60-PRG-FILE present with correct types and sizes.

## Files Changed

| File | Change |
|---|---|
| `crates/sd1disk/src/hfe.rs` | New — all HFE logic |
| `crates/sd1disk/src/error.rs` | Add 3 new error variants |
| `crates/sd1disk/src/lib.rs` | `pub mod hfe; pub use hfe::{read_hfe, write_hfe};` |
| `crates/sd1cli/src/main.rs` | Add `HfeToImg` and `ImgToHfe` subcommands |

## Out of Scope

- HFE v2/v3 support (different container format; not needed for Ensoniq SD-1)
- Transparent auto-detection of `.hfe` vs `.img` in existing commands
- Repair of Sojus-corrupted `.img` files — **not feasible**. The Sojus VST3 MAME bug
  silently discards sector 0 of every track on write; that data is unrecoverable. The
  correct mitigation is to use `.hfe` output from Sojus (unaffected by the bug) or to
  use `sd1diskutil img-to-hfe` + Sojus's fixed `esq16_dsk.cpp` once released.
