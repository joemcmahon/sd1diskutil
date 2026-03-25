---
date: 2026-03-25T03:04:08Z
session_name: general
researcher: Claude
git_commit: 31c716f
branch: main
repository: sd1diskutil
topic: "SD-1 track param b10 lookup fixed; wrong-patches root cause identified as AllPrograms/AllSequences sync mismatch"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, allprograms, track-params, rom-programs, interleave, b10]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: b10 lookup corrected; wrong-patches diagnosed as SysEx sync issue

## Task(s)

**COMPLETED: Fix `dump_programs.py` to use b10 (not b0) as program byte**
- `TRACK_PARAM_PROG_BYTE` changed from 0 to 10
- Full INT0 and ROM 0/1 program name lookup added
- Verified against COUNTRY-* reference disk

**COMPLETED: Diagnose "wrong patches" report on NC12**
- Root cause confirmed: AllPrograms and AllSequences SysEx were captured at different points in time with different program arrangements
- The disk format, interleave, and Rust write pipeline are all correct
- The SD-1 shows exactly what the data predicts

**COMPLETED: Fix b10 → de-interleaved slot mapping in dump_programs.py**
- Previous code used `disk_names[b10]` (direct slot lookup) — WRONG
- b10 is bank×6+patch addressing; must convert to de-interleaved slot:
  - banks 0–4: `slot = b10 * 2` (even slots)
  - banks 5–9: `slot = (b10 - 30) * 2 + 1` (odd slots)
- Verified: b10=44→slot 29=PIPE-ORGAN1 ✓, b10=58→slot 57=CLEAR-GUITR ✓

**OPEN: Re-dump NC12 SysEx from real hardware**
- User needs to retrieve diskettes from storage
- Load the 60-sequence file into the real SD-1
- Dump AllPrograms + AllSequences in a single session without changing programs
- This will produce a coherent SysEx where programs and sequences are in sync

**OPEN: b10=0x7E mystery**
- NIGHT ROAD Track 3 has b10=0x7E (126); high bit not set, not 0x7F, not 0xFF, not in 0x00–0x3B user range
- SD-1 MAME showed DIGIPIANO-1 (ROM 0 bank 1 patch 0) for this track
- No consistent decoding theory yet; may be a firmware quirk or alternate ROM addressing

## Critical References

- `tools/dump_programs.py` — primary diagnostic tool (just committed, commit 99abbba)
- `thoughts/shared/handoffs/general/2026-03-24_18-55-09_sd1diskutil-track-param-b10-program-byte-discovery.md` — session that identified b10 as program byte
- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (authoritative file layout)

## Recent Changes

- `tools/dump_programs.py` (new file, commit 99abbba) — complete rewrite of track param section:
  - `tools/dump_programs.py:56` — `TRACK_PARAM_PROG_BYTE = 10`
  - `tools/dump_programs.py:59-70` — `INT0_PROGRAMS` table (60 stock RAM programs)
  - `tools/dump_programs.py:72-100` — `ROM_ALL_PROGRAMS` table (120 programs, ROM 0 + ROM 1)
  - `tools/dump_programs.py:234-264` — `decode_b10()` with full RAM/ROM lookup and correct slot mapping
  - `tools/dump_programs.py:267-298` — `dump_track_params()` accepts `disk_programs` for embedded program lookup

## Learnings

### b10 is bank×6+patch addressing, not a direct de-interleaved slot index

The SD-1 loads 60 programs from the disk into INT0 RAM in bank order (bank 0 patch 0 through bank 9 patch 5). The de-interleave reorders them by storage geometry (even slots = banks 0–4, odd slots = banks 5–9), but the SD-1 reads each program's internal metadata and places it in the correct INT0 bank slot. Sequence track params then reference programs by their bank×6+patch sequential index (b10), not by their de-interleaved slot position.

Conversion: for b10 in 0x00–0x3B:
- If b10 < 30: `deint_slot = b10 * 2`
- If b10 >= 30: `deint_slot = (b10 - 30) * 2 + 1`

### ROM program encoding (verified for ROM 0 and derived for ROM 1)

- b10 high bit set (0x80–0xFE) = ROM program
- `enc = b10 & 0x7F`; `rom_index = enc + 8`; lookup in `ROM_ALL_PROGRAMS`
- ROM 0 occupies rom_index 0–59; ROM 1 occupies 60–119
- enc 0 = ROM 0 bank 1 patch 2 (ROM 0 bank 0 and bank 1 patches 0–1 unreachable; enc would be negative)
- Formula derived from 2 confirmed data points + MAME verification (BALLAD-KIT b10=0xB1 ✓)

### "Wrong patches" = AllPrograms/AllSequences captured at different times

The NC12 SysEx contains programs from a *different SD-1 state* than when the sequences were authored. The sequences reference bank×6+patch addresses that were correct when written, but AllPrograms was captured after the user reorganized their programs. The disk is faithful to the SysEx; the SysEx has the mismatch. Fix: re-dump from real hardware with programs in the original arrangement.

### b10=0x7E is unknown

0x7E (126) is in the range 0x3C–0x7E which our current model has no explanation for. The SD-1 (MAME) showed DIGIPIANO-1 (ROM 0 bank 1 patch 0, rom_index=6) for this value. This does NOT fit the formula `rom_index = enc + 8` (which would give rom_index=134, out of range). Possible explanations: alternate ROM bank encoding, firmware special value, or MAME artifact.

### INT0 = RAM bank, not ROM

The term "INT0" refers to the user RAM bank that is initialized to a fixed default set on power-up but can be overwritten by loading a disk file. The default init programs are what `INT0_PROGRAMS` in the tool reflects; actual loaded content may differ.

### Disk write workflow

The CLI requires an existing disk image: `cargo run -- create <image>` then `cargo run -- write <image> <sysex>`.
Not `cargo run -- write <sysex> <image>` (argument order matters; image is first).

## Post-Mortem

### What Worked
- **Empirical MAME verification**: Loading the generated image in MAME and reading the SD-1 display directly confirmed or denied each hypothesis about b10 encoding — faster than pure analysis
- **Cross-referencing two data points**: b10=44→PIPE-ORGAN1 and b10=58→CLEAR-GUITR together uniquely determined the bank×6+patch→de-interleaved slot conversion formula
- **ROM_ALL_PROGRAMS flat table**: Combining ROM 0 and ROM 1 into a single 120-entry list with `rom_index = enc + 8` offset is clean and matches observed data
- **dump_programs.py as end-to-end diagnostic**: Single tool verifies programs section, track assignments, and SysEx comparison in one run

### What Failed
- **Direct de-interleaved slot lookup for b10**: `disk_names[b10]` was wrong; b10 is bank-addressed, not slot-indexed. Took MAME verification to reveal
- **Multiple prior sessions believed b10=b0**: Three sessions confirmed "b0=3→MERLIN" etc. as coincidental matches for small slot numbers where bank address = slot index
- **ROM formula with negative encoded values**: ROM 0 bank 0 gives enc < 0 with formula `bank*6+patch-8`; those programs (8 total) are unreachable via sequence program changes

### Key Decisions
- **INT0_PROGRAMS kept as fallback**: When no disk programs available (e.g. standalone sequences), the tool falls back to stock init programs — useful for COUNTRY-*-style files where the programs ARE the defaults
- **`dump_track_params` shows 3 sequences by default**: Sufficient to see program usage patterns without noisy output for all 60 slots
- **Committed `dump_programs.py` as tool, not test**: It's a diagnostic aid, lives in `tools/`, separate from the Rust codebase

## Artifacts

- `tools/dump_programs.py` — diagnostic tool for program slot and track param verification (commit 99abbba)
- `tools/compare_allseq_sysex_vs_disk.py` — earlier SysEx vs disk analysis tool (untracked, scratch)
- `disk_with_everything.img` — reference disk; COUNTRY-* is the authoritative positive control
- `/tmp/nc12_retest.img` — freshly generated NC12 disk image (ephemeral, regenerate with create+write)
- `/tmp/nc12.syx` — local copy of NC12 SysEx (ephemeral copy of Volumes/Aux Brain path)

## Action Items & Next Steps

1. **Retrieve diskettes from storage** — load the NC12 60-sequence file into the real SD-1 (not MAME)
2. **Re-dump SysEx in a single session**: AllPrograms + AllSequences without changing any programs between dumps
   - SysEx Librarian path: `/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/`
   - File to replace: `seq-NC 12 North final (all).syx`
3. **Regenerate disk and retest**: `cargo run -- create /tmp/nc12_v2.img && cargo run -- write /tmp/nc12_v2.img <new_syx>`
4. **Run dump_programs.py** to verify track assignments before loading in MAME
5. **Investigate b10=0x7E** — once fresh SysEx is available, check if that track still has 0x7E or if it was a corrupted/stale value in the original dump

## Other Notes

**NC12 SysEx location:**
```
/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-NC 12 North final (all).syx
```
4 packets: AllPrograms (0x03, 31800 bytes) + AllPresets (0x05, 960 bytes) + Command (0x00, skip) + AllSequences (0x0A)

**NC12 disk file name on image:** `NC12SEQ` (generated from SysEx filename stem)
**NC12 presets file name on image:** `NC12PST`
When running `dump_programs.py`, use prefix `NC12SEQ` not `NC12` (otherwise it matches NC12PST first).

**COUNTRY-* prefix has a leading space:** use `' COUNTRY'` not `'COUNTRY'` when running the tool.

**Test count:** 54 tests (43 unit + 11 integration) — unchanged throughout this session.

**ROM 0 bank layout** (user-provided, verified against MAME):
- Banks 0–9, 6 patches each; see `ROM_ALL_PROGRAMS` in `tools/dump_programs.py:72-100`
- ROM 0 bank 0 and bank 1 patches 0–1 are unreachable via sequence program changes (enc would be negative)

**b10=0x7E showed DIGIPIANO-1 in MAME** — may be worth checking the SD-1 firmware source or Giebler spec addenda for values in the 0x3C–0x7E range. Could be a second user bank, transposition, or MIDI channel encoding.
