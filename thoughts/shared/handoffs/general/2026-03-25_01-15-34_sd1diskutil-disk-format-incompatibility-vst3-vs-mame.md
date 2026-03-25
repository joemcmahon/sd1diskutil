---
date: 2026-03-25T08:15:34Z
session_name: general
researcher: Claude
git_commit: 31c716f
branch: main
repository: sd1diskutil
topic: "SD-1 disk format incompatibility between VST3 and MAME/Rust; interleave investigation continues"
tags: [rust, ensoniq, sd-1, disk-image, sysex, interleave, vst3, mame, directory, disk-format]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: VST3 disk format incompatible with our Rust format; disk_with_everything.img is the right ground truth

## Task(s)

**RESUMED FROM: Prior handoff — interleave_sixty_programs is wrong, programs land at wrong INT bank/patch**

**COMPLETED: Attempted VST3 ground-truth approach**
- Created /tmp/db_rebuild.img with `cargo run -- create` + `cargo run -- write` (FPST + FSEQ from DB final SysEx)
- User mounted in Ableton Live / SD-1 VST3 plugin; sequences loaded correctly; patches loaded from SysEx
- User saved combined sequences+patches as "SEQ-DB FSEP" and "SEQ-DB FSE0" to the image
- Result saved to ~/Downloads/db_rebuild.img

**COMPLETED: Discovered VST3 disk format is incompatible with ours**
- Our Rust code puts directory at block 15; VST3 puts directory at block 1
- `cargo run -- list` cannot see VST3-written files (FSEP, FSE0) — only finds block-15 entries
- VST3 CAN read our block-15 format (sequences loaded successfully)
- VST3 writes new entries at block 1 alongside our block-15 entries
- This explains why `list` showed garbled output on VST3-formatted images

**COMPLETED: Ruled out VST3 as direct byte-comparison ground truth**
- The two disk formats (VST3 block-1 vs our block-15) differ in 710K+ bytes throughout the image
- Cannot simply diff "programs section" between VST3 and our images at the same offsets
- The db-good.img we created earlier showed `6d b6` filler at the expected programs location
- db-good.img programs section was all filler — confirming completely different layout

**OPEN: Correct interleave still unknown**
**OPEN: Fix interleave_sixty_programs in Rust**
**OPEN: Fix deinterleave_sixty_programs in Rust**
**OPEN: Fix decode_b10 in dump_programs.py**

**KEY DISCOVERY (end of session): disk_with_everything.img is right here in the repo root**
- `cargo run -- list ./disk_with_everything.img` works and lists 49 files correctly
- Contains "SD1-INT" — SixtyPrograms (63 blocks, 31800 bytes) — the canonical INT0 bank
- Contains "COUNTRY-*" — SixtySequences (no programs) — previously used as reference
- This is the right ground truth for figuring out the interleave

## Critical References

- `crates/sd1disk/src/types.rs:149-168` — `interleave_sixty_programs` (WRONG — needs fix)
- `crates/sd1disk/src/types.rs:300-315` — `deinterleave_sixty_programs` (WRONG — needs fix)
- `tools/dump_programs.py:285` — `decode_b10` (WRONG formula — needs fix)
- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (authoritative)

## Recent Changes

No code changes this session. All findings are diagnostic/investigative.

Disk images created/modified this session:
- `/tmp/db_rebuild.img` — our clean Rust-written image (FPST + FSEQ only, no programs)
- `~/Downloads/db_rebuild.img` — VST3-modified version (has FSEP+FSE0 added at block 1)
- `/tmp/db-good.img` — VST3-formatted fresh image (confirmed different format, all filler at programs offset)

## Learnings

### VST3 disk format is incompatible with ours
- VST3 writes directory entries at block 1, offset 0x20 (32 bytes into block 1)
- Our Rust code writes directory at block 15 (`SUBDIR_START_BLOCK = 15` in `crates/sd1disk/src/directory.rs:8`)
- MAME reads our block-15 format successfully
- VST3 reads our block-15 format successfully
- VST3 writes NEW entries to block 1 (doesn't use block 15 for writes)
- `cargo run -- list` only scans block 15 → cannot see VST3-written files
- `tools/dump_programs.py` has same limitation (`SUBDIR_START_BLOCK = 15`)

### VST3 directory entry format
The block-1 entries use the filename-first layout (11 bytes name at e[0:11]), not our layout (type_info at e[0], file_type at e[1], name at e[2:13]). Size bytes (3-byte BE) appear to be at rest[10:13] (after the 11-byte name). Start block appears at rest[2:4] as little-endian u16.

### disk_with_everything.img is the right ground truth
This file was previously used as a reference but forgot about in later sessions. It lives at `./disk_with_everything.img` (repo root). It was readable by `cargo run -- list`. Key files:
- **SD1-INT** — SixtyPrograms, 31800 bytes — this is 60 programs in CORRECT order
- **COUNTRY-*** — SixtySequences — has programs embedded (used in earlier sessions)
- Various ROM banks, presets, etc.

### Recommended approach for fixing interleave
1. Extract SD1-INT from disk_with_everything.img using `cargo run -- extract`
2. Load the INT0 programs from the SD-1 AllPrograms SysEx (known-good source)
3. De-nybblize the SysEx to get the raw 60×530 bytes
4. Compare: does SD1-INT match the raw SysEx in direct/sequential order?
5. If so: the correct "interleave" is NO INTERLEAVE — programs stored sequentially
6. Alternatively: compare SD1-INT against what our current interleave_sixty_programs produces
7. The diff tells us the correct mapping

### The b10 formula in dump_programs.py
The handoff says the fix is simple: change `disk_programs[deint_slot]` → `disk_programs[b10]` at line 285 (after fixing the interleave, de-interleave gives programs in SysEx order, so direct indexing works).

### PROGRAM_NAME_OFFSET = 498 may be correct
The dump_programs.py tool has this constant. The garbage output when scanning programs in this session was because `/tmp/db_final.img`'s programs section actually contained SEQUENCE EVENT DATA (the FSEQ no-programs format puts sequence data at offset 11776, same as where programs go in the with-programs format). The Downloads/db_rebuild.img FSEP file may have the correct data — but reading it requires handling the block-1 directory.

### db_final.img confusion
`/tmp/db_final.img` was originally our Rust-written image from last session (FPST + FSEQ, no programs embedded). But earlier in the session it showed garbled output from `cargo run -- list` — this was because the VST3 had loaded and partially reformatted it in a previous session. It's not a clean image.

## Post-Mortem

### What Worked
- **Rebuilding db_rebuild.img**: `cargo run -- create + write` produced a clean image the VST3 could read
- **VST3 loaded sequences correctly**: Confirms our FSEQ format is MAME-compatible
- **cargo run -- list ./disk_with_everything.img**: Works perfectly, 49 files, canonical reference is right here

### What Failed
- **VST3 as byte-comparison ground truth**: VST3 uses block-1 directory; our images use block-15 directory; the raw images are incompatible for direct byte comparison
- **db-good.img as ground truth**: The VST3-formatted image had `6d b6` filler everywhere at the expected programs offset
- **Scanning program names at NAME_OFFSET=498**: This offset is correct in theory, but the section we were reading was actually sequence event data masquerading as programs

### Key Decisions
- Decision: Do NOT attempt to fix interleave based on VST3 disk comparison
  - Reason: VST3 writes a fundamentally different disk format (block-1 directory) that can't be byte-compared to our format directly
- Decision: Use disk_with_everything.img SD1-INT as ground truth instead
  - Reason: It's a SixtyPrograms file in our own format, readable by our tools, and contains the canonical INT0 program bank

## Artifacts

- `./disk_with_everything.img` — canonical reference disk (49 files, readable by our tools)
- `/tmp/db_rebuild.img` — clean Rust-written image (FPST + FSEQ for DB final)
- `~/Downloads/db_rebuild.img` — VST3-modified version (FSEP+FSE0 added at block 1, timestamp 00:25)
- `crates/sd1disk/src/types.rs:149-168` — interleave_sixty_programs (needs fix)
- `crates/sd1disk/src/types.rs:300-315` — deinterleave_sixty_programs (needs fix)
- `tools/dump_programs.py:285` — decode_b10 (simple fix: use b10 directly)

## Action Items & Next Steps

1. **Extract SD1-INT from disk_with_everything.img**
   ```
   cargo run -- extract ./disk_with_everything.img "SD1-INT" /tmp/sd1_int.syx
   ```
2. **De-nybblize SD1-INT syx to get raw 31800 bytes** — compare against our interleave_sixty_programs output on the same source programs
3. **Figure out the correct interleave** by comparing:
   - SD1-INT raw bytes (correct order as stored on a reference disk)
   - vs. what our current interleave_sixty_programs produces from the same source
4. **Fix interleave_sixty_programs** in `crates/sd1disk/src/types.rs:149-168`
5. **Fix deinterleave_sixty_programs** in `crates/sd1disk/src/types.rs:300-315` (exact inverse)
6. **Fix decode_b10** in `tools/dump_programs.py:285` — change `disk_programs[deint_slot]` to `disk_programs[b10]`
7. **Run 54 tests** (43 unit + 11 integration) — some interleave tests will likely fail and need updating
8. **Regenerate disk images** and verify in MAME

## Other Notes

**What disk_with_everything.img contains (full listing):**
- 12 OneProgram files (individual programs)
- 11 SixPrograms files (6-program banks)
- 1 ThirtyPrograms file
- 8 SixtyPrograms files: SD1-INT, SD1-ROM-0, SD1-ROM-1, VFXSD-INT, KEYS 1-60, VSD-1000-C, ADD-VPC-100, ADD-VPC-101
- 4 TwentyPresets files
- Sequences: COUNTRY-*, SD1-PALETTE, PETROUCHKA*, ROCK-BEATS, SWING+SHUFL, etc.
- VERSION-)10 (SequencerOs)

**SysEx file locations:**
- All Shatterday SysEx: `/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/`
- DB final: `seq-DB final (all).syx` (175,412 bytes)
- All 8 files: AllPrograms (0x03) + AllPresets (0x05) + Command (0x00) + AllSequences (0x0A)

**Test count:** 54 tests (43 unit + 11 integration)
**Disk write CLI:** `cargo run -- create <image>` then `cargo run -- write <image> <sysex>`
**Programs section:** bytes 11776–43575 within a SixtySequences+Programs file

**"Four-and-five problem" (user's term):** This likely refers to the physical sector interleave on Ensoniq floppies. The VST3/hardware uses a different sector ordering than our linear layout. However, since MAME reads our linear layout correctly, this is NOT blocking — it only affects VST3 compatibility, which is a secondary concern.

**block-1 directory format (VST3):** Entries at `512 + 0x20 + i*26` bytes. Name at e[0:11] (not e[2:13] like our format). Start block at e[13:15] as little-endian u16. Size bytes (24-bit BE) at e[21:23].
