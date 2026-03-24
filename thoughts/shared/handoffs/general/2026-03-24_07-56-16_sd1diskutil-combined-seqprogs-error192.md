---
date: 2026-03-24T07:56:16Z
session_name: general
researcher: Claude
git_commit: 55a1625
branch: main
repository: sd1diskutil
topic: "SD-1 Combined SixtySequences+60Programs disk layout — error 192 on load"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, allprograms, combined-format, error-192]
status: in_progress
last_updated: 2026-03-24
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Combined SixtySequences+60Programs write — error 192 on emulator load

## Task(s)

**COMPLETED: Integration tests for AllSequences write pipeline**
- Two new integration tests in `crates/sd1disk/tests/operations_tests.rs` (commit `55a1625`)
- `allsequences_write_then_list_finds_file_with_correct_size` — write→list→verify size/blocks/type
- `allsequences_write_verify_seq_data_on_disk` — confirms seq bytes at offset 11776, zero padding

**IN PROGRESS: Combined SixtySequences+60Programs disk format**
- Root cause identified: when a SysEx dump contains both AllPrograms and AllSequences, the SD-1
  disk format requires the programs to be **embedded inside the SixtySequences file** (not written
  as a separate SixtyPrograms file). The combined layout ("60 Programs" variant) places programs
  at offset 11776 and pushes sequence data to offset 44032.
- Code changes implemented and committed but **loading still gives error 192 in MAME emulator**.
  The structural approach is correct per the Giebler spec; something in the layout details is off.

## Critical References

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (authoritative)
- `crates/sd1disk/src/types.rs:206` — `allsequences_to_disk()` (signature changed this session)
- `disk_with_everything.img` — reference disk; COUNTRY-* is a SixtySequences+60Programs file

## Recent Changes

- `crates/sd1disk/src/types.rs:206-295` — `allsequences_to_disk()` now accepts
  `interleaved_programs: Option<&[u8]>`; when `Some`, embeds 31800 bytes of programs at offset
  11776 and moves sequence data to offset 44032
- `crates/sd1cli/src/main.rs:233-270` — write command detects AllPrograms+AllSequences pair;
  pre-interleaves programs, skips writing AllPrograms as a separate file, passes interleaved
  bytes to `allsequences_to_disk`
- `crates/sd1disk/tests/operations_tests.rs` — all `allsequences_to_disk` call sites updated
  to pass `None` (committed `55a1625`)

Note: the combined-format CLI changes are **uncommitted**. Commit before continuing.

## Learnings

### Combined dump format: programs embedded in SixtySequences file

The Giebler spec defines two SixtySequences layouts:
- **No Programs**: seq data at offset 11776
- **60 Programs**: programs at 11776–43575, zeros at 43576–44031, seq data at 44032

When the SD-1 does a full dump (AllPrograms + AllPresets + AllSequences), the disk file should
be the "60 Programs" variant — one SixtySequences file with programs inside, plus a separate
TwentyPresets file. Track patch assignments in sequence headers (bytes 28–159) reference program
slot numbers in the co-embedded program table.

### Confirmed by reference disk

`disk_with_everything.img` COUNTRY-* (SixtySequences, 58983 bytes) starts sequence data at file
offset 44032, confirming it is the 60-Programs variant. This is what our code now tries to produce.

### Error 192 still occurring

After writing the combined file (`NC12NORTSEQ`, 90112 bytes, 176 blocks), the MAME emulator
still gives error 192 ("sequencer memory corrupt") on load. The file size is plausible:
- 44032 (header+programs+padding) + padded sequence data ≈ 90112 bytes
- But the exact padded layout may still be wrong

### Possible causes of error 192 to investigate

1. **Programs are interleaved but reference disk programs may NOT be interleaved in this layout.**
   The no-programs SixtyPrograms file uses interleaving (even/odd byte split). But the programs
   embedded in the SixtySequences file — do they use the same interleaving? The Giebler spec
   says "60 Programs (530 bytes each — mixed together)." "Mixed together" might mean a different
   layout than the standalone SixtyPrograms file. Need to check against reference disk.

2. **Sequence data offset 44032 but block boundary check.** 44032 / 512 = 86 (clean block
   boundary), so that's not the issue. But the `size_sum` field in the global section still
   encodes the unpadded seq data length — verify this hasn't changed accidentally.

3. **The 60-programs layout may expect uninterleaved (straight) program data**, not the interleaved
   format used by the standalone SixtyPrograms file. This is the most likely candidate.

### How to verify programs layout in reference disk

Extract the programs section from COUNTRY-* (offset 11776, length 31800) and compare it to
what `interleave_sixty_programs` produces vs the raw AllPrograms SysEx payload. The reference
disk is the ground truth.

```
# Extract programs section from reference COUNTRY-* file
# First extract the file from disk_with_everything.img (block 1360, 121 blocks = 61952 bytes)
# Then look at bytes 11776–43575
```

The `tools/inject_reference_seq.py` extracts files by name; use it to get the raw bytes.

### Track parameter format (sequence headers, bytes 28–159)

12 tracks × 11 bytes each. These bytes include program number, MIDI channel, volume, pan per
track. The program number here is the slot index (0–59) into the co-embedded program table.
These are written as-is from the SysEx headers — we don't modify them.

## Post-Mortem

### What Worked

- Detecting the combined format in the CLI by checking for AllPrograms+AllSequences co-presence
- Absorbing AllPrograms into AllSequences write (no separate file) matches the SD-1 disk spec
- File size math: 44032 + padded_seq_data = 90112 bytes (176 blocks) looks correct
- All 54 tests pass after the signature change

### What Failed

- Loading the combined file in MAME still gives error 192 — the structural approach is right
  but a detail in the programs section layout is likely wrong (interleaved vs. raw)

### Key Decisions

- **Optional parameter on `allsequences_to_disk`**: `Option<&[u8]>` keeps the no-programs path
  unchanged and backwards-compatible; all existing call sites pass `None`
- **Interleave before passing**: CLI interleaves programs (same as standalone SixtyPrograms)
  before passing to `allsequences_to_disk` — but this may be wrong for the combined layout
- **Skip AllPrograms in write loop when combining**: clean, no separate file written

## Artifacts

- `crates/sd1disk/src/types.rs:206-295` — `allsequences_to_disk()` with programs support
- `crates/sd1cli/src/main.rs:233-270` — combined write detection logic
- `crates/sd1disk/tests/operations_tests.rs:241-342` — AllSequences integration tests
- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec reference
- `tools/inject_reference_seq.py` — extract named file from reference disk (use to get raw programs bytes)
- `tools/compare_allseq_sysex_vs_disk.py` — SysEx vs disk binary analysis tool

## Action Items & Next Steps

1. **Commit uncommitted changes** — `types.rs`, `main.rs` (combined format + signature change)
2. **Verify programs layout in reference disk** — extract COUNTRY-* from `disk_with_everything.img`,
   read bytes 11776–43575, compare against:
   a. Raw AllPrograms SysEx payload (denybblized, no interleaving)
   b. `interleave_sixty_programs()` output
   Whichever matches is what the combined layout expects.
3. **Fix programs embedding if needed** — if reference shows raw (non-interleaved) programs,
   pass the raw SysEx payload directly instead of the interleaved output
4. **Re-test in MAME emulator**
5. **Write integration test for combined format** — similar to existing AllSequences tests
   but with a synthetic AllPrograms payload and `Some(interleaved)` argument

## Other Notes

**Test file used this session:**
```
/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-NC 12 North final (all).syx
```
4 packets: AllPrograms (31800 bytes, 60 programs) + AllPresets (960 bytes) + Command (skipped) + AllSequences (54927 bytes)

**Output with combined fix (before error 192):**
```
Written: NC12NORTPST (960 bytes, 2 block(s))
Written: NC12NORTSEQ (90112 bytes, 176 block(s))
```

**Reference disk COUNTRY-* layout (confirmed working):**
- Block 1360, 121 FAT blocks (61952 bytes), directory claims 58983 bytes
- File offset 44032: sequence data starts (= 60-programs variant confirmed)
- 8 defined sequences, block-padded strides

**Disk structure constants (confirmed):**
- FAT: block 5, 3-byte BE entries, EOF=0x000001
- SubDir0: block 15, 26-byte entries
- First data block: 23, total: 1600, block size: 512

**Test count:** 54 total (43 unit + 11 integration), all passing
