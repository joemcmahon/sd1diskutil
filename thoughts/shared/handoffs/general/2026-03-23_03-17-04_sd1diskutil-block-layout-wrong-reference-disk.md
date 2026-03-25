---
date: 2026-03-23T10:17:04Z
session_name: general
researcher: Claude
git_commit: 009d83e
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — Wrong Block Layout (Used Wrong Reference Disk)"
tags: [rust, ensoniq, sd-1, disk-image, debugging, fat, directory, block-layout, regression]
status: in_progress
last_updated: 2026-03-23
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: REVERT REQUIRED — block constants changed based on wrong reference disk

## Task(s)

**BLOCKED / NEEDS REVERT: Latest commits introduced wrong FAT and directory block positions**

This session fixed several off-by-one errors in block layout constants, but the fixes
were based on `zeroed.img` as the reference — which turns out to be from a DIFFERENT
Ensoniq device (not the SD-1). `disk_with_everything.img` is the confirmed-correct SD-1
format and must be used as the reference.

### Session history (most recent first):

1. **WRONG (needs revert)** — commits 009d83e and the directory/image.rs changes in the
   working tree: changed FAT_START_BLOCK 5→4, SUBDIR_START_BLOCK 15→14,
   OS_FREE_COUNT_OFFSET block 2→block 1. Based on zeroed.img (wrong device).

2. **CORRECT** — commit f746597: CLI name fix, AllPrograms/AllPresets, type_info=0x0F,
   remove set_free_blocks() calls. Keep this commit.

3. **CORRECT** — commit d112d5f and earlier: original implementation. Keep.

## Critical References

- `disk_with_everything.img` — 49-file SD-1 disk, confirmed accepted by emulator for all
  file types. This is the authoritative SD-1 format reference. Use it as the blank template
  source.
- `zeroed.img` — **NOT SD-1 format**. From a different Ensoniq device. Do not use as
  reference for block layout.
- `crates/sd1disk/src/fat.rs` — FAT_START_BLOCK must be 5 (block 5), not 4
- `crates/sd1disk/src/directory.rs` — SUBDIR_START_BLOCK must be 15 (block 15), not 14

## Recent Changes (THIS SESSION — NEED REVERT)

These commits are on main and must be reverted or corrected:

- `crates/sd1disk/src/fat.rs:5` — FAT_START_BLOCK changed 5→4 (WRONG, revert to 5)
- `crates/sd1disk/src/fat.rs:142-161` — regression test asserts block 4, needs to assert block 5
- `crates/sd1disk/src/directory.rs:8` — SUBDIR_START_BLOCK changed 15→14 (WRONG, revert to 15)
- `crates/sd1disk/src/directory.rs` — regression test asserts block 14, needs to assert block 15
- `crates/sd1disk/src/image.rs:13-15` — OS_FREE_COUNT_OFFSET changed to block 1 bytes 2-3.
  VERIFY: disk_with_everything.img block 2 bytes 0-3 = `00 00 00 05` = 5 free blocks.
  So original offset (2 * BLOCK_SIZE = block 2, byte 0, read as u32) WAS CORRECT.
  Revert to u32 read from block 2.
- `crates/sd1disk/tests/operations_tests.rs` — changed to use zeroed.img as reference.
  Revert to use disk_with_everything.img.
- `blank_image.img` — currently derived from zeroed.img (wrong device). Must be rebuilt
  from disk_with_everything.img.

These changes are committed on main. Use `git revert` or manually fix.

## Learnings

### The Emulator Creates Wrong-Format Disks

**Critical discovery:** The SD-1 emulator has a bug where its "format disk" operation
creates disks with the WRONG block layout (FAT at block 4, dirs at block 14). But when
reading/writing pre-existing correctly-formatted disks, it uses the correct layout
(FAT at block 5, dirs at block 15).

This means:
- `test_image1.img` (emulator-created blank) = wrong layout → emulator rejects it
- `zeroed.img` (emulator-created with one file) = wrong layout → not a valid SD-1 reference
- `disk_with_everything.img` = correctly formatted → accepted by emulator

### Correct SD-1 Block Layout (disk_with_everything.img format)

```
Block 0:   OS ID block — `6d b6 6d b6...` pattern (512 bytes all non-zero)
Block 1:   Unknown structured data (154 non-zero bytes, starts 00 80 01 00 00 0a...)
           Wait — actually blank_image/zeroed have this in block 0 too. See note.
Block 2:   OS block with free count as u32 big-endian at byte 0
           disk_with_everything: `00 00 00 05` = 5 free blocks ✓
Block 3:   Ends with DR marker
Block 4:   Starts here in disk_with_everything (check structure)
Blocks 5-14: FAT (10 blocks × 170 entries × 3 bytes + "FB" marker)
Blocks 15-22: Sub-directories (4 dirs × 2 blocks each)
  SubDir 0: blocks 15-16
  SubDir 1: blocks 17-18
  SubDir 2: blocks 19-20
  SubDir 3: blocks 21-22
  Second blocks of each pair (16, 18, 20, 22) end with "DR" marker when empty/partially full
```

**NOTE**: Block 0 of disk_with_everything.img is `6d b6 6d b6...` but block 0 of
blank_image.img (test_image1.img) and zeroed.img is `00 80 01 00 00 0a...`. This
discrepancy was noticed but not resolved. disk_with_everything.img IS the confirmed-good
SD-1 format, so its block 0 is correct for the SD-1.

### OS_FREE_COUNT_OFFSET

In disk_with_everything.img (correct SD-1 format):
- Block 2, bytes 0-3 = `00 00 00 05` = 5 free blocks (u32 big-endian)
- This matches the ORIGINAL code: `OS_BLOCK_START = 2 * BLOCK_SIZE`, read as u32
- Our "fix" to block 1 bytes 2-3 was WRONG (based on zeroed.img, wrong device)

### What the Blank Image Should Look Like

For a blank SD-1 disk (based on disk_with_everything.img stripped of files):
- Block 0: same as disk_with_everything.img block 0 (`6d b6 6d b6...`)
- Block 1: same as disk_with_everything.img block 1
- Block 2: free count = 1577 as u32 BE at byte 0, rest same as disk_with_everything
- Blocks 3-4: same as disk_with_everything.img
- Blocks 5-13: FAT — all Free (0x000000) except entries 0-22 which are EndOfFile
  Actually for a BLANK disk (no files), all FAT entries for data blocks should be Free.
  But reserved blocks (0-22) should be EndOfFile. Verify this pattern in disk_with_everything.img.
- Block 14: FAT tail block (same as disk_with_everything.img last FAT block but cleared of file chains)
- Blocks 15-22: Sub-directories — ALL zeros except DR markers at end of blocks 16, 18, 20, 22

## Post-Mortem

### What Worked

- `cmp -l | awk` block-by-block comparison: very effective for finding structural differences
- Checking `cargo run -p sd1cli -- list` output: immediately reveals if directory reads work
- Verifying specific block content via Python hex dump: confirmed FAT chain positions

### What Failed

- **Used zeroed.img as reference**: zeroed.img is from a different Ensoniq device with
  different block layout (FAT at 4, dirs at 14). All analysis based on it was wrong.
- **Trusted emulator-formatted blank disks**: The emulator creates incorrectly-formatted
  disks. test_image1.img and zeroed.img are both emulator-created and have wrong layout.
- **Mistook emulator creation bugs for hardware format**: The emulator reads real format
  correctly but writes wrong format when creating new disks.

### Key Decisions

- **Decision**: Use disk_with_everything.img as the ONLY valid reference for SD-1 format
  - Reason: User confirmed it works in emulator for all file types
  - zeroed.img and test_image1.img are emulator-created and have wrong block layout

## Artifacts

- `crates/sd1disk/src/fat.rs` — needs FAT_START_BLOCK reverted to 5
- `crates/sd1disk/src/directory.rs` — needs SUBDIR_START_BLOCK reverted to 15
- `crates/sd1disk/src/image.rs` — needs OS_FREE_COUNT_OFFSET reverted to 2 * BLOCK_SIZE (u32 read)
- `crates/sd1disk/tests/operations_tests.rs` — needs update to use disk_with_everything.img
- `blank_image.img` — needs to be rebuilt from disk_with_everything.img

## Action Items & Next Steps

### Step 1: Fix constants (revert wrong changes)

```
FAT_START_BLOCK = 5        (was 4 after our changes)
SUBDIR_START_BLOCK = 15    (was 14 after our changes)
OS_FREE_COUNT_OFFSET = 2 * BLOCK_SIZE  (block 2, byte 0, read as u32)
free_blocks() → read 4 bytes as u32 big-endian
set_free_blocks() → write 4 bytes as u32 big-endian
```

### Step 2: Update regression tests

The regression tests added this session are good ideas but assert the WRONG block numbers.
Update them to assert:
- FAT chain for block 23 appears in disk block **5** (not 4)
- Directory entry for SubDir 0 appears in disk block **15** (not 14)

### Step 3: Rebuild blank_image.img from disk_with_everything.img

Write a Python script to create blank_image.img from disk_with_everything.img:
1. Start with disk_with_everything.img
2. Clear all FAT entries for blocks 23-1599: set to 0x000000 (Free)
   - FAT covers blocks 5-14, each with 170 entries × 3 bytes + FB
   - Find all non-EndOfFile, non-Free entries and zero them
3. Clear all sub-directory entries in blocks 15-22:
   - Zero out all 26-byte entry slots except keep DR markers at end of blocks 16,18,20,22
4. Set free block count at block 2 bytes 0-3 to `00 00 06 29` = 1577
5. Verify result: list command shows 0 files, 1577 free blocks

### Step 4: Test in emulator

After fixing constants and blank_image.img:
1. `cargo run -p sd1cli -- create /tmp/test.img`
2. `cargo run -p sd1cli -- write /tmp/test.img <sysex_file>`
3. Load in SD-1 emulator — should NOT get BAD DEVICE ID

### Step 5: Commit

Single commit: "fix: correct SD-1 block layout — FAT at block 5, dirs at block 15"

## Other Notes

**disk_with_everything.img block structure summary (first 23 blocks):**
```
Block  0: 512 non-zero (6d b6 pattern — SD-1 OS ID)
Block  1: 154 non-zero (00 80 01 00 00 0a...)
Block  2: 356 non-zero (free count = 5 at bytes 0-3)
Block  3:   2 non-zero (DR at end)
Block  4:   2 non-zero (DR at end? — check)
Blocks 5-14: FAT (non-zero = many, FB at end of each)
Block 15: 373 non-zero (SubDir 0 files)
Block 16: 374 non-zero (SubDir 0 continued)
Block 17: 176 non-zero (SubDir 1 files)
Block 18:   2 non-zero (DR at end — SubDir 1 second block)
Block 19:   0 non-zero (SubDir 2 empty)
Block 20:   2 non-zero (DR at end — SubDir 2 second block)
Block 21:   0 non-zero (SubDir 3 empty)
Block 22:  21 non-zero (DR at end — SubDir 3 second block)
```

**For a blank disk, blocks 15-22 should be:**
```
Block 15:   0 (SubDir 0 empty, first block)
Block 16:   2 (DR at end — SubDir 0 second block)
Block 17:   0 (SubDir 1 empty)
Block 18:   2 (DR at end)
Block 19:   0 (SubDir 2 empty)
Block 20:   2 (DR at end)
Block 21:   0 (SubDir 3 empty)
Block 22:  ~21 (DR at end, possibly other OS data)
```
Block 22's non-zero content in disk_with_everything.img should be PRESERVED in the blank
template (don't zero it out — it has OS structure, not file data).

**Key test commands:**
```
cargo test                    # All tests must pass
cargo run -p sd1cli -- create /tmp/test.img
cargo run -p sd1cli -- write /tmp/test.img <syx>
cargo run -p sd1cli -- list /tmp/test.img
```

**Python comparison snippet** (use to verify blank structure):
```python
python3 -c "
with open('blank_image.img', 'rb') as f: b = f.read()
with open('disk_with_everything.img', 'rb') as f: d = f.read()
for blk in range(23):
    s = blk*512
    nz = sum(1 for x in b[s:s+512] if x != 0)
    print(f'Block {blk:2d}: {nz:3d} non-zero')
"
```
