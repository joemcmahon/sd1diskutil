---
date: 2026-03-25T01:55:09Z
session_name: general
researcher: Claude
git_commit: c2b57be
branch: main
repository: sd1diskutil
topic: "SD-1 Track Parameter Encoding — b10 is Program Byte, not b0"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, allprograms, track-params, programs, int0, rom-programs]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Track param b10 = program byte (b0 was wrong); dump_programs.py needs fix

## Task(s)

**COMPLETED: Cross-verify COUNTRY-* track-to-program assignments**
- Loaded COUNTRY-* from `disk_with_everything.img` and decoded per-sequence track program assignments
- Cross-checked against SD-1 live readout from user; found complete mismatch initially

**COMPLETED: Root cause analysis of wrong decode**
- Previous sessions assumed `b0` (first byte of 11-byte track param block) = program slot index
- This was WRONG. `b10` (last byte, index 10) is the program index
- All 8 sequences ($ COUNTRY, END, COUNTRY, COUNTRY 2, INTRO, COUNTRYSOLO, COUNTRYSOL2, TAG) now match SD-1 exactly

**OPEN: Fix `dump_programs.py`**
- `TRACK_PARAM_PROG_BYTE = 0` constant is wrong; should be `10`
- The decode logic and INT0 lookup table need updating

## Critical References

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (authoritative file layout)
- `crates/sd1disk/src/types.rs:144-317` — interleave/deinterleave, allsequences_to_disk
- `tools/dump_programs.py` — diagnostic tool with wrong b0 assumption, needs fixing

## Recent Changes

No code changes were made this session. Analysis only.

## Learnings

### `b10` (byte index 10, last byte of 11-byte track param block) is the program index

The Giebler spec (bytes 28–159 of sequence header = 12 × 11-byte track param blocks) does NOT document the internal layout of each 11-byte block. We determined empirically:

- **`b10` (byte 10 of block) = program assignment for that track**
  - `0x00–0x3B` (0–59): INT0 user bank, sequential index = `bank * 6 + patch` (0-indexed)
  - `0x80–0xFE`: ROM program, `b10 & 0x7F` = encoded ROM prog number = `bank * 6 + patch - 8` for ROM patches 0
  - `0xFF`: undefined/no program assigned

- **`b0` (byte 0 of block) encoding (partial understanding):**
  - `0xFF`: track completely inactive
  - `0x7F`: track active, no program change sent on sequence recall
  - Other values: sends program change on recall (exact meaning still unclear — possibly MIDI channel or something else entirely)

- **Previous sessions were wrong** about `b0 = direct program slot index`. The "confirmations" (b0=3→MERLIN, b0=47→NORM-1-KIT) in the NC12 investigation were coincidences or mistakes.

### INT0 bank layout (SD-1 user bank, 60 programs)

Sequential numbering: `prog_num = bank * 6 + patch` (both 0-indexed):
```
bank 0: ARTIC-ELATE  OLYMPIANO    ALTO-SAX     MERLIN       WAY-FAT      GROOVE-KIT
bank 1: ALLS-FAIR    IN-CONCERT   SOLOTRUMPET  INSPIRED     AMEN-CHOIR   PASSION
bank 2: SYMPHONY     MY-DESIRE    MUTED-HORNS  STACK-BASS   DRAWBARS-1   SONOTAR
bank 3: STRINGS      BRASS-STAB   MANDOLIN     CROWN-CHOIR  TUBULAR HIT  JAZZ-KIT
bank 4: STRUM-ME     LUNAR        BLUES-HARP   WIDEPUNCH    BRIGHT-PNO   PIPE-ORGAN1
bank 5: MALLETS      SWEEPER      KOTO-DREAMS  SWELL-SAW    WILBUR       MEATY-KIT
bank 6: FIDDLE       PEDAL-STEEL  BANJO-BANJO  CLOCK-BELLS  THE-QUEEN    ROCK-KIT-2
bank 7: SMOOTH-STRG  DARK-HALL    GUITAR-PADS  FANFARE      MINI-LEAD    NORM-1-KIT
bank 8: STRATOS-VOX  FUNKY-CLAV2  COOL-FLUTES  OH-BE-EX     DANCEBASS-2  WOODY-PERC
bank 9: ANNABELL     FUNK-GUITAR  ELEC-BASS2   CLEAR-GUITAR STUDIO-CITY  MEAN-KIT-1
```

### ROM patches 0 encoding

Two confirmed data points:
- `b10 & 0x7F = 40` → ROM patches 0, bank 8, patch 0 = REEL-STEEL
- `b10 & 0x7F = 24` → ROM patches 0, bank 5, patch 2 = " ELEC-BASS "

Formula (empirically derived from 2 data points): `encoded = bank * 6 + patch - 8`
- Verify: `8*6 + 0 - 8 = 40` ✓; `5*6 + 2 - 8 = 24` ✓
- Note: only 2 data points; formula needs more verification from other ROM program references

### COUNTRY-* embedded programs = stock SD-1 INT0 programs (interleaved)

De-interleave produces: even slots (0,2,4,...,58) = INT0 banks 0–4; odd slots (1,3,5,...,59) = INT0 banks 5–9. This is just an artifact of the interleave algorithm — the programs ARE INT0 stock programs, the interleave reorders them but doesn't change content.

### Track parameter b0/b10 don't directly encode "has sequence data"

Whether a track has sequence data must be determined from the track offset table in the sequence data section (non-zero offset = has data). Using `b0 == 0x7F` as a proxy for "no seq data" is unreliable — some tracks without seq data have non-0x7F b0 values.

### COUNTRY track 6 BANJO-BANJO in SD-1 is state carryover

File has `b10 = 0xFF` (undefined) for COUNTRY track 6. SD-1 shows BANJO-BANJO there because END sequence previously loaded BANJO-BANJO on that MIDI channel. Not encoded in the file.

### Sequence header track numbering

- Spec "Track 1 Parameters" (bytes 28–38) = SD-1 display "track 0"
- Spec "Track 12 Parameters" (bytes 149–159) = SD-1 display "track 11"
- Track 0 in the sequence data = conductor track (clock events only), hidden from SD-1 display
- SD-1 displays 12 tracks (0–11) = spec Tracks 1–12

## Post-Mortem

### What Worked
- **Empirical approach**: printing ALL 11 bytes of each track param block, then cross-referencing against known program names (PEDAL STEEL = INT0 bk6p1, NORM-1-KIT = INT0 bk7p5, BANJO-BANJO = INT0 bk6p2) immediately revealed b10 as the program byte
- **User providing INT0 bank layout**: the bank/patch → sequential mapping let us convert from display address to b10 value and verify every track
- **COUNTRY-* as reference**: SD-1 reference file with known programs (all INT0) made it easy to verify because program names are well-defined

### What Failed
- **b0 hypothesis**: Three prior sessions believed b0=direct program slot. Turned out to be coincidental matches for specific slot numbers
- **BCD encoding hypothesis** (from prior session): disproved again
- **De-interleave diagnosis loop**: Spent time suspecting the de-interleave was wrong; it's actually correct — the issue was reading the wrong byte, not wrong program data

### Key Decisions
- **Kept de-interleave as-is**: verified correct by matching all 60 COUNTRY-* programs to INT0 bank contents
- **Using b10 not b0**: empirically confirmed from 6+ program instances across 2 sequences
- **ROM prog formula `bank*6+patch-8`**: derived from exactly 2 data points (REEL-STEEL, ELEC-BASS); marked as needing more verification

## Artifacts

- `tools/dump_programs.py` — needs fix: `TRACK_PARAM_PROG_BYTE = 0` → `10`, decode logic update, INT0 lookup table
- `disk_with_everything.img` — reference disk; COUNTRY-* is the authoritative test file
- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (does NOT document 11-byte track param internals)

## Action Items & Next Steps

1. **Fix `dump_programs.py`** — change `TRACK_PARAM_PROG_BYTE = 0` to `10`, update `decode_b0()` function to `decode_b10()`, add INT0 lookup table (60 entries above), update ROM decode formula
2. **Re-run dump on NC12** with corrected tool — check if NC12 track program assignments now make sense given b10 = program byte
3. **Clarify b0 encoding** — we know `0xFF` = inactive, `0x7F` = no program change on recall; the meaning of other values (27, 89, 53, etc.) is still unknown. Could be MIDI channel, transposition, or something else. Low priority unless we need it.
4. **Verify ROM formula with more data points** — if any sequence uses other ROM patches 0 programs, check that `b10 & 0x7F = bank*6+patch-8` holds
5. **Commit `tools/dump_programs.py`** (with fix applied) — still untracked
6. **Retest NC12 "wrong patches" report** — the original complaint; now that we understand track param encoding correctly, regenerate NC12 disk and verify in MAME

## Other Notes

**Test SysEx file (NC12):**
```
/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-NC 12 North final (all).syx
```
4 packets: AllPrograms (0x03) + AllPresets (0x05) + Command (0x00, skip) + AllSequences (0x0A)

**COUNTRY-* directory entry:** subdir=0, slot=37, name=`' COUNTRY-*'` (leading space), type_info=0x2F, file_type=0x13

**SD-1 test setup:** `disk_with_everything.img` loaded in MAME; recall ` COUNTRY-*` file named `SEQ-NC 1SEQ` for NC12 tests

**Test count:** 54 tests pass (43 unit + 11 integration), unchanged

**File layout constants (verified):**
- Sequence headers: file bytes 0–11279 (60 × 188)
- Global section: 11280–11300
- Programs section: 11776–43575 (60 × 530, interleaved)
- Sequence data: 44032+

**Interleave algorithm:** even bytes of interleaved block → programs 0,2,4,...,58; odd bytes → programs 1,3,5,...,59. De-interleave is exact inverse. Verified correct.
