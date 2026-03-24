---
date: 2026-03-24T06:52:00Z
session_name: general
researcher: Claude
git_commit: 9ca196c
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — AllSequences write implemented; disk image testing in progress"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, fat, directory, testing]
status: in_progress
last_updated: 2026-03-24
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: AllSequences write done; Python disk injector has wrong FAT EOF marker

## Task(s)

**COMPLETED: AllSequences SysEx → on-disk conversion implemented**
- `allsequences_to_disk()` in `crates/sd1disk/src/types.rs`
- Wired up in `crates/sd1cli/src/main.rs:264-268` (was a skip/warning, now calls conversion)
- 2 new unit tests added; 52 total tests pass
- `allsequences_to_disk` exported from `crates/sd1disk/src/lib.rs`

**IN PROGRESS: End-to-end test on SD-1 MAME emulator (VST)**
- Two bugs discovered in the Python disk-injection test script (not in production code)
- Need to re-run the test with both bugs fixed

## Critical References

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler format spec (authoritative)
- `crates/sd1disk/src/fat.rs` — FAT constants and EOF marker
- `disk_with_everything.img` — reference disk with working COUNTRY-* sequence file

## Recent Changes

- `crates/sd1disk/src/types.rs:170-223` — `allsequences_to_disk()` function added
- `crates/sd1disk/src/types.rs:331-374` — two unit tests: `allsequences_to_disk_layout` and `allsequences_to_disk_rejects_short_payload`
- `crates/sd1disk/src/lib.rs:17` — `allsequences_to_disk` added to public exports
- `crates/sd1cli/src/main.rs:264-268` — AllSequences arm now converts and writes (was `continue` skip)
- `tools/compare_allseq_sysex_vs_disk.py` — new analysis script (keeper)

## Learnings

### AllSequences payload structure (CONFIRMED across 3 SysEx files)
```
payload[0:240]              = 60 × 4-byte SD-1 internal memory pointer table (SKIP on write)
payload[240:-(21+11280)]    = sequence event data
  - First 12 bytes are ALWAYS zeros (SD-1 internal header, skip on disk write)
  - Actual disk data starts at payload[240+12] = payload[252]
payload[-(21+11280):-21]    = 60 × 188-byte sequence headers (copy verbatim to disk[0:11280])
payload[-21:]               = 21-byte global section (copy verbatim to disk[11280:11301])
```

### On-disk SixtySequences No-Programs layout
- `[0:11280]`     60 × 188-byte headers
- `[11280:11301]` global: [0:2]=curr_seq (BE u16), [2:6]=size_sum (BE u32), [6:21]=global info
- `[11301:11776]` 475 zeros
- `[11776:]`      seq_data_len bytes = `size_sum - 0xFC`

### 12-byte skip is constant across all AllSequences files
- Verified across `seq-countryseq.syx`, `seq-rockseq1.syx`, `seq-playseq1.syx`
- ptr_table[1]=21 and ptr_table[2]=252 are also constant — these are SD-1 internal RAM addresses, not data offsets

### FAT constants (crates/sd1disk/src/fat.rs)
- FAT starts at block 5, 3-byte big-endian entries (u24), 170 entries per block
- **EOF marker = 0x000001** (NOT 0x01FFFF — this caused "DISK ERROR - BAD FAT")
- Free block = 0x000000, Bad block = 0x000002, Next = block number as u24 BE

### Disk structure constants (crates/sd1disk/src/directory.rs)
- SubDir0 starts at **block 15** (not block 2)
- Directory entry size = **26 bytes** (not 32)
- First data block = 23

### OS version marker in disk header
- Block 2, offset 28-31 = `4f 53 0f 0a` = "OS" + version bytes
- Our `create` command writes this correctly — **do not overwrite block 2**
- Previous Python script accidentally clobbered this by writing at the wrong SubDir block (2 instead of 15)

### Disk image testing: CLI write vs Python injection
- Our production CLI (`sd1cli write`) works correctly — produces correct directory and FAT
- Python-based direct injection is useful for testing reference data bypassing SysEx conversion
- The Python injector needs the fixes below to work correctly

## Post-Mortem

### What Worked
- Binary comparison approach: `compare_allseq_sysex_vs_disk.py` quickly confirmed the 12-byte skip
- Verifying across 3 SysEx files confirmed the structure is invariant
- `cargo run -p sd1cli -- create` correctly initializes the OS marker and FAT

### What Failed
- Python disk injector: wrong SubDir block (2 instead of 15) → overwrote OS marker → "DISK ERROR - BAD DISK OS"
- Python disk injector: wrong FAT EOF marker (0x01FFFF instead of 0x000001) → "DISK ERROR - BAD FAT"
- SysEx `seq-countryseq.syx` has OS 1.x headers (VFX-SD era) vs reference OS 3.0 — may cause error 192

## Artifacts

- `tools/compare_allseq_sysex_vs_disk.py` — binary analysis / comparison tool
- `crates/sd1disk/src/types.rs` — `allsequences_to_disk()` at line 170
- `crates/sd1cli/src/main.rs:264-268` — AllSequences wire-up

## Action Items & Next Steps

### Immediate: Fix Python injector and re-test

The Python injector `tools/compare_allseq_sysex_vs_disk.py` does NOT do injection — it's an analysis tool. The injection code was written inline in the shell. To test the reference COUNTRY-* bypassing SysEx:

```python
# CORRECT FAT EOF marker:
img[fat_offset(last_blk):fat_offset(last_blk)+3] = (0x000001).to_bytes(3, 'big')

# CORRECT SubDir block:
SUBDIR_START_BLOCK = 15  # NOT 2

# CORRECT entry size:
SUBDIR_ENTRY_SIZE = 26   # NOT 32

# DO NOT touch block 2 (OS marker lives at block 2, offset 28-31)
```

Run the corrected version and test `/tmp/test-ref3.img` in the MAME emulator.

**If reference loads**: production code is fine; SysEx OS 1.x headers may still cause error 192 for SysEx-derived files. Consider patching bytes 186-187 of each header to `0x03 0x00` (OS 3.0) when writing AllSequences.

**If reference still crashes**: deeper disk structure issue — compare FAT layout byte-for-byte between reference and our output.

### Investigate OS version mismatch
The SysEx file `seq-countryseq.syx` has OS version 1.x in sequence headers (bytes 186-187 per header). The reference disk has OS 3.00. SD-1 emulator may reject OS 1.x files with error 192. If confirmed: patch headers on write to force OS 3.00, or accept that only OS 3.x SysEx files will load.

### file_number incrementing (low priority)
`file_number: 0` always written. Not a crash risk.

## Other Notes

**FAT entry byte layout:**
```
fat_offset(block) = (FAT_START_BLOCK + block // 170) * 512 + (block % 170) * 3
value written as 3-byte big-endian u24
```

**SysEx test files:**
- `/Volumes/Aux Brain/.../SysEx Librarian/sequences/seq-countryseq.syx` (OS 1.54, 2 packets)
- `/Volumes/Aux Brain/.../SysEx Librarian/sequences/seq-rockseq1.syx` (OS 1.x)
- `/Volumes/Aux Brain/.../SysEx Librarian/sequences/seq-playseq1.syx` (OS 1.x)

**Reference disk files in disk_with_everything.img:**
- COUNTRY-* (SixtySeq+60Progs): SubDir0 slot37, block=1360, size=58983 — OS 3.00 headers
- ROCK-BEATS (ThirtySeq): SubDir0 slot38, block=810, size=12118

**Production write test (CLI-based — working correctly):**
```bash
cargo run -p sd1cli -- create /tmp/test.img
cargo run -p sd1cli -- write /tmp/test.img seq-countryseq.syx --name COUNTRY
cargo run -p sd1cli -- list /tmp/test.img
# → COUNTRY SixtySequences 71 35980
```

**To rebuild reference test disk correctly (fixes both bugs):**
```python
# EOF: (0x000001).to_bytes(3, 'big')
# SubDir: SUBDIR_START_BLOCK = 15
# Entry: SUBDIR_ENTRY_SIZE = 26
# Skip block 2 entirely (OS marker)
```
