---
date: 2026-04-02T15:14:51-07:00
session_name: general
researcher: Claude
git_commit: 983805ea1d001753c041c04152963874e530424d
branch: main
repository: sd1diskutil
topic: "file_number / type_info / free_blocks Bug Fixes"
tags: [bugfix, directory, fat, emulator-compatibility]
status: complete
last_updated: 2026-04-02
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Three directory-write correctness bugs fixed; awaiting emulator verification

## Task(s)

1. **Diagnose why emulator couldn't see sd1cli-written SixtySequences file — COMPLETED**
   User disk `/Volumes/Aux Brain/Music, canonical/Ensoniq/Blank image.IMG` had a
   sd1cli-written file (`SEQ-AN ESEQ`) visible in `sd1cli list` but invisible in the
   emulator's file-selection screen.

2. **Fix three root-cause bugs in `cmd_write` — COMPLETED**
   All three fixes are in `main` at commit `983805e` (the pre-session README commits;
   the actual code changes are uncommitted — see "Recent changes" below).

3. **Emulator end-to-end verification — IN PROGRESS**
   User is running the full workflow: erase disk in emulator → save sequences →
   verify sd1cli → write sysex via sd1cli → verify emulator sees and loads it →
   re-save under new name → verify both agree. Results not yet received.

## Critical References

- `crates/sd1disk/src/directory.rs` — `next_file_number`, `file_type_info`, and all
  directory entry logic
- `crates/sd1disk/src/fat.rs` — `count_free` and all FAT manipulation
- `crates/sd1cli/src/main.rs:398-424` — `cmd_write` where all three fixes land

## Recent changes

All changes are **uncommitted** (working tree only):

- `crates/sd1disk/src/directory.rs` — added `pub fn next_file_number(img, &FileType) -> u8`
  and `pub fn file_type_info(&FileType, bool) -> u8` with 8 new tests
- `crates/sd1disk/src/fat.rs` — added `pub fn FileAllocationTable::count_free(img) -> u32`
  with 3 new tests
- `crates/sd1disk/src/lib.rs:11` — re-exports `next_file_number` and `file_type_info`
- `crates/sd1cli/src/main.rs:4-7` — imports `next_file_number`, `file_type_info`
- `crates/sd1cli/src/main.rs:398-424` — three fixes in `cmd_write`:
  1. `type_info` now uses `file_type_info()` → `0x00`/`0x20` (was `0x0F`/`0x2F`)
  2. `file_number` now uses `next_file_number()` (was hardcoded `0`)
  3. `img.set_free_blocks(FileAllocationTable::count_free(&img))` added before `save`

## Learnings

### Bug 1 (primary): file_number conflict
- The SD-1 emulator indexes its file-selection list by `file_number` (byte 22 of each
  26-byte directory entry). When two files of the same `FileType` share the same
  `file_number`, the emulator only shows whichever it encounters first in the directory
  scan — the other is silently invisible.
- `cmd_write` was hardcoding `file_number: 0` for all writes.
- Real hardware/emulator assigns 0, 1, 2, … per-type sequentially.
- `next_file_number` counts existing entries of matching `FileType` across all 4
  subdirectories and returns the count as the next number.

### Bug 2 (secondary): type_info lower nibble
- `cmd_write` was producing `type_info = 0x0F` (normal) / `0x2F` (programs-embedded).
- Real hardware/emulator produces `0x00` / `0x20`.
- The lower nibble `0x0F` is incorrect; only bit 5 (`0x20`) carries meaning for the
  SD-1 OS (signals programs embedded in SixtySequences, telling it to read seq data at
  offset 44032 instead of 11776).
- Confirmed by hex-dumping the directory blocks of the live disk and comparing
  emulator-written entries vs sd1cli-written entries.

### Bug 3 (tertiary): free block count not updated after write
- `DiskImage::free_blocks()` reads a big-endian u32 from OS block (block 2, offset 0)
  — this is a cached count maintained by the SD-1 OS itself.
- `cmd_write` was never calling `set_free_blocks()`, so the header count stayed at its
  value from before the write. Observed: `inspect` showed 1316 free while FAT scan
  showed 1188 free (128-block discrepancy on the test disk).
- Fix: call `img.set_free_blocks(FileAllocationTable::count_free(&img))` right before
  `img.save()`.

### Disk format details confirmed
- Directory entries are 26 bytes; byte 22 = `file_number` ("SLOT" in `sd1cli list`)
- 4 subdirectories (SubDir 0–3), each starting at block `15 + dir_idx*2`
- `parse_entry` skips entries where **byte 1** (file_type) is 0 — not byte 0
  (type_info). This matters when hex-dumping: an entry with type_info=0x00 but
  file_type≠0 is valid (as emulator-written SD1-PALETTE demonstrates).
- `FileAllocationTable::allocate` only returns candidate block list; caller must call
  `set_chain` to commit to FAT. `count_free` must be called after `set_chain`.

### How `all_entries` selects between block-15 and block-1 directory
- If SubDir 0 (block 15) has any valid entries → use SubDirs 0–3 (hardware format)
- If SubDir 0 is empty → fall back to block-1 flat directory (VST3 plugin format)
- The check is on SubDir 0 only; a single entry anywhere in SubDir 0 triggers the
  hardware path.

## Post-Mortem

### What Worked
- **Hex dump + Python analysis**: Dumping raw directory bytes (checking byte 1, not
  byte 0, to detect empty slots) immediately revealed SD1-PALETTE had type_info=0x00
  while sd1cli was writing 0x0F.
- **Comparing emulator-written vs sd1cli-written entries side by side**: Made the
  file_number conflict obvious (two SixtySequences both at slot 0).
- **TDD for new helpers**: Writing tests for `next_file_number`, `file_type_info`, and
  `count_free` before implementing them caught the `allocate`-vs-`set_chain` API
  subtlety immediately (count_free test failed on first try, revealing allocate doesn't
  commit to FAT).

### What Failed
- **First hex dump used wrong empty-slot check** (byte 0 instead of byte 1), causing
  SD1-PALETTE to appear missing from the directory. Re-running with the correct check
  (matching `parse_entry`'s `data[1] == 0` guard) showed all four files.

### Key Decisions
- **`next_file_number` and `file_type_info` go in `sd1disk`, not `sd1cli`**: They
  describe disk format semantics and belong alongside the directory code, not in the
  CLI. Makes them testable without I/O.
- **`count_free` goes in `FileAllocationTable`**: It's a FAT operation; putting it in
  `DiskImage` would require importing FAT internals into image.rs (layering violation).
- **Take `&FileType` not `FileType` in helpers**: `FileType` doesn't implement `Copy`;
  taking by reference avoids move errors in cmd_write's loop where `file_type` is also
  needed by the `DirectoryEntry` struct literal.

## Artifacts

- `crates/sd1disk/src/directory.rs:142-155` — `next_file_number` and `file_type_info`
- `crates/sd1disk/src/directory.rs:422-526` — new tests (13 tests for 3 behaviors)
- `crates/sd1disk/src/fat.rs:136-142` — `count_free`
- `crates/sd1disk/src/fat.rs:252-280` — new tests for `count_free`
- `crates/sd1disk/src/lib.rs:11` — updated re-exports
- `crates/sd1cli/src/main.rs:4-7` — updated imports
- `crates/sd1cli/src/main.rs:398-424` — the fixed `cmd_write` close

## Action Items & Next Steps

1. **Await emulator verification results** from user's end-to-end test:
   - Erase disk in emulator → save sequences → `sd1cli list` confirms → `sd1cli write`
     sysex → emulator can see and load it → re-save under new name → both agree
2. **Commit the fixes** (use `/commit` skill per git-commits.md rule):
   - Suggested message: "fix: assign correct file_number, type_info, and free_blocks on write"
3. **Re-write the broken disk**: The existing test disk still has the old bad entries
   (SEQ-AN ESEQ with file_number=0, type_info=0x2F). User should re-write with
   `--overwrite` to fix it: `sd1cli write --overwrite <disk.IMG> <sysex.syx>`
4. **Optional — inspect free_blocks discrepancy in older files**: The emulator-written
   SD1-PALETTE has type_info=0x00 (correct), so that file is fine. The type_info issue
   only affects sd1cli-written files.

## Other Notes

- Test command: `cargo test` from workspace root — 84 tests, ~0.7s (73 sd1disk lib +
  11 integration)
- The `--overwrite` flag removes the old entry and frees its FAT chain before
  re-writing, so `next_file_number` sees the correct count after removal.
- User's live test disk: `/Volumes/Aux Brain/Music, canonical/Ensoniq/Blank image.IMG`
- Real hardware reference disk: `/Users/joemcmahon/Downloads/Ensoniq.hfe`
- Previous session's handoff (HFE implementation): `thoughts/shared/handoffs/general/2026-03-31_18-23-57_hfe-read-write-implementation-complete.md`
