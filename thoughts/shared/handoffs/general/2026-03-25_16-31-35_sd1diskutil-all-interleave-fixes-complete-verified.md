---
date: 2026-03-25T23:31:35Z
session_name: general
researcher: Claude
git_commit: 2d7224b
branch: main
repository: sd1diskutil
topic: "SD-1 sixty-programs interleave fix — all components verified complete"
tags: [rust, ensoniq, sd-1, disk-image, interleave, programs, sysex, dump-programs, python]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: All interleave fixes complete and verified across multiple SysEx datasets

## Task(s)

**COMPLETED: Resume from prior handoff and verify regenerated disk image**
- Prior session fixed Rust interleave/deinterleave and `decode_b10`; this session verified the output
- Confirmed KOTO-DREAMS at b10=32 (INT bank 5 patch 2) — verified against hardware

**COMPLETED: Fix `deinterleave_sixty_programs` in `dump_programs.py`**
- Python mirror of the Rust function was never updated when Rust was fixed
- Was using old alternating-pairs algorithm: slot k = even chunk k//2 (k even) or odd chunk k//2 (k odd)
- Fixed to simple concatenation: even stream → progs 0–29, odd stream → progs 30–59
- Fixed in commit 2d7224b

**COMPLETED: Verify against second independent SysEx dataset (NC 12 North)**
- Wrote `seq-NC 12 North final (all).syx` to a fresh disk image
- All 60 custom patches verified — disk de-interleave matches SysEx exactly

## Critical References

- `crates/sd1disk/src/types.rs:153-167` — `interleave_sixty_programs` (correct)
- `crates/sd1disk/src/types.rs:302-318` — `deinterleave_sixty_programs` (correct)
- `tools/dump_programs.py:163-174` — `deinterleave_sixty_programs` (now correct)

## Recent Changes

- `tools/dump_programs.py:163-174` — replaced alternating-pairs de-interleave with `even_stream + odd_stream` concatenation

## Learnings

### The Python deinterleave bug
The Python `deinterleave_sixty_programs` function was written to mirror the OLD (wrong) Rust algorithm. When Rust was fixed in a prior session, the Python was not updated. The old Python code produced an alternating output: slot 0 = even_data[0:530], slot 1 = odd_data[0:530], slot 2 = even_data[530:1060], etc. This placed KOTO-DREAMS (SysEx slot 32, odd stream index 2) at Python output slot 5.

### How to diagnose future de-interleave discrepancies
If `dump_programs.py` shows programs at unexpected slots: first verify with a raw Python snippet extracting `even_stream + odd_stream` directly from the file bytes at `PROGRAMS_OFFSET`. If that gives correct names, the display function is wrong; if not, the Rust interleave is wrong.

### File size with vs without embedded programs
- Without programs: FSEQ file = 11776 + padded_seq_total bytes
- With programs: FSEQ file = 44032 + padded_seq_total bytes
- Two different SysEx files can produce same total file size if their padded_seq_total values differ by 32256. Don't use file size alone to confirm programs are embedded — use `dump_programs.py` or raw byte inspection.

### Hardware confirmation
KOTO-DREAMS confirmed on hardware at INT bank 5 patch 2 = b10=32. This is the canonical reference point for verifying correct interleave output.

## Post-Mortem

### What Worked
- Raw byte inspection via inline Python script to verify programs were correctly embedded before diagnosing the display tool
- Running `dump_programs.py` with the SysEx comparison argument (`--syx`) to get immediate slot-by-slot diff
- Using a second independent SysEx dataset (NC 12 North) with many custom patches as a cross-check

### What Failed
- Initial `dump_programs.py` run appeared to confirm programs at wrong positions — misleading because the display function itself was buggy, not the data
- File size comparison between old and new images was inconclusive due to different sequence data sizes between the two SysEx files

### Key Decisions
- Decision: Fix Python `deinterleave_sixty_programs` to match current Rust (concatenation), not the old Rust (alternating pairs)
  - Reason: The Rust is the authoritative implementation; Python is a display/analysis tool that must mirror it exactly

## Artifacts

- `tools/dump_programs.py:163-174` — corrected `deinterleave_sixty_programs`
- `thoughts/shared/handoffs/general/2026-03-25_16-06-26_sd1diskutil-interleave-bug-fixed-programs-at-correct-bank-patch.md` — prior session handoff (context on Rust fixes)

## Action Items & Next Steps

The interleave fix chain is fully complete and verified. No outstanding bugs in this area.

Possible future work:
1. **MAME verification** — load a generated disk image in MAME to visually confirm the bank/patch display matches expectations (optional; software verification is already thorough)
2. **Hardware round-trip test** — write a generated disk image to physical floppy and load on real SD-1 hardware
3. **Extract round-trip test** — write programs to disk, extract back to SysEx, compare against original AllPrograms payload byte-for-byte

## Other Notes

**SysEx files verified this session:**
- `seq-DB final (all).syx` — all 60 slots match; KOTO-DREAMS at slot 32 ✓
- `seq-NC 12 North final (all).syx` — all 60 slots match; all custom patches correct ✓

**SysEx library location:**
- `/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/`
- Each `*final (all).syx` file has 4 packets: AllPrograms, AllPresets, Command, AllSequences
- Command packet is filtered by the write command; AllPrograms is embedded into AllSequences on disk

**Test command pattern:**
```
cargo run -- create /tmp/test.img
cargo run -- write /tmp/test.img <sysex>
python3 tools/dump_programs.py /tmp/test.img "<FILENAME>" <sysex>
```
Look for "All 60 slots match!" at the end.

**Commit history for this fix chain:**
- `41cbc53` — Rust interleave: first-30/last-30 split
- `c3b88da` — decode_b10: direct lookup + analysis tools
- `2d7224b` — Python deinterleave: concatenation to match Rust
