---
date: 2026-03-25T22:39:50Z
session_name: general
researcher: Claude
git_commit: 31c716f
branch: main
repository: sd1diskutil
topic: "VST3 block-1 directory support added; interleave bug fix is next"
tags: [rust, ensoniq, sd-1, disk-image, directory, vst3, block1, interleave, list-command]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: VST3 block-1 directory reading fixed; interleave fix is next

## Task(s)

**COMPLETED: Diagnose why `cargo run -- list` fails on VST3-written disks**
- Root cause: VST3 zeroes block 15 (our primary directory) and writes all entries at block 1 offset 0x1e
- Same 26-byte entry format, just different location
- Normal SD-1 hardware (and our code) uses blocks 15/17/19/21 for subdirectories

**COMPLETED: Fix `list` and `extract` to read VST3 block-1 directory**
- Added `block1_entries()` and `block1_find()` to `crates/sd1disk/src/directory.rs`
- Extracted shared `parse_entry()` helper used by both `SubDirectory` and the new functions
- `cmd_list` and `cmd_extract` in `crates/sd1cli/src/main.rs` now fall back to block-1 when subdirectory 0 is empty
- All 57 tests pass (46 unit + 11 integration)

**COMPLETED: Verify `~/Downloads/db_rebuild.img` is the VST3-modified disk**
- `/tmp/db_rebuild.img` (00:18) = clean Rust-written image, only FPST + FSEQ
- `~/Downloads/db_rebuild.img` (00:25) = VST3-modified, has FSEP (sequences+patches at block 203) and FSE0
- `list` now correctly shows all 4 files on the Downloads version

**CONFIRMED: `blank_image.img` origin**
- Created from `zeroed.img` (hardware-formatted SD-1 disk) by stripping one file (commit 009d83e)
- NOT derived from `disk_with_everything.img`
- Both disks use block 15 because that's where real SD-1 hardware writes its directory

**OPEN: Fix `interleave_sixty_programs` in Rust**
**OPEN: Fix `deinterleave_sixty_programs` in Rust**
**OPEN: Fix `decode_b10` in `dump_programs.py`**

## Critical References

- `crates/sd1disk/src/types.rs:149-168` — `interleave_sixty_programs` (WRONG — needs fix)
- `crates/sd1disk/src/types.rs:300-315` — `deinterleave_sixty_programs` (WRONG — needs fix)
- `tools/dump_programs.py:285` — `decode_b10` (fix: change `disk_programs[deint_slot]` to `disk_programs[b10]`)

## Recent Changes

- `crates/sd1disk/src/directory.rs` — added `parse_entry()` (private), `block1_entries()`, `block1_find()` (public); refactored `SubDirectory::read_entry` to call `parse_entry`; added 3 new tests
- `crates/sd1disk/src/lib.rs:11` — exported `block1_entries`, `block1_find`
- `crates/sd1cli/src/main.rs` — added `all_entries()` helper; updated `cmd_list` to use it; updated `cmd_extract` to also search block-1 entries via `block1_find`

All changes are uncommitted (working tree only).

## Learnings

### VST3 vs hardware directory format
- Real SD-1 hardware uses blocks 15/17/19/21 for 4 subdirectories (39 entries each, 26 bytes/entry)
- VST3 plugin overwrites block 1 (the SD-1 OS data block) with its own flat directory
- VST3 directory: entries start at block 1 byte offset 0x1e (30 bytes), same 26-byte format
- VST3 zeroes block 15 entirely when it writes to the disk
- Block 1 on hardware disks has `00 80 01 00 00 0a...` repeating OS data; byte at position 0x1f = 0x80 which is an invalid FileType → safe to always scan block 1 (spurious hits are impossible)

### Why the fallback works safely
The detection logic: if subdirectory 0 (block 15) has no valid entries AND block 1 has valid entries → use block-1 format. If block 15 has entries → use subdirectories as normal. This correctly handles: fresh blank disks, hardware-written disks, VST3-modified disks.

### The garbled `list` output on VST3 disks (before fix)
Block 21 (subdirectory 3 base) happened to contain FSEQ sequence event data with bytes at positions matching valid FileType values (0x0f, 0x0d, 0x0c at slot[1]). These produced 3 false entries with nonsense names/sizes.

### The interleave bug (still open)
`interleave_sixty_programs` uses an even/odd byte interleave that places programs at wrong INT bank/patch positions on disk. Confirmed via VST3 test: same AllPrograms SysEx loaded directly into VST3 puts KOTO-DREAMS at INT 5 slot 2 (b10=32); our disk image has it at INT 2 patch 4 (b10=16).

The ground truth for fixing this: `~/Downloads/db_rebuild.img` file `SEQ-DB FSEP` (block 203, 178 blocks, 87469 bytes). This file was saved by the SD-1 VST3 with the correct patch bank embedded. Diffing the programs section (bytes 11776–43575 within the file) against what our `interleave_sixty_programs` produces from the same source will reveal the correct mapping.

### decode_b10 fix (still open)
Current formula in `tools/dump_programs.py:234-264`: `deint_slot = b10 * 2 if b10 < 30 else (b10 - 30) * 2 + 1`
This formula is WRONG — it was "confirmed" against COUNTRY-* but that also had the wrong interleave, so both matched by coincidence.
Correct lookup after fixing deinterleave: `disk_programs[b10]` directly (no conversion needed).

## Post-Mortem

### What Worked
- **Hex dumping specific blocks**: Comparing block 1 and block 15 across all three disk variants immediately showed the VST3 format difference
- **TDD**: Writing failing tests for `block1_entries` / `block1_find` first, then implementing — tests passed on first compile after implementation
- **Extracting `parse_entry` helper**: Sharing the 26-byte parsing logic between `SubDirectory` and the new block-1 functions eliminated duplication and made the refactor clean
- **Three-disk validation**: Checking `list` against VST3 disk, reference disk, and clean Rust disk simultaneously confirmed the fix works in all cases

### What Failed
- **First `all_entries` implementation**: Used `if subdir_entries.is_empty()` as the fallback check — this failed because garbled entries from block 21 made `subdir_entries` non-empty despite block 15 being zeroed
- **Initial `BLOCK1_DIR_OFFSET` hypothesis was 0x20 (32 bytes)**: Actually 0x1e (30 bytes) — off by 2. Verified by parsing the actual block 1 bytes

### Key Decisions
- Decision: Fall back to block-1 ONLY when subdirectory 0 is empty (not when all 4 subdirs are empty)
  - Reason: The VST3 always zeroes block 15 specifically; checking subdirectory 0 is a reliable signal
- Decision: Keep `parse_entry` private, expose only `block1_entries` and `block1_find` as public API
  - Reason: Callers only need the high-level operations; raw entry parsing is an implementation detail
- Decision: Do NOT attempt interleave fix without the corrected disk image diff
  - Reason: Two previous sessions guessed the interleave incorrectly; derive from evidence

## Artifacts

- `crates/sd1disk/src/directory.rs:1-10` — new constants (`BLOCK1_DIR_OFFSET`)
- `crates/sd1disk/src/directory.rs:97-137` — `parse_entry`, `block1_entries`, `block1_find`
- `crates/sd1disk/src/directory.rs:155-159` — refactored `SubDirectory::read_entry`
- `crates/sd1disk/src/directory.rs:340-410` — 3 new tests for block-1 directory
- `crates/sd1disk/src/lib.rs:11` — updated exports
- `crates/sd1cli/src/main.rs:3-6` — updated imports
- `crates/sd1cli/src/main.rs:166-179` — new `all_entries()` helper
- `crates/sd1cli/src/main.rs:180-202` — updated `cmd_list`
- `crates/sd1cli/src/main.rs:389` — updated `cmd_extract` fallback
- `~/Downloads/db_rebuild.img` — VST3-modified disk with FSEP (correct interleave ground truth)

## Action Items & Next Steps

1. **Commit the block-1 directory fix** — all 57 tests pass, ready to commit
2. **Extract SEQ-DB FSEP from `~/Downloads/db_rebuild.img`**:
   ```
   cargo run -- extract ~/Downloads/db_rebuild.img "SEQ-DB FSEP" /tmp/db_fsep_raw.bin
   ```
   Note: this extracts as SysEx; to get raw disk bytes for diffing, need to read the file data directly from disk blocks (block 203, 178 blocks, 87469 bytes)
3. **Diff programs section** (bytes 11776–43575 within FSEP) against what `interleave_sixty_programs` produces from `seq-DB final (all).syx` AllPrograms payload — the diff reveals the correct interleave
4. **Fix `interleave_sixty_programs`** in `crates/sd1disk/src/types.rs:149-168`
5. **Fix `deinterleave_sixty_programs`** in `crates/sd1disk/src/types.rs:300-315` (exact inverse of interleave)
6. **Fix `decode_b10`** in `tools/dump_programs.py:285` — change `disk_programs[deint_slot]` to `disk_programs[b10]`
7. **Run all 57 tests** — some interleave tests will likely fail and need updating
8. **Regenerate disk images** and verify in MAME

## Other Notes

**Disk image locations:**
- `~/Downloads/db_rebuild.img` — VST3-modified (FSEP at block 203, FSE0 at block 381) — GROUND TRUTH for interleave fix
- `/tmp/db_rebuild.img` — clean Rust-written (FPST + FSEQ only)
- `./disk_with_everything.img` — reference disk (49 files, hardware-written, block-15 format)

**VST3 block-1 directory layout:**
- Byte 0-29 of block 1: header/metadata (includes "OS" at bytes 28-29)
- Bytes 30+ (0x1e+): entries at 26-byte intervals, same format as `SubDirectory`
- Entry count: up to 39 (same SUBDIR_CAPACITY)

**extract command caveat:** `cargo run -- extract` wraps output in SysEx format. To get raw disk bytes for the programs section diff, read directly from image bytes at `203 * 512` with length 87469. Use Python:
```python
with open('/Users/joemcmahon/Downloads/db_rebuild.img', 'rb') as f:
    f.seek(203 * 512)
    fsep_raw = f.read(87469)
programs_section = fsep_raw[11776:43576]  # 31800 bytes = 60 × 530
```

**Source SysEx for comparison:**
- `/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-DB final (all).syx`
- Packet 1 = AllPrograms (0x03), de-nybblize to get 31800 bytes of raw program data

**Test count:** 57 tests (46 unit + 11 integration) — all passing after this session's changes.

**b10 encoding (unchanged, correct):**
- 0x00–0x3B: RAM program, b10 = bank×6+patch
- 0x80–0xFE: ROM program, enc = b10 & 0x7F, rom_index = enc + 8
- 0x7F: no program change; 0xFF: track inactive
