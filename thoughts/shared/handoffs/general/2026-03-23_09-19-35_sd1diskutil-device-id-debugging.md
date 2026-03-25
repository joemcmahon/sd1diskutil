---
date: 2026-03-23T09:19:35Z
session_name: general
researcher: Claude
git_commit: d112d5f
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — BAD DEVICE ID Debugging & Fixes"
tags: [rust, ensoniq, sd-1, disk-image, debugging, fat, directory, blank-image]
status: in_progress
last_updated: 2026-03-23
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: BAD DEVICE ID debugging — blank_image.img still wrong

## Task(s)

**IN PROGRESS: Fix sd1cli-written disk images to be accepted by the SD-1 emulator**

After the previous session completed full implementation, this session focused on:
1. Writing README.md and improving --help output ✅
2. Fixing AllPrograms/AllPresets SysEx write support ✅
3. Fixing subtract-with-overflow panic in cmd_write ✅
4. Debugging "BAD DEVICE ID" emulator rejection of written images — **PARTIALLY RESOLVED**

**KEY DISCOVERY (end of session):** "BAD DEVICE ID" is the SD-1's error for trying to LOAD from an empty disk — it is NOT a disk structure error. However, `test5.img` (created by our tool) still gets BAD DEVICE ID on ANY disk access (not just load), meaning our `blank_image.img` template still has wrong reserved block content.

## Critical References

- Previous handoff: `thoughts/shared/handoffs/general/2026-03-23_07-50-03_sd1diskutil-implementation-complete.md`
- `crates/sd1disk/src/image.rs` — DiskImage, blank_image embedding, free_blocks location (WRONG — see Learnings)
- `crates/sd1cli/src/main.rs` — cmd_write, cmd_delete

## Recent Changes

- `README.md` — created top-level user manual (new file)
- `crates/sd1cli/src/main.rs:10-13` — fixed binary name `sd1disk` → `sd1cli`, added long_about to all subcommands and args
- `crates/sd1cli/src/main.rs` — added `AllPrograms` → `SixtyPrograms` and `AllPresets` → `TwentyPresets` match arms in `cmd_write`
- `crates/sd1cli/src/main.rs` — removed `set_free_blocks()` calls from both `cmd_write` and `cmd_delete` (was corrupting block 2)
- `crates/sd1cli/src/main.rs:280` — changed `type_info: 0` → `type_info: 0x0F` in directory entry (every real entry has 0x0F)
- `blank_image.img` — replaced with hardware-initialized image from emulator (test_image1.img)

## Learnings

**"BAD DEVICE ID" on empty disk is expected behavior** — the SD-1 emulator shows this when you try to LOAD from a disk with no files. It is NOT a disk structure validation error. We wasted time debugging a non-error. A disk with at least one file written to it will load correctly.

**However**, our `blank_image.img` still produces images that get BAD DEVICE ID on ANY access (insert/browse, not just load). This means the reserved block structure is still wrong.

**Block layout (verified by hex comparison):**
- Block 0 (0x000-0x1FF): 40-byte records ending with `ID` (0x4944) — OS identification. Same on all disks. ✓
- Block 1 (0x200-0x3FF): 30-byte records containing free block count (`0x0629` = 1577) and `OS` (0x4F53) marker. Critical for disk recognition.
- Block 2 (0x400-0x5FF): Master directory listing the 4 sub-directories. NOT the OS free block count location.
- Blocks 3-4: Additional OS structures ending with `DR` (0x4452) marker.
- Blocks 5-14: FAT (3 bytes/entry, 170 entries/block)
- Blocks 15-22: Sub-directories (4 dirs × 2 blocks, 39 entries × 26 bytes each)
- Block 22: Last reserved block — 511 of 512 bytes differ between our blank and the good `zeroed.img`

**`OS_FREE_COUNT_OFFSET` in image.rs is WRONG** — it points to `2 * BLOCK_SIZE = 0x400` (block 2, master directory). The hardware actually stores free block count in block 1. Writing to 0x400 was corrupting the master directory. Fixed by removing set_free_blocks() calls from CLI entirely.

**`type_info` field must be `0x0F`** — every directory entry on real hardware disks has type_info=0x0F. We were writing 0x00.

**`zeroed.img` in the repo root** is a hardware-formatted disk with one 60-patch bank written to it. It's confirmed good by the emulator. Its reserved blocks differ from our current blank_image.img in blocks 1, 4, 22 (and block 5 is FAT which differs because it has a file).

**The real blank_image.img fix needed:**
- We need a hardware-formatted BLANK (no files) disk image from the emulator to use as `blank_image.img`
- `zeroed.img` already has a file on it so can't be used directly
- User needs to format a fresh image with the emulator, save it without writing any files, and that becomes the new `blank_image.img`
- Alternatively: copy zeroed.img to blank_image.img (it has correct reserved blocks) and strip the FAT chain and directory entry — since our write operation will overwrite those anyway with correct data

**Actually viable shortcut:** Copy zeroed.img reserved blocks (0–22) into a fresh all-zeros image for blocks 23–1599. That gives a correct blank template. The FAT in blocks 5-14 on a blank disk must match what the emulator writes (currently unknown for zeroed.img since it has a file).

## Post-Mortem

### What Worked
- Binary diff with `cmp -l` to identify exactly which blocks differ — very effective
- `cmp -l | awk block grouping` to count differing bytes per block
- Removing `set_free_blocks()` from CLI was correct — the OS block location was wrong and was corrupting block 2

### What Failed
- Assumed `blank_image.img` was a valid hardware blank — it wasn't (had SYS-EX-FILE content in block 1)
- Replaced with `test_image1.img` (hardware blank from emulator) but that still had wrong reserved blocks vs `zeroed.img`
- Spent time debugging "BAD DEVICE ID" which turned out to be expected behavior for empty disks
- `OS_BLOCK_START = 2 * BLOCK_SIZE` was wrong; real free count is in block 1

### Key Decisions
- Removed `set_free_blocks()` entirely rather than fixing the offset — both `list` and `inspect` use FAT-derived counts anyway; the hardware doesn't maintain this field reliably
- `type_info = 0x0F` — observed from real hardware disk entries in `disk_with_everything.img`

## Artifacts

- `README.md` — top-level user manual (new)
- `crates/sd1cli/src/main.rs` — cmd_write, cmd_delete fixes
- `blank_image.img` — updated (but still wrong reserved blocks)
- `zeroed.img` — hardware-formatted disk with one file; correct reserved blocks but can't use directly as blank template

## Action Items & Next Steps

1. **Get a correct blank_image.img** — Two options:
   - **Option A (preferred):** Have user format a fresh disk with emulator, write nothing to it, save it as `blank_image.img`. Rebuild.
   - **Option B:** Take `zeroed.img` reserved blocks (blocks 0-22, bytes 0-11775) and combine with an all-zero block 23-1599 area (bytes 11776-819199). Use that as `blank_image.img`. Risk: the FAT blocks 5-14 within the reserved area of zeroed.img reflect a file being present; need to verify those are zeros on a fresh blank.

2. **Verify type_info=0x0F fix works** — test5.img has this fix plus the no-set_free_blocks fix. Once blank_image.img is fixed, create a new test image with this corrected blank and test in emulator.

3. **Understand block 22** — 511 of 512 bytes differ between our blank and zeroed.img. Need to see what's there on a hardware blank disk. It may be a critical structure the emulator checks on insert.

4. **Run cargo test** after blank_image.img update to ensure all 49 tests still pass. The `reserved_blocks_are_end_of_file` test checks FAT entries 0-22 are EndOfFile — verify this still holds.

5. **Commit all current fixes** once disk validation is confirmed working.

## Other Notes

**Test commands:**
```
cargo test                    # 49 tests must pass
cargo run -p sd1cli -- create /tmp/test.img
cargo run -p sd1cli -- write /tmp/test.img <some.syx>
cargo run -p sd1cli -- list /tmp/test.img
```

**Disk images in repo root:**
- `blank_image.img` — embedded template for `DiskImage::create()` — currently wrong
- `disk_with_everything.img` — 49 files, 5 free blocks — reference for correct directory entry format
- `zeroed.img` — hardware-formatted, one 60-patch bank, confirmed good by emulator
- `test_image1.img` — hardware-formatted blank (but may have wrong reserved blocks; used as current blank_image.img)

**SD-1 directory entry format (26 bytes):**
- Byte 0: type_info = **0x0F** (not 0x00!)
- Byte 1: file_type (0x0A=OneProgram, 0x0D=SixtyPrograms, etc.)
- Bytes 2-12: name (11 bytes, space-padded)
- Byte 13: _reserved = 0
- Bytes 14-15: size_blocks (u16 BE)
- Bytes 16-17: contiguous_blocks (u16 BE)
- Bytes 18-21: first_block (u32 BE)
- Byte 22: file_number
- Bytes 23-25: size_bytes (u24 BE)

**FAT encoding:** 3 bytes per entry, big-endian. Free=0x000000, EndOfFile=0x000001, BadBlock=0x000002, Next(n)=n.
