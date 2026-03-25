---
date: 2026-03-24T23:53:56Z
session_name: general
researcher: Claude
git_commit: c2b57be
branch: main
repository: sd1diskutil
topic: "SD-1 Combined SixtySequences+60Programs — type_info fix, patches-wrong next"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, allprograms, combined-format, type-info, directory-entry, programs-interleave]
status: in_progress
last_updated: 2026-03-24
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: type_info=0x2F fix landed; programs load but patches are wrong

## Task(s)

**COMPLETED: Fix error 192 on combined SixtySequences+60Programs file**
- Root cause found and fixed: directory entry `type_info` byte 0 had bit 5 (`0x20`) clear
- SD-1 uses this bit to decide whether seq data is at offset 11776 (no programs) or 44032 (60-programs)
- Without the bit the SD-1 read interleaved program data as sequences → error 192
- Fix: `crates/sd1cli/src/main.rs:350-356` — sets `type_info=0x2F` when `file_type==SixtySequences && embed_programs`
- File now loads in MAME emulator (no error 192). Committed `6ecbaf8`.

**IN PROGRESS: Programs load but track patches are wrong**
- User confirmed: "Loaded successfully but the patches assigned to the tracks are definitely not the right ones."
- Exact meaning: the sequences play, but each track uses an unexpected program (patch)
- Root cause unknown — see investigation notes below

## Critical References

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (authoritative)
- `crates/sd1disk/src/types.rs:206-296` — `allsequences_to_disk()` — combined format write
- `disk_with_everything.img` — reference disk; COUNTRY-* is a working 60-programs SixtySequences file

## Recent Changes

- `crates/sd1cli/src/main.rs:350-362` — `type_info` now set to `0x2F` (programs bit set) instead of hardcoded `0x0F`
- `tools/hybrid_programs_test.py` — new tool: splices reference programs into test file to isolate program-data vs structural issues

## Learnings

### The `type_info` byte is the 60-programs variant flag

Directory entry byte 0 (`type_info`) in SD-1 disk images:
- Bit 5 (`0x20`) = "programs are embedded in this sequence file"
- Lower nibble `0x0F` = SixtySequences identity bits (ThirtySequences uses `0x00`)
- Combined: `0x2F` = SixtySequences with embedded programs; `0x0F` = no programs

All four 60-programs SixtySequences files in `disk_with_everything.img` have `type_info=0x2F`.
CLASS-PNO-* (no-programs SixtySequences) has `type_info=0x0F`.
ThirtySequences with programs: `0x20`. ThirtySequences without: `0x00`.

**This is what the SD-1 checks to decide where seq data starts (11776 vs 44032).**

### Debugging methodology that worked

The "hybrid test" approach was definitive:
1. Write no-programs file → test in MAME (WORKS)
2. Write 60-programs file → test in MAME (error 192)
3. Hybrid: replace our programs section with COUNTRY-*'s known-good programs → still error 192
4. Conclusion: the error was structural (not the programs bytes themselves)
5. Compare directory entries → found `type_info` difference

### Programs section is structurally correct

Verified:
- Programs at 11776–43575 (31800 bytes, interleaved correctly)
- Zero padding at 43576–44031 (456 bytes)
- Seq data at 44032 (correct)
- De-interleaving our programs gives 60 valid ASCII program names
- Global section (11280–11300) identical structure to reference

### "Wrong patches" — possible causes to investigate

1. **SD-1 does not auto-load embedded programs into voice banks.** The SixtySequences file might load sequences but require a separate user action to also load/activate the embedded programs. The track parameters correctly reference slot numbers 0-59, but those slots in the keyboard's current memory have whatever programs were last loaded.

2. **Interleaving order scrambles program slots.** If `interleave_sixty_programs` produces a layout where the SD-1's "slot N" corresponds to a different program than the AllPrograms SysEx slot N, the patches would be wrong. Verify by: de-interleave our embedded programs section and check if slot 0's name matches the expected program 0 from the SysEx.

3. **Track parameter program numbers use a different encoding.** The track parameters (sequence header bytes 28–159) contain program numbers per track. These are written as-is from the SysEx headers. If the SysEx uses MIDI program numbers (1-indexed or bank-relative) while the disk programs section uses 0-indexed slots, there's an off-by-one or bank mismatch.

4. **The combined dump SysEx source was inconsistent.** If the AllPrograms and AllSequences packets came from different saves, the program references in sequences won't match the program data.

## Post-Mortem

### What Worked
- **Hybrid test tool** (`tools/hybrid_programs_test.py`): Swapping programs between reference and test file definitively separated "program data wrong" from "structural layout wrong"
- **Directory entry field-by-field comparison**: Revealed `type_info` difference between COUNTRY-* (`0x2F`) and our file (`0x0F`) in a single Python dump
- **Cross-referencing ALL SixtySequences/ThirtySequences files**: Confirmed the `0x20` bit pattern holds across ALL six sequence files, ruling out coincidence

### What Failed
- **Assumed no-programs worked → 60-programs would work structurally**: Was wrong; the file loaded for no-programs precisely because seq data is at 11776, which the SD-1 reads by default
- **Hybrid test expectation**: Expected hybrid (COUNTRY-* programs + our sequences) to load, confirming programs content was the issue. It didn't → led us to structural investigation
- **Checking global section and seq headers for the flag**: Neither contained the variant flag; it was in the directory entry metadata, not the file data itself

### Key Decisions
- **Test `type_info` fix in isolation**: Created `/tmp/nc12_fixed.img` with fresh write after fix, verified `type_info=0x2f` in directory, then handed to user for MAME testing
- **Kept `0x0F` as default**: Non-combined SixtySequences files still get `0x0F` (correct per spec); only programs-embedded case gets `0x2F`

## Artifacts

- `crates/sd1cli/src/main.rs:350-362` — `type_info` fix (commit `6ecbaf8`)
- `tools/hybrid_programs_test.py` — hybrid programs splice tool (commit `c2b57be`)
- `tools/compare_allseq_sysex_vs_disk.py` — earlier SysEx vs disk analysis tool
- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — authoritative spec

## Action Items & Next Steps

1. **Determine if SD-1 auto-loads programs from SixtySequences+programs file**
   - Load COUNTRY-* from `disk_with_everything.img` in MAME on a fresh state — do the COUNTRY programs auto-appear in memory?
   - If yes: our programs section must be wrong somehow
   - If no: the SD-1 requires manual program recall; this is a UI workflow issue not a format bug

2. **Verify program slot mapping after interleaving**
   - Extract our NC12NORTSEQ programs section (file offset 11776, 31800 bytes)
   - De-interleave with `deinterleave_sixty_programs()`
   - Compare slot 0's name to the first program in the AllPrograms SysEx — should match

3. **Check track parameter program numbers**
   - Look at sequence header bytes 28–38 (track 1 parameters), specifically the program number field
   - Verify these are 0-indexed slot numbers into the co-embedded programs table

4. **Write integration test for combined format** once patches issue resolved

## Other Notes

**Test SysEx file:**
```
/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-NC 12 North final (all).syx
```
4 packets: AllPrograms (31800 bytes, 60 programs) + AllPresets (960 bytes) + Command (skipped) + AllSequences (54927 bytes)

**Test disk with fix:** `/tmp/nc12_fixed.img` — confirmed loads in MAME without error 192

**Reference disk structure constants (confirmed):**
- FAT: block 5, 3-byte BE entries, EOF=0x000001
- SubDir0: block 15, 26-byte entries
- First data block: 23, total: 1600, block size: 512
- `type_info` for 60-programs SixtySequences: **must be 0x2F**, not 0x0F

**Directory entry field layout (26 bytes):**
- `[0]`: type_info — `0x2F` = 60-progs SixtySeq; `0x0F` = no-progs SixtySeq; `0x20` = 60-progs ThirtySeq; `0x00` = no-progs ThirtySeq
- `[1]`: file_type — `0x13` = SixtySequences
- `[2:13]`: 11-byte name
- `[14:16]`: size_blocks (BE u16)
- `[16:18]`: contiguous_blocks (BE u16)
- `[18:22]`: first_block (BE u32)
- `[22]`: file_number
- `[23:26]`: size_bytes (BE 3-byte)

**Test count:** All 54 tests pass (43 unit + 11 integration)
