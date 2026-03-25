---
date: 2026-03-25T00:43:18Z
session_name: general
researcher: Claude
git_commit: c2b57be
branch: main
repository: sd1diskutil
topic: "SD-1 Combined SixtySequences+60Programs — programs verified correct, wrong-patches root cause identified"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, allprograms, combined-format, programs, interleave, track-params, rom-programs]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Programs verified correct; "wrong patches" likely ROM program references

## Task(s)

**COMPLETED: Fix error 192 on combined SixtySequences+60Programs file** (prior session)
- Root cause was `type_info=0x0F` instead of `0x2F` in directory entry byte 0
- Fix at `crates/sd1cli/src/main.rs:350-362` (commit `6ecbaf8`)

**COMPLETED: Verify programs section is correct**
- All 60 program slots on disk match AllPrograms SysEx exactly (slot 0 = ARTIC-ELATE … slot 59 = NASTY-ORGAN)
- Interleave algorithm confirmed correct (verified with COUNTRY-* reference disk)
- Track parameter b0 byte confirmed as direct program slot index (0-59 = user bank)

**OPEN: "Wrong patches" report needs retest**
- User earlier reported "loaded successfully but the patches assigned to the tracks are definitely not the right ones"
- But all data is now verified correct — this likely needs a fresh retest

## Critical References

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (authoritative)
- `crates/sd1disk/src/types.rs:144-317` — `interleave_sixty_programs`, `allsequences_to_disk`, `deinterleave_sixty_programs`
- `disk_with_everything.img` — reference disk; COUNTRY-* is working 60-programs SixtySequences

## Recent Changes

- `tools/dump_programs.py` (new) — diagnostic tool: de-interleaves programs from disk image, prints slot names, compares to AllPrograms SysEx, shows track param b0 values

## Learnings

### Programs section is verified correct end-to-end

After running `tools/dump_programs.py /tmp/nc12_fixed.img 'SEQ-NC 1SEQ' '/path/to/sysex.syx'`:
- All 60 disk slots match SysEx AllPrograms exactly ("All 60 slots match!")
- De-interleave algorithm produces correct results (verified against COUNTRY-* reference)

### Track parameter b0 byte = direct user bank slot index (0-59)

From NIGHT INTRO seq1: b0=3 → user slot 3 = MERLIN ✓; b0=47 → user slot 47 = NORM-1-KIT ✓
- Values 0-59: user bank (embedded programs)
- Values 60-127: ROM programs (b0=85, b0=107 seen in NC12 sequences)
- b0=127 (0x7F): appears to mean "no program change / use current" — most tracks in NC12 use this
- b0=255 (0xFF): inactive/undefined track

### NC12 sequences mostly reference ROM programs

Most tracks in NC12 sequences have b0=127 or b0≥60. Only a few tracks per sequence use the embedded user programs:
- NIGHT INTRO: Track 1 (b0=85 ROM), Track 2 (b0=3 user=MERLIN), rest b0=127
- NIGHT ROAD: Track 2 (b0=47 user=NORM-1-KIT), Track 5 (b0=107 ROM), rest b0=127

This means "wrong patches" for b0=127 tracks is EXPECTED BEHAVIOR — those tracks use whatever ROM program the SD-1 currently has loaded, not the embedded user programs.

### SD-1 auto-loads embedded programs (confirmed)

User tested COUNTRY-* from `disk_with_everything.img` in MAME — programs loaded and assigned correctly to tracks. The SD-1 DOES auto-load the embedded programs section when recalling a SixtySequences+60Programs file.

### SysEx packet format (Python parser fix)

Ensoniq SysEx format: `F0 0F 05 <model> <channel> <msg_type> <nybbles> F7`
- `body[4]` = message type (NOT `body[3]` which is MIDI channel)
- AllPrograms = 0x03, AllPresets = 0x05, AllSequences = 0x0A
- The `dump_programs.py` tool had this bug and was fixed

### type_info byte encoding (directory entry byte 0)

- `0x2F` = SixtySequences with 60 embedded programs (REQUIRED for programs to load)
- `0x0F` = SixtySequences without programs
- `0x20` = ThirtySequences with 60 programs
- `0x00` = ThirtySequences without programs

## Post-Mortem

### What Worked
- **dump_programs.py tool**: Single tool that verifies interleave, compares to SysEx, and shows track params — definitively proved programs section is correct
- **COUNTRY-* as positive control**: Testing the reference disk confirmed de-interleave algorithm correctness before investigating NC12
- **Direct b0 mapping**: Cross-referencing b0 values against disk slot names decoded the track param encoding without needing the spec

### What Failed
- **BCD hypothesis for track param encoding**: Initially thought b0=0x35=53 encoding slot 35 was BCD. Disproved by b0=0x0B not encoding slot 15 the same way. Actual encoding is direct index (coincidence for 0x35).
- **Wrong SysEx message type constant**: Initial `dump_programs.py` used 0x09 for AllPrograms (which is SingleSequence). Needed 0x03.
- **Wrong byte offset in SysEx parser**: Used `body[3]` (MIDI channel) instead of `body[4]` (message type). Tool silently found no packets.

### Key Decisions
- **dump_programs.py shows 3 sequences (not all 60)**: Enough to verify b0 encoding; showing all 60 would be noisy
- **Stripped high bit in program name extraction**: Ensoniq sets MSB for mute status flags, so `& 0x7F` needed for readable ASCII

## Artifacts

- `tools/dump_programs.py` — new diagnostic tool for program slot verification
- `tools/hybrid_programs_test.py` — prior session hybrid splice tool (commit `c2b57be`)
- `tools/compare_allseq_sysex_vs_disk.py` — earlier SysEx vs disk analysis tool
- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — authoritative spec
- `crates/sd1cli/src/main.rs:350-362` — type_info=0x2F fix (commit `6ecbaf8`)

## Action Items & Next Steps

1. **Regenerate and retest the disk image** — `/tmp/nc12_fixed.img` may be stale
   ```
   cargo run -- write /path/to/nc12.syx /tmp/nc12_retest.img
   ```
   Then load `SEQ-NC 1SEQ` in MAME and verify:
   - Do tracks that reference b0=3 (user slot 3 = MERLIN) show MERLIN?
   - Do tracks with b0=127 show whatever the current ROM program is? (This is expected)

2. **Clarify what "wrong patches" means** — if the user is hearing wrong sounds specifically on tracks that use user programs (b0=0-59), there's still a bug. If it's only the b0=127/ROM tracks that sound unexpected, that's correct behavior (those tracks use whatever ROM program the SD-1 has).

3. **Write integration test for combined format** — now that we understand the format, add a test verifying that written+de-interleaved programs match the input AllPrograms SysEx

4. **Commit the new tool** — `tools/dump_programs.py` is new and untracked; commit it

## Other Notes

**Test SysEx file (NC12):**
```
/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-NC 12 North final (all).syx
```
4 packets: AllPrograms (31800 bytes, 60 programs, type 0x03) + AllPresets (960 bytes, type 0x05) + Command (type 0x00, skipped) + AllSequences (54927 bytes, type 0x0A)

**NC12 program-using tracks (non-ROM):**
- NIGHT INTRO seq1, Track 2: b0=3 → MERLIN
- NIGHT ROAD seq2, Track 2: b0=47 → NORM-1-KIT
- Most other tracks: b0=127 (ROM/current)

**NC12 disk file name:** `SEQ-NC 1SEQ` (not NC12NORTSEQ — generated from sysex filename)

**Test count:** 54 tests pass (43 unit + 11 integration) — unchanged since type_info fix

**Reference disk constants:**
- FAT: block 5, 3-byte BE entries, EOF=0x000001
- SubDir0: block 15, 26-byte entries
- type_info for SixtySeq+60programs: **must be 0x2F**
- Programs section: bytes 11776–43575 (31800 bytes), seq data at 44032

**Interleave algorithm** (`crates/sd1disk/src/types.rs:149-168`):
- even_data = programs 0, 2, 4, ..., 58 concatenated (30 × 530 = 15900 bytes)
- odd_data = programs 1, 3, 5, ..., 59 concatenated
- result[2i] = even_data[i], result[2i+1] = odd_data[i]
- De-interleave is exact inverse; verified correct with COUNTRY-* reference disk
