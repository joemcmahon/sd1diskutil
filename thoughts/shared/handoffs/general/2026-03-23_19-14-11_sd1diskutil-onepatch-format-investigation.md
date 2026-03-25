---
date: 2026-03-23T19:14:11Z
session_name: general
researcher: Claude
git_commit: 8031004
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — OneProgram write test & AllPrograms format investigation"
tags: [rust, ensoniq, sd-1, disk-image, sysex, format, debugging, onepatch, allprograms]
status: in_progress
last_updated: 2026-03-23
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: OneProgram write test pending; AllPrograms format may be wrong

## Task(s)

**IN PROGRESS: Verify OneProgram write path works in SD-1 emulator**

At end of this session, `/tmp/sabre_test.img` was created with `sabresaw.syx`
(OneProgram, "SABRE SAW", 530 bytes). User is testing in emulator RIGHT NOW.
Result not yet known — this is the critical next step.

**OPEN QUESTION: Does AllPrograms (SixtyPrograms) disk format match SysEx payload?**

PIANO.SYX (AllPrograms) was written to a disk and loaded in the SD-1 emulator.
Result: garbled names ("blank, @1.I, @2., 2., T, @@8.") and non-functional patches.

Two hypotheses:
1. The SD-1 on-disk format for AllPrograms is NOT the same as the denybblized SysEx payload
2. The blank_image.img reserved blocks are still wrong in a way that affects data reads

## Critical References

- `disk_with_everything.img` — 49-file SD-1 disk, confirmed accepted by emulator. Authoritative format reference.
- `crates/sd1disk/src/sysex.rs` — SysEx parse/nybblize/denybblize
- `crates/sd1cli/src/main.rs:210-233` — cmd_write, AllPrograms and OneProgram paths

## Recent Changes

This session (commit `8031004`):
- `crates/sd1disk/src/fat.rs:5` — FAT_START_BLOCK reverted 4→5 (correct SD-1 hardware value)
- `crates/sd1disk/src/fat.rs:148-164` — regression test updated to assert block 5
- `blank_image.img` — rebuilt from `disk_with_everything.img` by stripping file data; FAT blocks 5-14 cleared (reserved 0-22 = EndOfFile, data 23-1599 = Free), dir blocks 15-22 zeroed (DR markers kept), free count = 1577

Discarded (wrong working tree changes from previous session `009d83e`):
- `directory.rs` was being changed SUBDIR 15→14 (wrong)
- `image.rs` was being changed OS_FREE_COUNT_OFFSET to block 1
- `operations_tests.rs` was being switched to zeroed.img (wrong)

## Learnings

### The emulator creates wrong-format disks
The SD-1 emulator's "format disk" operation creates disks with wrong layout
(FAT at block 4, dirs at block 14). When READING pre-existing correct disks, it
uses the correct layout (FAT at block 5, dirs at block 15). Do NOT use
emulator-created blank disks as format references.

### Correct SD-1 block layout (confirmed from disk_with_everything.img)
- Block 0: OS ID (`6d b6` pattern)
- Block 1: Structured OS data (154 non-zero bytes)
- Block 2: Free block count as u32 BE at bytes 0-3
- Blocks 3-4: OS structures (end with DR)
- Blocks 5-14: FAT (170 entries × 3 bytes + FB marker per block)
- Blocks 15-22: Sub-directories (4 dirs × 2 blocks each; second blocks end with DR)

### blank_image.img rebuild approach
Built from disk_with_everything.img by:
1. Clearing FAT entries for blocks 23-1599 to Free (0x000000), keeping EndOfFile for 0-22
2. Zeroing all directory blocks 15-22, restoring only DR markers at end of blocks 16,18,20,22
3. Setting free count at block 2 bytes 0-3 to 1577
Block 22 OS version data (offset 476+) was NOT preserved — it overlapped with directory
slot 38 of SubDir 3 and caused that slot to appear occupied. Zeroing it fixed the tests.
The zeroed.img (confirmed-good emulator disk) had completely different content there anyway.

### AllPrograms disk format — POSSIBLY WRONG
The on-disk data for SixtyPrograms files in disk_with_everything.img does NOT have
readable ASCII names at byte offset 498 within each 530-byte program (where SysEx payload
has them). The only ASCII text found in the on-disk data is at offset 466 within some
programs (but not all). This MIGHT mean the disk format and SysEx format differ.

**Counter-evidence**: sizes match (31800 bytes = 60 × 530), contiguous_blocks = 63 for all
SixtyPrograms files on the reference disk. The format COULD be identical and the SD-1 INT
programs just happen to not have ASCII-range names at offset 498.

**Key test pending**: if OneProgram (sabresaw.syx) works in the emulator, then AllPrograms
format is the issue. If OneProgram also fails, there's a deeper structural problem.

### Directory entry fields (from disk_with_everything.img reference)
- `type_info` = 0x0F for all real entries
- `file_number` increments per-type (OneProgram: 0,1,2...; SixtyPrograms: 0,1,2...)
- `contiguous_blocks` = `size_blocks` for all contiguous files (most files)
- Our write path sets `file_number: 0` always — may need to be sequential per type

## Post-Mortem

### What Worked
- Discarding wrong working tree changes (`git checkout HEAD -- file`) before fixing constants
- Rebuilding blank_image.img via Python from disk_with_everything.img — clean and repeatable
- Block-by-block comparison (`python3 -c "for blk in range(23): nz = ..."`) to verify structure
- All 50 tests pass after fixes

### What Failed
- Previous session (009d83e) used zeroed.img as reference — wrong device, wrong block layout
- OS version data at block 22 offset 476 was initially preserved, causing directory test failures
  (bytes decode as a valid SequencerOs entry). Zeroing it fixed tests, and zeroed.img confirmed
  it's not needed.
- AllPrograms (PIANO.SYX) write produced garbled names/non-functional patches in emulator

### Key Decisions
- **FAT_START_BLOCK = 5**: confirmed from disk_with_everything.img (correct SD-1 reference)
- **SUBDIR_START_BLOCK = 15**: confirmed from disk_with_everything.img
- **Block 22 OS data zeroed**: chose to zero the OS version string at block 22 offset 476-509
  because (a) it overlapped with dir slot 38, (b) zeroed.img had completely different content
  there, (c) tests pass with it zeroed
- **set_free_blocks() removed from CLI**: not the library method (still exists), just CLI calls.
  The OS block free count is unreliable on hardware disks; FAT-derived count used for display.

## Artifacts

- `crates/sd1disk/src/fat.rs` — FAT_START_BLOCK = 5, updated regression test
- `blank_image.img` — rebuilt from disk_with_everything.img (correct SD-1 blank template)
- `/tmp/sabre_test.img` — test disk with sabresaw.syx written (OneProgram, "SABRE SAW")
  **This file is the immediate next thing to verify in the emulator.**

## Action Items & Next Steps

### IMMEDIATE: Check sabre_test.img emulator result
User was testing `/tmp/sabre_test.img` when this handoff was created.

**If OneProgram works** (name shows, patch sounds correct):
- The OneProgram write path is correct
- AllPrograms format needs investigation
- Likely need to compare byte-by-byte: extract a SixtyPrograms file from disk_with_everything.img
  and compare with PIANO.SYX denybblized payload to find the transformation

**If OneProgram also fails** (garbled name or broken patch):
- More fundamental issue — either blank_image.img reserved blocks still wrong,
  or the data format assumption (disk = denybblized SysEx) is wrong for ALL types
- Check: does the emulator show BAD DEVICE ID or does it load the disk but show bad data?

### If AllPrograms format is the issue
1. Extract SD1-INT from disk_with_everything.img (63 blocks starting at block 221)
2. Wrap it as a SysEx file: `F0 0F 05 00 00 03 [nybblize(data)] F7`
3. Load that SysEx in a MIDI tool and see if it parses correctly as SD-1 programs
4. OR: find an SD-1 AllPrograms SysEx dump from the same bank and compare byte-by-byte
   with the on-disk data to find the mapping

### Add SysEx inspector command (discussed, not yet implemented)
User requested an `inspect-sysex` subcommand for sd1cli that shows:
- Message type, MIDI channel, payload size
- For AllPrograms: all 60 program names
- For OneProgram: name and key parameters
This would make debugging much easier without needing the emulator.

### file_number field
Our write path always sets `file_number: 0`. The reference disk shows file_number increments
per file type (second SixtyPrograms = 1, third = 2, etc.). Verify whether this matters
for the SD-1 to correctly distinguish multiple files of the same type.

## Other Notes

**Test commands:**
```
cargo test                          # 50 tests must pass
cargo run -p sd1cli -- create /tmp/test.img
cargo run -p sd1cli -- write /tmp/test.img <file.syx>
cargo run -p sd1cli -- list /tmp/test.img
```

**Disk images in repo root:**
- `blank_image.img` — embedded template (correct, rebuilt this session)
- `disk_with_everything.img` — 49 files, authoritative SD-1 format reference
- `zeroed.img` — hardware-formatted, one 60-patch bank (NOT same device as SD-1 per previous sessions, but user says confirmed good by emulator — treat with caution)
- `test_image1.img` — emulator-created blank, wrong block layout, do not use

**SysEx test files available:**
- `/Volumes/Aux Brain/Music, canonical/Ensoniq/BANKS/PIANO.SYX` — AllPrograms, 60 patches
- `/Volumes/Aux Brain/Music, canonical/Ensoniq/Ensoniq SD VFX presets/singles/sabresaw.syx` — OneProgram, "SABRE SAW"

**AllPrograms cmd_write path** (`crates/sd1cli/src/main.rs:215-217`):
```rust
sd1disk::MessageType::AllPrograms => {
    (packet.payload.clone(), FileType::SixtyPrograms)
}
```
This writes the raw denybblized SysEx payload directly to disk. If the on-disk format
differs from SysEx payload, this is where the fix would go.

**AllPrograms on-disk structure scan result** (from disk_with_everything.img SD1-INT):
- No readable ASCII at offset 498 within programs (where SysEx names are)
- Some ASCII found at offset 466 in programs 1 and 3 ("DFYLNUAGMLI", "RVUIDBER-OG")
  — possibly program names at a different offset, or coincidental ASCII in param data
- On-disk values include bytes > 0x7F (e.g. 0xB0, 0xD5) confirming it's NOT nybblized MIDI
