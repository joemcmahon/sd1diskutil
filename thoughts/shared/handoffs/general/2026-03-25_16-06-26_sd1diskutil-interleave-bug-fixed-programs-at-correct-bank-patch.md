---
date: 2026-03-25T23:06:26Z
session_name: general
researcher: Claude
git_commit: c3b88da
branch: main
repository: sd1diskutil
topic: "SD-1 sixty-programs interleave bug fixed; programs now land at correct b10 positions"
tags: [rust, ensoniq, sd-1, disk-image, interleave, programs, sysex, dump-programs]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Interleave bug fixed — programs now at correct bank/patch positions

## Task(s)

**COMPLETED: Commit VST3 block-1 directory fix**
- All prior session's changes committed (5b2b405, 9b2563e)

**COMPLETED: Fix `interleave_sixty_programs` in Rust**
- Root cause confirmed: was using even/odd-indexed programs; hardware expects first-30/last-30 split
- Fixed in `crates/sd1disk/src/types.rs` (commit 41cbc53)

**COMPLETED: Fix `deinterleave_sixty_programs` in Rust**
- Exact inverse of corrected interleave: even bytes → progs 0–29, odd bytes → progs 30–59
- Fixed in same commit (41cbc53)

**COMPLETED: Fix `decode_b10` in `tools/dump_programs.py`**
- Was using wrong formula `b10*2` or `(b10-30)*2+1`; correct is `disk_programs[b10]` directly
- Fixed in `tools/dump_programs.py` (commit c3b88da)

**OPEN: Regenerate disk images and verify in MAME**
- Need to re-run the disk image generation with the fixed interleave
- Verify that programs appear at the correct bank/patch positions in MAME

## Critical References

- `crates/sd1disk/src/types.rs:149-168` — `interleave_sixty_programs` (NOW CORRECT)
- `crates/sd1disk/src/types.rs:300-320` — `deinterleave_sixty_programs` (NOW CORRECT)
- `tools/dump_programs.py:278-290` — `decode_b10` (NOW CORRECT)

## Recent Changes

- `crates/sd1disk/src/types.rs:149-168` — interleave: even_data = payload[0..15900], odd_data = payload[15900..31800]
- `crates/sd1disk/src/types.rs:300-320` — deinterleave: result = even_bytes || odd_bytes (concat, no slot rearrangement)
- `tools/dump_programs.py:284-287` — decode_b10: removed deint_slot formula, uses `disk_programs[b10]` directly
- `tools/analyze_interleave.py` — new analysis tool that derived the correct mapping from VST3 ground truth
- `tools/compare_allseq_sysex_vs_disk.py` — new tool for comparing AllSequences SysEx vs on-disk sequence data

## Learnings

### The correct interleave structure
The SD-1 on-disk SixtyPrograms format byte-interleaves two 15900-byte streams:
- Even byte positions (0, 2, 4, …): programs 0–29 concatenated (b10 = 0–29)
- Odd byte positions (1, 3, 5, …): programs 30–59 concatenated (b10 = 30–59)

Within each 1060-byte pair k: even bytes = program k, odd bytes = program k+30.

The hardware de-interleaves by: even stream slot k → b10=k, odd stream slot k → b10=k+30.

### How the bug was diagnosed
The VST3 plugin stores programs in a DIFFERENT byte format than SysEx AllPrograms (name at offset 242 on disk vs 498 in SysEx). This meant direct byte comparison failed. The correct approach was:
1. Extract even/odd streams from the VST3 disk data
2. Search for SysEx program NAMES within each stream
3. The name positions revealed: even slot k = sysex[k], odd slot k = sysex[k+30]

### VST3 vs hardware byte format
- Hardware-written disks (and our Rust code): SysEx format, program name at byte offset 498 within each 530-byte program
- VST3-written disks: native format, program name at byte offset 242
- These are different byte layouts of the same program data
- Our Rust code correctly uses SysEx format — only the ordering was wrong

### Why old decode_b10 formula seemed to work
The formula `deint_slot = b10*2 if b10<30 else (b10-30)*2+1` was derived against the WRONG interleave output. It happened to find the right program name by coincidence for the COUNTRY-* test case (both interleave and lookup were wrong in complementary ways). After fixing the interleave, the correct lookup is `disk_programs[b10]`.

### All 57 tests still pass after the fix
The existing roundtrip tests (interleave → deinterleave = identity) pass because both functions were updated consistently. No test needed changing.

## Post-Mortem

### What Worked
- **Name-based search rather than byte comparison**: Searching for 11-byte program names in the even/odd streams worked even when full 530-byte programs didn't match (due to format difference between SysEx and VST3 native)
- **VST3 disk as ground truth for ordering**: Even though VST3 uses different byte format, the bank/patch ordering is correct and derivable from name positions
- **Python analysis script (`tools/analyze_interleave.py`)**: Systematically tested hypotheses (simple concat, byte interleave, name search) and produced a definitive answer
- **Simulating hardware de-interleave in Python**: Confirmed the fix by showing what the hardware would see at each b10 position with old vs new interleave

### What Failed
- **Direct byte comparison with VST3 disk**: Failed because VST3 uses different program byte layout (names at offset 242, not 498). Led to 24816/31800 byte differences even with correct ordering
- **Simple concatenation hypothesis**: Programs don't appear as contiguous 530-byte chunks in the disk data
- **Assuming VST3 format = SysEx format**: Had to discover they're different layouts of the same parameters

### Key Decisions
- Decision: Fix interleave to first-30/last-30, derive from evidence (VST3 ground truth)
  - Reason: Two previous sessions guessed wrong; the only reliable approach was to extract the actual mapping from the known-good VST3 disk
- Decision: Keep SysEx-format bytes on disk (name at offset 498)
  - Reason: Hardware disks also use this format; converting to VST3 native format would break hardware compatibility and wasn't needed
- Decision: `disk_programs[b10]` direct lookup in decode_b10
  - Reason: After correct deinterleave, the output is programs 0–59 in natural order, so indexing by b10 directly is correct

## Artifacts

- `crates/sd1disk/src/types.rs:149-168` — corrected `interleave_sixty_programs`
- `crates/sd1disk/src/types.rs:300-320` — corrected `deinterleave_sixty_programs`
- `tools/dump_programs.py:278-290` — corrected `decode_b10`
- `tools/analyze_interleave.py` — analysis tool (new)
- `tools/compare_allseq_sysex_vs_disk.py` — sequence comparison tool (new)

## Action Items & Next Steps

1. **Regenerate `db_rebuild.img`** using the fixed interleave:
   ```
   cargo run -- write-programs ~/path/to/seq-DB final (all).syx output.img
   ```
   (or whatever the actual command is)
2. **Verify in MAME**: Load the regenerated disk image and confirm programs appear at the correct bank/patch positions — specifically verify KOTO-DREAMS is at INT 6 patch 3 (b10=32), not INT 3 patch 5 (b10=16)
3. **Run `dump_programs.py`** against the regenerated disk to confirm decode_b10 now reports correct program names
4. **Consider testing with actual SD-1 hardware** if available

## Other Notes

**Disk image locations:**
- `~/Downloads/db_rebuild.img` — VST3-modified (FSEP at block 203) — ground truth for interleave ordering
- `./disk_with_everything.img` — reference disk (49 files, hardware-written, block-15 format) — confirms SysEx byte format on disk
- `/tmp/db_rebuild.img` — clean Rust-written (FPST + FSEQ only, no programs)

**SysEx source:**
- `/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-DB final (all).syx`
- Packet 0 = AllSequences (0x0A), Packet 1 = AllPrograms (0x03)
- AllPrograms: 31800 bytes = 60 × 530, name at offset 498 within each program

**b10 encoding (unchanged, correct):**
- 0x00–0x3B: RAM program, b10 = bank×6 + patch (0-indexed)
- 0x80–0xFE: ROM program
- 0x7F: no program change; 0xFF: track inactive

**Test count:** 57 tests (46 unit + 11 integration) — all passing after all fixes.
