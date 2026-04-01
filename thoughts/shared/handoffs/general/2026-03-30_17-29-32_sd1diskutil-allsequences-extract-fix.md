---
date: 2026-03-30T17:29:32-05:00
session_name: general
researcher: Claude
git_commit: 4b970e0
branch: main
repository: sd1diskutil
topic: "SixtySequences Extract Fix + Sojus MAME Off-by-One Bug Discovery"
tags: [implementation, sequences, extract, sysex, round-trip, mame-bug]
status: complete
last_updated: 2026-03-30
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: AllSequences extract fix + Sojus VST3 MAME bug discovery

## Task(s)

1. **AllSequences (SixtySequences) extract bug — COMPLETED**
   `cmd_extract` was incorrectly wrapping raw on-disk `SixtySequences` bytes in a
   `SingleSequence` SysEx header, producing a ~2× oversized file. Root cause: no inverse
   of `allsequences_to_disk()` existed. Implemented `disk_to_allsequences()` and wired it in.

2. **Round-trip verification with "Ascention Island alternate take.syx" — COMPLETED**
   Write → extract produces all headers, global section, and 26,808 bytes of event data
   matching the original byte-for-byte.

3. **Sojus VST3 emulator MAME off-by-one bug — DOCUMENTED, awaiting fix**
   Confirmed with Sojus developers: their VST3 plugin uses MAME's `get_track_data_mfm_pc`
   which expects PC-standard sectors 1–10, but Ensoniq uses sectors 0–9. When saving a
   `.img`, sector 0 is silently discarded and each track's data is shifted one sector,
   zeroing the last sector per track. This corrupts every `.img` file written by the VST3
   plugin. `.hfe` files are unaffected (raw MFM flux, bypasses sector extraction).
   Sojus is preparing a patched `esq16_dsk.cpp` for MAME.

## Critical References

- `crates/sd1disk/src/types.rs` — all disk↔SysEx conversion functions
- `crates/sd1cli/src/main.rs:396-450` — `cmd_extract`, including the new `SixtySequences` arm

## Recent changes

- `crates/sd1disk/src/types.rs:294-364` — new `disk_to_allsequences()` function (inverse of `allsequences_to_disk()`)
- `crates/sd1disk/src/types.rs:520-574` — two new tests: `disk_to_allsequences_round_trips_via_disk`, `disk_to_allsequences_rejects_short_disk`
- `crates/sd1disk/src/lib.rs:17` — export `disk_to_allsequences`
- `crates/sd1cli/src/main.rs:6` — import `disk_to_allsequences`
- `crates/sd1cli/src/main.rs:399-424` — `cmd_extract` now tracks `use_contiguous` (VST3 block-1 vs FAT) and uses `disk_to_allsequences` for `SixtySequences`
- `crates/sd1cli/src/main.rs:430-441` — split `SixtySequences` from `OneSequence | ThirtySequences`; `SixtySequences` uses `disk_to_allsequences` + `AllSequences` type, checks `entry.type_info & 0x20` for embedded programs

## Learnings

### Sojus VST3 MAME off-by-one bug (CRITICAL)
- Any `.img` file written by the VST3 plugin is corrupted: sector 0 of every track is
  discarded, data shifts one position within each track, and the last sector per track
  is zeroed.
- Per-track effect: on a 2DD disk (160 tracks), blocks 0, 10, 20, … contain wrong data
  and blocks 9, 19, 29, … are zeroed.
- `.hfe` files are NOT affected.
- `sd1diskutil`'s own write path is NOT affected (we write raw binary, not through MAME).
- A round-trip of `sd1diskutil write → sd1diskutil extract` is completely clean.
- Sojus confirmed the bug and is releasing a fixed `esq16_dsk.cpp`.

### AllSequences on-disk format
- `disk_to_allsequences()` must know `has_programs` (from `entry.type_info & 0x20`) to
  pick the correct sequence data offset: `11776` without programs, `44032` with.
- Per-sequence block padding: each sequence's `ds` bytes are padded to 512-byte boundaries
  on disk. De-padding is done by reading `ds` bytes then advancing `ceil(ds/512)*512`.
- The 240-byte SD-1 internal pointer table is never stored on disk; it's zeroed on
  reconstruct. The SD-1 hardware rebuilds it from the sequence headers.
- The original SysEx may have 128 trailing bytes in the event section beyond `seq_data_len`
  (confirmed non-zero in "Ascention Island alternate take.syx"). These are irrelevant —
  the SD-1 uses `seq_data_len` from the global section to know where data ends.

### SysEx payload encoding
- `SysExPacket.payload` is already **de-nybblized** (raw bytes) when parsed.
- `to_bytes()` re-nybblizes on output (2 output bytes per raw byte).
- Original AllSequences SysEx: 76,985 bytes → 38,489 raw bytes de-nybblized.
- Extracted AllSequences SysEx: 76,729 bytes → 38,361 raw bytes. 128-byte difference
  is the trailing data described above, functionally irrelevant.

### VST3 block-1 vs standard directory (cmd_extract)
- Files in the VST3 block-1 directory must be read with contiguous block reads (not FAT
  chain traversal), because the SD-1 overlays its directory data on top of the FAT blocks.
- The `use_contiguous` flag in `cmd_extract` tracks which path a file came from.

## Post-Mortem (Required for Artifact Index)

### What Worked
- **TDD approach**: Writing `disk_to_allsequences_round_trips_via_disk` before implementing
  the function caught the design clearly. The `disk→payload→disk` test is the right shape
  because it avoids the pointer-table zeroing issue.
- **Python diagnostic scripts**: Using inline Python to inspect raw bytes, compare section
  by section (ptr_table, event_lead, event_data, headers, global), and compute size ratios
  was very effective for understanding where the 128-byte discrepancy came from.
- **Comparing against the original SysEx directly**: The "check the original's global section"
  step proved the discrepancy was in the source data (trailing bytes past `seq_data_len`),
  not in our algorithm.

### What Failed
- **Initial size comparison approach**: Comparing total SysEx file sizes (76,985 vs 76,729)
  without first understanding de-nybblization ratios led to confusion about how much data
  should be present. Need to work in de-nybblized bytes, not SysEx bytes.

### Key Decisions
- **Zeroed ptr table on reconstruct**: The 240-byte internal pointer table is zeroed on
  reconstruction, not preserved. The SD-1 always rebuilds it from the sequence headers.
  - Alternatives: store the original ptr table bytes on disk as extra metadata
  - Reason: The SD-1 doesn't use the ptr table from SysEx; storing it would waste space
    and complicate the format.
- **`has_programs` from `type_info & 0x20`**: Rather than heuristically detecting the
  sequence data offset, use the existing `type_info` flag that `cmd_write` already sets.
  - Reason: Single source of truth, no ambiguity.

## Artifacts

- `crates/sd1disk/src/types.rs` — `disk_to_allsequences()` at line 294
- `crates/sd1disk/src/types.rs` — tests at lines 520-574
- `crates/sd1cli/src/main.rs` — `cmd_extract` at lines 396-450

## Action Items & Next Steps

1. **Wait for Sojus fixed `esq16_dsk.cpp`** — once released, test that newly created `.img`
   files from the VST3 plugin are no longer corrupted. No code changes needed on our side.

2. **(Optional) Add a `repair-img` command** — detect and compensate for the Sojus
   off-by-one corruption in existing `.img` files. Would need to shift each track's
   sectors back one position (losing the original sector 0 data, which is unrecoverable).
   Low priority since our write path is unaffected.

3. **(Optional) `cmd_write` FAT cleanup** — skip `set_chain()` for block-1 path writes
   (it's harmless but wasteful). See prior handoff for details.

4. **(Optional) `cmd_delete` review** — `free_chain()` uses FAT traversal for VST3-managed
   files. May need the same contiguous-vs-FAT distinction as `cmd_extract`.

5. **Live hardware validation** — load the extracted `SEQ-ASCESEQ` SysEx back into a
   real SD-1 or the Sojus VST3 plugin (once fixed) to confirm playback is correct.
   Test file: `/Volumes/Aux Brain/Music, canonical/SysEx Librarian/seq-Ascention Island alternate take.syx`

## Other Notes

- **SysEx test file**: `/Volumes/Aux Brain/Music, canonical/SysEx Librarian/seq-Ascention Island alternate take.syx`
  This is a 4-packet file: AllPrograms (63,607 bytes), AllPresets (1,927 bytes), Command (17 bytes),
  AllSequences (76,985 bytes). `cmd_write` writes AllPresets and AllSequences only (skips
  AllPrograms and Command silently — this is expected behavior).

- **Hardware reference**: KOTO-DREAMS = INT bank 5 patch 2 (b10=32). From prior session.

- **All tests passing**: 11 unit tests in sd1disk + 2 new round-trip tests = 13 total,
  all green after commit `4b970e0`.

- **Embedded programs on write**: When a SysEx file contains both AllPrograms and
  AllSequences, `cmd_write` embeds the programs into the SixtySequences file (type_info
  `0x2F`). The Ascention Island test file does this.

- **`.hfe` workflow**: For writing disks to real hardware, use `.hfe` format (bypasses
  the MAME sector extraction bug). Sojus plugin supports both formats.
