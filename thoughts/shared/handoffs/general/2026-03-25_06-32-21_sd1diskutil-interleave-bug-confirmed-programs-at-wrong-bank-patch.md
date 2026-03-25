---
date: 2026-03-25T06:32:21Z
session_name: general
researcher: Claude
git_commit: 31c716f
branch: main
repository: sd1diskutil
topic: "SD-1 interleave bug confirmed: programs land at wrong INT bank/patch positions on disk"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, allprograms, interleave, interleave-bug, b10, vst3]
status: complete
last_updated: 2026-03-25
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: interleave_sixty_programs is wrong — programs land at wrong INT bank/patch on disk

## Task(s)

**COMPLETED: Resumed from prior handoff, tested additional SysEx files**
- Tested `seq-DB final (all).syx` — created disk image, ran dump_programs.py, loaded in MAME
- MAME showed wrong patches (same symptom as NC12)
- Identified specific mismatch: MELODY 2 track 2 b10=0x38=56 → WOODY-PERC (expected KOTO-DREAMS)

**COMPLETED: Ruled out data interpretation bugs**
- Verified nybblize/denybblize is correct
- Verified 60-program count (63607 byte packet = 7 header + 63600 = 60×530×2 nybblized)
- Verified b10 formula with COUNTRY-* positive controls (PIPE-ORGAN1, CLEAR-GUITR)
- Concluded: code is correct, mismatch must be sync issue

**COMPLETED: Discovered via SD-1 VST3 that interleave IS the real bug**
- User loaded DB final SysEx into SD-1 VST3 plugin (receives SysEx directly, no disk)
- Patches loaded at correct INT positions: KOTO-DREAMS at INT 5 slot 2 (b10=32)
- MAME loading our disk image showed KOTO-DREAMS at INT 2 patch 4 (b10=16) — different position!
- Same source data, different INT positions → our disk interleave is wrong

**CONFIRMED: interleave_sixty_programs is placing programs at wrong bank/patch**
- User saved the VST3-loaded patch bank (programs at correct INT positions)
- Loaded sequence+60 patches from our disk image
- Reloaded the saved correct patch bank over the disk's embedded programs
- Result: **tracks had the correct patches** — sequences ARE coherent with AllPrograms SysEx
- There is NO sync mismatch. AllPrograms and AllSequences were captured correctly.
- The interleave is the only bug.

**OPEN: Fix interleave_sixty_programs in Rust**
**OPEN: Fix decode_b10 in dump_programs.py**
**OPEN: Regenerate all disk images with corrected interleave**

## Critical References

- `crates/sd1disk/src/types.rs:149-168` — `interleave_sixty_programs` (WRONG — needs fix)
- `tools/dump_programs.py:263-298` — `decode_b10` (WRONG formula — needs fix)
- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — Giebler spec (authoritative)

## Recent Changes

No code changes this session. All findings are diagnostic.

- `/tmp/db_final.img` — disk image created from DB final SysEx (wrong interleave, discard)

## Learnings

### The interleave bug

`interleave_sixty_programs` shuffles SysEx programs using even/odd byte interleaving. The result is that programs end up at **different INT bank/patch positions** than the SysEx order specifies.

The SD-1 AllPrograms SysEx sends programs in b10 order: SysEx program k = bank k//6, patch k%6 = b10=k. The SD-1 hardware expects that when it reads the disk, program at disk position k lands in INT0 bank k//6, patch k%6. Our current interleave violates this.

**Evidence:**
- VST3 (direct SysEx load): KOTO-DREAMS → INT 5 slot 2 (b10=32) ← correct
- MAME (our disk image): KOTO-DREAMS → INT 2 patch 4 (b10=16) ← wrong

**Proof that sequences are correct:**
- User saved correct patch bank from VST3
- Loaded disk sequences, then reloaded correct patches
- All track assignments were correct
- No sync mismatch exists — this was a false diagnosis

### What the correct interleave should do

The disk interleave must preserve the b10→program mapping. After the SD-1 reads the disk and loads into INT0, program originally at SysEx index k must end up at INT0 bank k//6, patch k%6.

The current even/odd byte interleave splits programs into even-indexed (0,2,4,...,58) and odd-indexed (1,3,5,...,59) and interleaves their bytes. This does NOT preserve b10 mapping.

To understand the correct interleave, compare the byte layout of a known-good disk image (e.g. the user's saved corrected DB disk) against our Rust output. The corrected image will be available after this session.

### The COUNTRY-* verification was misleading

The COUNTRY-* b10=44→PIPE-ORGAN1 and b10=58→CLEAR-GUITR "verifications" appeared to confirm the formula but were actually testing a broken system. COUNTRY-* was verified with the wrong interleave; the formula deint_slot=(b10-30)*2+1 happened to give program names that matched MAME because the COUNTRY-* AllPrograms also has those programs at those (wrong) positions. Both the interleave and the verification used the same wrong mapping, so they agreed.

### decode_b10 formula is wrong

Current formula: `deint_slot = b10 * 2 if b10 < 30 else (b10 - 30) * 2 + 1`

After de-interleaving, `disk_programs[k]` = SysEx program k = the program that should be at b10=k. The correct lookup is simply `disk_programs[b10]` — direct, no conversion. The formula was adding an incorrect indirection layer on top of an already correct de-interleave.

### Correct approach to verify interleave

1. User saves the SD-1 VST3 patch bank as a 60-program set into the DB final disk image
2. Compare that disk image byte-for-byte against our Rust-generated image
3. The programs section (bytes 11776–43576) will differ — the correct layout is whatever the VST3/SD-1 produces
4. Reverse-engineer the correct interleave from the diff

## Post-Mortem

### What Worked
- **VST3 as ground truth**: Loading AllPrograms SysEx into the SD-1 VST3 plugin showed the correct INT bank/patch positions without any disk interleave interference. This was the key to breaking the false sync-mismatch diagnosis.
- **Patch save/reload test**: User's idea to save correct patches, load disk sequences, then reload patches — proved sequences are correct in a single decisive test.
- **Checking multiple SysEx files**: DB final showed same symptom as NC12, ruling out NC12-specific data issues.

### What Failed
- **COUNTRY-* as positive control**: Was believed to confirm the interleave formula, but actually both the formula and the COUNTRY-* data had the same wrong mapping, so they agreed by coincidence.
- **Sync mismatch diagnosis**: Three sessions concluded AllPrograms/AllSequences were captured at different times. This was completely wrong — the interleave was the real bug all along.
- **"b10 requires de-interleave conversion" belief**: The formula `deint_slot = b10*2 if b10<30 else (b10-30)*2+1` was empirically "confirmed" but was actually compounding the error.

### Key Decisions
- Decision: Do not fix interleave before seeing the user's corrected disk image
  - Reason: The correct interleave should be derived from comparing the good image against the bad one, not guessed from first principles (we've already guessed wrong twice)

## Artifacts

- `tools/dump_programs.py` — needs decode_b10 fix (line 285)
- `crates/sd1disk/src/types.rs` — needs interleave_sixty_programs fix (lines 149-168)
- `crates/sd1disk/src/types.rs` — deinterleave_sixty_programs also wrong (lines 300-315)
- `/tmp/db_final.img` — wrong interleave (for comparison only)
- User will produce: corrected DB final disk image (from VST3 save) — use as ground truth for fixing interleave

## Action Items & Next Steps

1. **Get the corrected disk image from user** — saved via SD-1 VST3 with correct patch bank embedded
2. **Diff the programs section** (bytes 11776–43576) between corrected image and `/tmp/db_final.img` — the diff reveals the correct interleave
3. **Fix `interleave_sixty_programs`** in `crates/sd1disk/src/types.rs:149-168` based on the diff
4. **Fix `deinterleave_sixty_programs`** in `crates/sd1disk/src/types.rs:300-315` (must be exact inverse)
5. **Fix `decode_b10`** in `tools/dump_programs.py:285` — change `disk_programs[deint_slot]` to `disk_programs[b10]`
6. **Run all tests** — 54 tests (43 unit + 11 integration); some interleave tests may now fail and need updating
7. **Regenerate all disk images** with corrected interleave and verify in MAME

## Other Notes

**SysEx file locations:**
- All Shatterday SysEx: `/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/`
- DB final: `seq-DB final (all).syx` (175,412 bytes, 2007-03-22)
- All 8 files have same structure: AllPrograms (0x03) + AllPresets (0x05) + Command (0x00) + AllSequences (0x0A)

**Programs section on disk:** bytes 11776–43576 (31800 bytes = 60 × 530)

**b10 encoding (unchanged — this is correct):**
- 0x00–0x3B: RAM program, b10 = bank×6+patch
- 0x80–0xFE: ROM program, enc = b10 & 0x7F, rom_index = enc + 8
- 0x7F: no program change
- 0xFF: track inactive

**Test count:** 54 tests (43 unit + 11 integration)

**Disk write CLI:** `cargo run -- create <image>` then `cargo run -- write <image> <sysex>`
File prefix for DB: `SEQ-DB FSEQ` (not `SEQ-DB F` — that matches presets first)
