---
date: 2026-03-26T00:01:31Z
session_name: general
researcher: Claude
git_commit: 2d7224b
branch: main
repository: sd1diskutil
topic: "VST3 extract fix — contiguous reads for block-1 directory files"
tags: [rust, ensoniq, sd-1, disk-image, fat, extract, vst3, block1]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Fix extract to use contiguous reads for VST3 block-1 files

## Task(s)

**COMPLETED: Attempted extract round-trip test (Step 3 from prior handoff)**
- User wrote NC 12 North patches as a standalone 60-PROGRAMS file to `~/Downloads/nc12_test.img`
- Attempted `cargo run -- extract nc12_test.img 60-PRG-FILE` → failed with "FAT is corrupt"
- Investigated and found the image was corrupt (VST3 write buffers never flushed to disk — see Learnings)
- Round-trip test deferred until user can do a proper virtual disk eject before quitting Live

**COMPLETED: Fix `cmd_extract` to use contiguous reads for VST3 block-1 files**
- Root cause: `cmd_extract` called `FileAllocationTable::chain()` for ALL files, including those found via `block1_find()`
- VST3-managed files use contiguous block allocation; the FAT is not reliable for them (SD-1 hardware overlays directory data on FAT blocks in block 1)
- Fixed: `cmd_extract` now tracks which directory the file came from and uses contiguous reads for block-1 files
- Fixed in `crates/sd1cli/src/main.rs:399-416`

## Critical References

- `crates/sd1disk/src/directory.rs:123-140` — `block1_entries` / `block1_find`: the VST3 block-1 directory reader
- `crates/sd1disk/src/fat.rs:5-10` — FAT constants (FAT_START_BLOCK=5, ENTRIES_PER_FAT_BLOCK=170, ENTRY_SIZE=3 bytes)

## Recent Changes

- `crates/sd1cli/src/main.rs:399-416` — replaced single FAT-chain path with branch: contiguous reads for block-1 files, FAT chain for subdirectory files

## Learnings

### VST3 disk image write-buffer behavior
The SD-1 VST3 plugin maintains an in-memory buffer of the simulated disk. It does NOT flush this buffer to the `.img` file until a disk-swap/eject operation is performed in the plugin. Simply quitting Ableton Live terminates the emulation without flushing. Symptoms:
- macOS "Date Added" / modification time on the `.img` file does NOT change
- OS free-block counter (block 2 bytes 1024-1027) is stale
- FAT entries for newly allocated files remain FREE (0x000000)
- The directory entry IS written (block 1 offset 0x1e) but the data blocks contain old/garbage content

**Diagnosis pattern**: if `list` shows a file but `inspect` shows "Free blocks: 0" and `extract` fails with FAT corrupt, suspect unflushed write buffer.

### SD-1 FAT structure quirks
The SD-1 blank disk (embedded in `blank_image.img` template) ships with the FAT pre-initialized with hardware-specific entries:
- FAT[0-93] are non-zero (the SD-1 OS uses these blocks for reserved/system purposes)
- The SD-1 uses a 3-byte-per-entry FAT at blocks 5-10, with 170 entries per 512-byte block
- Our `allocate()` function starts at FIRST_DATA_BLOCK=23 and correctly skips non-zero entries, so on a blank template disk it allocates starting at block 94, NOT block 23

**Consequence**: if a disk was written by the real hardware or VST3 plugin, files may be at blocks 23, 25, etc. with FAT entries that don't match our expectations. Our `allocate` would then put new files at block 94+, which is correct.

### VST3 block-1 directory vs. FAT
The SD-1 hardware format stores the VST3 directory at block 1 offset 0x1e (absolute byte offset 542). The FAT for blocks 0-255 is stored in block 1 (absolute bytes 512-1023). These regions overlap. The VST3 plugin therefore:
1. Always allocates contiguous blocks
2. Uses `first_block + size_blocks` from its own directory, never FAT chain traversal
3. Never maintains the FAT for its own files

Our `cmd_write` also calls `set_chain()` for VST3-path writes, which writes FAT data into block 1 bytes that are also used by the directory. These FAT writes get overwritten by subsequent directory writes. The FAT for VST3-managed files is therefore always unreliable.

### Contiguous reads are the right approach for VST3 files
`DirectoryEntry.size_blocks` and `.contiguous_blocks` always match for VST3-created files. Reading `size_blocks` consecutive blocks starting at `first_block` is always correct for block-1 directory files.

## Post-Mortem

### What Worked
- Direct raw block reads bypassing FAT to inspect data during diagnosis
- Python inline scripts to trace FAT entries, inspect directory entries, decode program names
- Checking `inspect` free-block count vs `list` free-block count as a quick corruption signal

### What Failed
- Initial attempt to extract using FAT chain: hit FREE immediately at first_block (FAT[201]=0)
- Trying to read program names with wrong offset (0 instead of 498) and wrong deinterleave (stream concat instead of byte-level alternation)
- Incorrect Python FAT reader (assumed 2-byte entries at block 1; actual format is 3-byte entries at block 5)

### Key Decisions
- Decision: Track `use_contiguous` boolean alongside entry rather than changing the directory API
  - Alternatives: add a method to `DirectoryEntry` to indicate source, or always use contiguous reads
  - Reason: minimal change, localized to `cmd_extract`, preserves existing behavior for subdirectory files

## Artifacts

- `crates/sd1cli/src/main.rs:399-416` — fixed `cmd_extract` with contiguous-read branch
- `thoughts/shared/handoffs/general/2026-03-25_16-31-35_sd1diskutil-all-interleave-fixes-complete-verified.md` — prior session handoff (all interleave fixes complete)

## Action Items & Next Steps

1. **Extract round-trip test** (deferred): once user does a proper virtual disk eject in the VST3 plugin before quitting Live, write NC 12 North programs to a fresh disk and run:
   ```
   cargo run -- extract <img> 60-PRG-FILE --out /tmp/nc12_extracted.syx
   python3 tools/dump_programs.py <img> "<name>" <original.syx>
   ```
   Look for "All 60 slots match!"

2. **`cmd_write` FAT cleanup** (optional): `cmd_write` still calls `set_chain()` for block-1 files, which writes FAT data that will be immediately overwritten by the directory write. This is harmless but wasteful. Could skip `set_chain` for block-1 path writes.

3. **`cmd_delete` review** (optional): `free_chain()` in delete also uses FAT traversal — same issue for VST3-managed files. Not urgent since deletion of VST3 files isn't a common path.

## Other Notes

**SysEx library location**: `/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/`

**Test command pattern**:
```
cargo run -- create /tmp/test.img
cargo run -- write /tmp/test.img <sysex>
cargo run -- extract /tmp/test.img "<NAME>" --out /tmp/extracted.syx
python3 tools/dump_programs.py /tmp/test.img "<NAME>" <original.syx>
```

**Hardware confirmation reference**: KOTO-DREAMS is at INT bank 5 patch 2 (b10=32) — canonical reference point for verifying correct interleave output.

**All tests passing**: `cargo test` 11/11 passing as of this handoff.
