---
date: 2026-03-30T18:11:06-05:00
session_name: general
researcher: Claude
git_commit: a607767
branch: main
repository: sd1diskutil
topic: "HFE Read/Write Support — Design & Spec"
tags: [design, hfe, mfm, spec, brainstorming]
status: complete
last_updated: 2026-03-30
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: HFE read/write design spec completed, ready for implementation

## Task(s)

1. **Explore HFE file format with real disk — COMPLETED**
   Decoded `Ensoniq.hfe` (real hardware-written SD-1 disk) using a Python MFM decoder
   written during this session. Verified:
   - All 1600 sectors decode without error
   - FAT chains for all 3 files are clean (OMNIVERSE, SOPRANO-SAX, 60-PRG-FILE)
   - FAT free count (1510) matches OS block free count (1510)
   - Block↔sector mapping: `block = track×20 + side×10 + sector`

2. **Design HFE read/write support — COMPLETED**
   Full spec approved and committed. Implementation plan not yet written.

## Critical References

- `docs/superpowers/specs/2026-03-30-hfe-support-design.md` — approved design spec, start here
- `crates/sd1disk/src/image.rs` — `DiskImage` struct; HFE module must produce/consume this
- `crates/sd1disk/src/error.rs` — add 3 new error variants here

## Recent changes

- `docs/superpowers/specs/2026-03-30-hfe-support-design.md` — new, committed at a607767

No Rust code changed this session. All exploration was in Python (not committed).

## Learnings

### HFE format (v1)

- Magic: `HXCPICFE`, revision byte 0, 512-byte header block
- Track lookup table at block 1 (0x200): 4 bytes/track = `u16` block-offset + `u16` byte-length
- Track data: side 0 and side 1 interleaved in 256-byte chunks within each track's storage
- Ensoniq disk: 80 tracks, 2 sides, 10 sectors/track, sectors **0–9** (not 1–10 like PC)
- Track data length: 25,044 bytes both sides = 12,522 bytes/side/track

### MFM encoding in HFE

- HFE stores bits **LSB-first** per byte (bit 0 = first bit encountered by read head)
- A1* sync mark (`0x4489` MSB-first) stored as `[0x22, 0x91]` in HFE
- Gap byte `0x4E` encodes as `[0x49, 0x2A]` in HFE
- Sync byte `0x00` (with prev=0) encodes as `[0x55, 0x55]` in HFE
- Data bits are at **odd positions** (1,3,5,7,9,11,13,15) in the 16-bit MFM word (time-ordered)
- CRC is CRC16-CCITT: poly 0x1021, init 0xFFFF; covers A1×3 + marker + payload

### Block mapping (confirmed against blank_image.img)

```
block = track×20 + side×10 + sector
track  = block / 20
side   = (block / 10) % 2
sector = block % 10
```

Block 0 = track 0, side 0, sector 0 = `6db6` fill ✓ matches `blank_image.img`

### Sojus VST3 MAME bug (from prior session, confirmed this session)

- Corrupted `.img` files from Sojus VST3 are **not repairable** — sector 0 of every track
  is discarded; data is gone. This is noted in the spec as out-of-scope / not feasible.
- `.hfe` files from Sojus are clean (bypass MAME sector extraction).

### Python reference implementation

A complete working Python decoder was written during this session but **not committed**.
It lives in session context only. The Rust implementation should reproduce its logic:
- `extract_side()`: un-interleave 256-byte chunks
- `hfe_to_bits()`: LSB-first byte → time-ordered bit array
- `decode_mfm_byte()`: extract data bits at odd positions
- `find_sync()`: scan for `[0,1,0,0,0,1,0,0,1,0,0,0,1,0,0,1]` pattern

## Post-Mortem (Required for Artifact Index)

### What Worked

- **Bottom-up exploration first**: hex-dumping the HFE header before writing any decoder
  gave us the exact constants (track count, track length, bit rate) before writing code.
- **Verifying against blank_image.img**: using the existing embedded blank image as a
  reference to confirm the block→sector mapping was the fastest way to validate the decoder.
- **Running sd1diskutil list on decoded image**: using the existing tool to validate the
  Python decoder caught any mapping errors immediately.
- **Full FAT consistency check**: verifying free count in FAT == OS block free count
  gave high confidence the decode was byte-perfect.

### What Failed

- Initial sync mark search used MSB-first `[0x44, 0x89]` — found nothing because HFE
  stores bits LSB-first. Fixed by searching for `[0x22, 0x91]` (per-byte bit-reversed).

### Key Decisions

- **HFE in `sd1disk` library, not a new crate**: HFE is a serialization format for
  `DiskImage`; it belongs alongside `image.rs`. Alternatives: new crate (premature),
  CLI-only (loses testability).
- **Two explicit CLI commands** (`hfe-to-img`, `img-to-hfe`): mirrors existing verb-per-
  operation pattern; avoids magic extension detection.
- **Full CRC16-CCITT on read and write**: hardware-correct output; ensures HFE files work
  in Sojus VST3 and any other emulator. Not optional.
- **Corrupted .img repair: out of scope / not feasible**: sector 0 data is destroyed by
  the MAME bug; no recovery possible.

## Artifacts

- `docs/superpowers/specs/2026-03-30-hfe-support-design.md` — full approved design spec

## Action Items & Next Steps

1. **Invoke `writing-plans` skill** to produce an implementation plan from the design spec
   before writing any Rust code.

2. **Implement `crates/sd1disk/src/error.rs`** — add 3 new variants:
   ```
   InvalidHfe(&'static str)
   HfeCrcMismatch { track: u8, side: u8, sector: u8 }
   HfeMissingSector { track: u8, side: u8, sector: u8 }
   ```

3. **Implement `crates/sd1disk/src/hfe.rs`** — per the spec:
   - `crc16_ccitt(data: &[u8]) -> u16`
   - `encode_byte(byte: u8, prev_bit: &mut u8) -> [u8; 2]`
   - `encode_a1_sync() -> [u8; 2]`
   - `encode_track_side(img: &DiskImage, track: u8, side: u8) -> Vec<u8>`
   - `extract_side(raw: &[u8], side: u8, track_len: usize) -> Vec<u8>`
   - `decode_track_side(raw: &[u8]) -> Result<[Option<[u8; 512]>; 10]>`
   - `pub fn read_hfe(path: &Path) -> Result<DiskImage>`
   - `pub fn write_hfe(image: &DiskImage, path: &Path) -> Result<()>`

4. **Export from `crates/sd1disk/src/lib.rs`**:
   `pub mod hfe; pub use hfe::{read_hfe, write_hfe};`

5. **Wire CLI in `crates/sd1cli/src/main.rs`**:
   Add `HfeToImg { hfe: PathBuf, img: PathBuf }` and `ImgToHfe { img: PathBuf, hfe: PathBuf }`

6. **Write tests** per spec Section "Testing" (4 unit + 1 integration).

7. **Validate against `Ensoniq.hfe`** (real disk at `/Users/joemcmahon/Downloads/Ensoniq.hfe`):
   `sd1cli hfe-to-img /Users/joemcmahon/Downloads/Ensoniq.hfe /tmp/verify.img`
   then `sd1cli list /tmp/verify.img` should show OMNIVERSE, SOPRANO-SAX, 60-PRG-FILE.

## Other Notes

- The Python MFM decoder written this session is a complete working reference. If the
  Rust implementation produces unexpected results, re-derive from the Python logic rather
  than guessing. Key insight: LSB-first bit storage is the source of most confusion.
- `Ensoniq.hfe` is a hardware-written disk (not Sojus VST3). It has no block-1 VST3
  directory. SubDir 3 has 2 garbage entries that parse as valid but have 0 blocks and
  huge byte counts — pre-existing issue unrelated to HFE.
- Track gap3 size is dynamic (~75 bytes of 0x4E) — compute as
  `(12522 - fixed_content_bytes) / 10` to pad each track side to exactly 12,522 bytes.
- MFM track write order: Gap4a → Sync → Gap1 → [×10: Sync+IDAM+Gap2+Sync+DAM+Gap3].
  No index mark (Ensoniq doesn't use it).
- The `blank_image.img` embedded in `sd1disk` is a real hardware-formatted disk image
  and is the ground truth for block layout.
