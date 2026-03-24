---
date: 2026-03-24T07:22:02Z
session_name: general
researcher: Claude
git_commit: 9ca196c
branch: main
repository: sd1diskutil
topic: "SD-1 AllSequences on-disk block padding — root cause found and fixed"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, fat, block-padding, testing]
status: complete
last_updated: 2026-03-24
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: AllSequences block-padding fix; emulator confirmed working

## Task(s)

**COMPLETED: End-to-end test of AllSequences write in MAME SD-1 emulator**
- Root cause of error 192 identified: each sequence chunk must be padded to a 512-byte
  block boundary on disk; `allsequences_to_disk()` was packing sequences tight with no padding
- Fix applied to `crates/sd1disk/src/types.rs`
- Unit test updated to verify padded layout
- `/tmp/test-sysex2.img` loaded and played successfully in MAME emulator ✓

**COMPLETED: Reference disk injection tool**
- `tools/inject_reference_seq.py` written — copies a file from a known-good disk image
  into a fresh `sd1cli create`-initialized disk, bypassing SysEx conversion
- Used to validate disk structure independently of SysEx conversion
- `test-ref3.img` (reference copy of COUNTRY-*) also loaded successfully in emulator ✓

## Critical References

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler format spec (authoritative)
- `crates/sd1disk/src/types.rs:189` — `allsequences_to_disk()` function (just fixed)
- `disk_with_everything.img` — reference disk; COUNTRY-* at block 1360, size 58983

## Recent Changes

- `crates/sd1disk/src/types.rs:214-253` — `allsequences_to_disk()` rewritten to pad each
  sequence to 512-byte blocks (was packing sequences tight = error 192 on load)
- `crates/sd1disk/src/types.rs:338-395` — `allsequences_to_disk_layout` unit test updated
  to use a defined header and verify padded output (11776 + 512 for a 170-byte sequence)
- `tools/inject_reference_seq.py` — new tool: copies a named file from source disk into
  target disk with correct FAT chain and directory entry
- `crates/sd1disk/src/lib.rs:17` — `allsequences_to_disk` in public exports (from prior session)
- `crates/sd1cli/src/main.rs:264-268` — AllSequences arm wired up (from prior session)

Note: these changes are **uncommitted** as of this handoff. Commit before continuing.

## Learnings

### Block padding is the disk format, NOT an artifact of FAT allocation

The Giebler spec note "each defined sequence occupies at least one full disk block" means the
sequence data section in the file itself has each sequence padded to a 512-byte multiple. The
`size_sum` field in the global section records the **unpadded** sum (sum of `data_size` fields
from all sequence headers). The actual on-disk space consumed is the padded total.

Discovery method: FAT chain walk of reference file showed 121 blocks × 512 = 61952 bytes
while directory entry claimed 58983 bytes. Walking the sequence data section from offset 44032
with block-padded strides gave perfect matches for all 8 defined sequences.

### SysEx event data is packed (no block padding)

The AllSequences SysEx payload contains sequence data packed consecutively with no padding.
Conversion to disk format must insert the padding. The `data_size` from each sequence header
tells us exactly how many bytes to copy from the SysEx for that sequence.

### size_sum encodes unpadded total

`global[2:6]` (BE u32) = `sum(data_size_i) + 0xFC`. This equals `seq_data_len + 0xFC` where
`seq_data_len` is the unpadded bytes of sequence data. The SD-1 uses this to know how much data
to load; it does NOT reflect the padded on-disk size.

### Reference file on-disk layout (disk_with_everything.img COUNTRY-*)

- FAT chain: 121 blocks (61952 bytes), directory entry claims 58983 bytes (trailing zeros)
- Sequence data starts at **file offset 44032** (after 60 programs + 456 byte padding)
- 8 defined sequences, all match with 512-byte-aligned strides

### OS version bytes are NOT the cause of error 192

Bytes 186-187 per sequence header = OS major.minor of the writing machine. Reference disk has
OS 3.00; SysEx files have OS 1.x (VFX-SD era). Error 192 = "sequencer memory corrupt", not
an OS version gate. The SD-1 is backwards-compatible with VFX-SD sequences. OS version bytes
are metadata only.

### Disk structure constants (confirmed)

- FAT: starts block 5, 3-byte BE entries, 170/block, EOF=0x000001, free=0x000000
- SubDir0: starts block 15, 26-byte entries, 39 slots capacity
- First data block: 23, total blocks: 1600
- Block size: 512 bytes

### Python injection tool finds file by name prefix

`tools/inject_reference_seq.py` uses prefix matching. The COUNTRY-* entry in the reference
disk has a leading space: `' COUNTRY-*'`. Pass `" COUNTRY"` (with leading space) as the
name argument to find it.

## Post-Mortem

### What Worked

- Reference disk validation first: testing `test-ref3.img` (injected reference data) before
  `test-sysex2.img` (SysEx-converted) isolated disk structure from conversion correctness
- FAT chain walk to get true file size: directory entry says 58983 but chain is 61952 bytes;
  using chain data exposed the block-padding pattern
- Binary hypothesis testing: comparing packed vs padded walks against reference data
- `inspect-sysex` CLI command quickly showed 2-packet structure (Command + AllSequences)

### What Failed

- Initial assumption that `size_sum` encodes the on-disk padded size — it encodes unpadded
- Initial `REF_SEQ_DATA_OFF = 44032` assumption failed because I read only 58983 bytes of
  the reference file instead of the full FAT chain (missing the padded remainder)
- Hypothesis that OS 1.x version bytes in headers cause error 192 — incorrect; it's structural

### Key Decisions

- **Pad in `allsequences_to_disk()`, not in the caller**: conversion function owns the format
  contract; callers get a ready-to-write blob
- **Keep `size_sum` as-is in global section**: the SD-1 expects the unpadded sum there; only
  the layout of the data section changes
- **Walk headers to determine per-sequence sizes**: use `hdr[183:186]` (data_size) to know
  how many bytes to consume from the packed SysEx event data per sequence

## Artifacts

- `tools/inject_reference_seq.py` — reference disk injection utility
- `tools/compare_allseq_sysex_vs_disk.py` — SysEx vs disk binary analysis (keeper)
- `crates/sd1disk/src/types.rs:189-253` — `allsequences_to_disk()` (fixed)
- `crates/sd1disk/src/types.rs:338-395` — updated unit test

## Action Items & Next Steps

1. **Commit the current changes** — `types.rs`, `lib.rs`, `main.rs`, `tools/inject_reference_seq.py`
2. **Test remaining SysEx files** — `seq-rockseq1.syx`, `seq-playseq1.syx` in emulator
3. **Consider `file_number` field** — currently always 0; low priority, not a crash risk
4. **Consider AllSequences+Programs variant** — if a SysEx dump includes programs alongside
   sequences (SixtySeq+60Programs layout), the current code would write the wrong layout type.
   Not yet observed but worth noting.
5. **Write integration test** — a test that runs the full `write` → `list` → verify file size
   pipeline with a real SysEx fixture would catch regressions

## Other Notes

**Production write test (confirmed working):**
```
cargo run -p sd1cli -- create /tmp/test-sysex2.img
cargo run -p sd1cli -- write /tmp/test-sysex2.img seq-countryseq.syx --name COUNTRY
# → COUNTRY SixtySequences 76 38912
# Loaded and played successfully in MAME SD-1 emulator
```

**How to rebuild reference injection test:**
```
cargo run -p sd1cli -- create /tmp/test-ref3.img
python3 tools/inject_reference_seq.py disk_with_everything.img /tmp/test-ref3.img " COUNTRY"
cargo run -p sd1cli -- list /tmp/test-ref3.img
```

**SysEx test files on Aux Brain:**
- `SysEx Librarian/sequences/seq-countryseq.syx` (OS 1.54, 2 packets — CONFIRMED WORKING)
- `SysEx Librarian/sequences/seq-rockseq1.syx` (OS 1.x — not yet tested)
- `SysEx Librarian/sequences/seq-playseq1.syx` (OS 1.x — not yet tested)

**Test count:** 52 total (43 unit + 9 integration), all passing
