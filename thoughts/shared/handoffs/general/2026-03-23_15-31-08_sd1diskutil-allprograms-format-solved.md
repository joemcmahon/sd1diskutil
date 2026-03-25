---
date: 2026-03-23T22:31:08Z
session_name: general
researcher: Claude
git_commit: b1f2c63
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — AllPrograms format solved; inspect-sysex added"
tags: [rust, ensoniq, sd-1, disk-image, sysex, format, allprograms, interleave, inspect]
status: complete
last_updated: 2026-03-23
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: AllPrograms on-disk format reverse-engineered; inspect-sysex added

## Task(s)

**COMPLETED: OneProgram write path verified working**
- sabresaw.syx loaded in emulator, patch sounds correct, name shows "SABRE SAW" on Edit Program screen
- Root cause of blank filename: SD-1 LCD doesn't render lowercase — filenames must be uppercase
- Fix: `cmd_write` now calls `.to_uppercase()` on the resolved name (commit `b1f2c63`)

**COMPLETED: AllPrograms on-disk format reverse-engineered**
- The SD-1 SixtyPrograms on-disk format byte-interleaves even- and odd-indexed programs
- Even file bytes (0,2,4,...) = programs 0,2,4,...,58 concatenated (30 × 530 bytes)
- Odd file bytes (1,3,5,...) = programs 1,3,5,...,59 concatenated (30 × 530 bytes)
- Fix: `interleave_sixty_programs()` added to `sd1disk::types`, called from `cmd_write` AllPrograms arm
- Inverse `deinterleave_sixty_programs()` also added (needed for extract path)

**COMPLETED: `inspect-sysex` subcommand added**
- `sd1cli inspect-sysex <file.syx>` shows message type, channel, payload size
- For AllPrograms: lists all 60 program names with slot numbers and `<empty>` markers
- For OneProgram: shows program name
- This revealed PIANO.SYX has only 30 real programs (slots 0–23 and 30–35); rest are blank

**OPEN: AllPrograms write needs emulator verification**
- `/tmp/piano_test2.img` was created with PIANO.SYX using the new interleaved format
- User needs to load it and compare program names/distribution against `inspect-sysex` output
- Expected: 30 real patches at slots 0–23 and 30–35; 30 blank slots elsewhere

**OPEN: AllPresets (TwentyPresets) format unknown**
- Not yet investigated; may use same interleaving pattern as AllPrograms
- User has no 20-preset SysEx files for testing anyway

**OPEN: Extract path needs update for SixtyPrograms**
- `cmd_extract` currently writes raw on-disk bytes as SysEx; for SixtyPrograms it must
  call `deinterleave_sixty_programs()` first, then re-wrap as AllPrograms SysEx

**OPEN: file_number field always 0**
- Reference disk shows `file_number` incrementing per file type (0,1,2...)
- Currently always writes 0; unknown if this matters to SD-1

## Critical References

- `disk_with_everything.img` — 49-file SD-1 disk, authoritative format reference
- `crates/sd1disk/src/types.rs` — `interleave_sixty_programs`, `deinterleave_sixty_programs`
- `crates/sd1cli/src/main.rs` — `cmd_write`, `cmd_inspect_sysex`

## Recent Changes

- `crates/sd1cli/src/main.rs:235-242` — `.to_uppercase()` on resolved name in `cmd_write`
- `crates/sd1cli/src/main.rs:215-217` — AllPrograms arm now calls `interleave_sixty_programs()`
- `crates/sd1disk/src/types.rs:139-189` — `interleave_sixty_programs()` and `deinterleave_sixty_programs()` added
- `crates/sd1disk/src/lib.rs:17` — both functions re-exported
- `crates/sd1cli/src/main.rs` (Command enum + run() + cmd_inspect_sysex fn) — `inspect-sysex` subcommand added

## Learnings

### SD-1 LCD / filename encoding
The SD-1's LCD character ROM does not render lowercase ASCII. All filenames written to disk must be uppercase. The reference disk (`disk_with_everything.img`) confirms: every directory entry name is uppercase.

### SD-1 SixtyPrograms on-disk format (CONFIRMED)
The 31800-byte on-disk SixtyPrograms file is a byte-level interleave of all 60 programs:
- `file[2*i]   = even_data[i]`  where `even_data` = programs 0,2,4,...,58 concatenated
- `file[2*i+1] = odd_data[i]`   where `odd_data`  = programs 1,3,5,...,59 concatenated
Each program in both halves is in OneProgram format (530 bytes, name at offset 498).
Confirmed by de-interleaving SD1-INT from `disk_with_everything.img` — all 60 SD-1 INT program names appeared correctly.

### OneProgram on-disk format = SysEx payload
Confirmed: the OneProgram SysEx denybblized payload IS the correct on-disk binary format. No transformation needed beyond denybblization and uppercase naming.

### AllPrograms SysEx payload structure
Each of the 60 programs in the AllPrograms SysEx payload is in the same 530-byte format as a standalone OneProgram. Names at offset 498 within each 530-byte slot. Blank programs are all spaces (0x20).

### Emulator creates wrong-format disks
The SD-1 emulator's "format disk" creates FAT at block 4, dirs at block 14 (wrong). When READING existing correct disks, it uses FAT at block 5, dirs at block 15. Never use emulator-created blank disks as references.

### inspect-sysex is essential
Without it, debugging the AllPrograms format involved comparing against wrong reference banks. This tool should be run before any write operation to understand the source data.

## Post-Mortem

### What Worked
- Empirical byte-level comparison of reference disk against SysEx payload
- Scanning for ASCII name strings at all offsets to find consistent patterns
- Noticing that interleaved bytes at offset 466 in odd chunks spell two program names
- De-interleaving the ENTIRE file (not just adjacent chunks) revealed the format immediately
- Writing inspect-sysex to validate source data before chasing format bugs

### What Failed
- Comparing SD1-INT against PIANO.SYX to detect format differences — they're different patches, so all bytes differ regardless of format
- Hypothesis that names are at offset 466 within each 530-byte chunk — they're not; offset 466 in odd chunks is an artifact of the interleaving, not a real name field
- Hypothesis that programs are stored in byte-interleaved pairs within 1060-byte chunks — wrong; the interleaving is across the ENTIRE file
- Assumption that PIANO.SYX has 60 real programs — it only has 30

### Key Decisions
- **Interleave in library, not CLI**: `interleave_sixty_programs()` in `sd1disk::types` so the extract path can use `deinterleave_sixty_programs()` symmetrically
- **inspect-sysex shows raw bytes**: Shows full 11-char name field with leading/trailing spaces preserved, plus `<empty>` tag for all-space names

## Artifacts

- `crates/sd1disk/src/types.rs:6-9` — constants (added SIXTY_PROGRAMS_COUNT)
- `crates/sd1disk/src/types.rs:139-189` — `interleave_sixty_programs` + `deinterleave_sixty_programs`
- `crates/sd1disk/src/lib.rs:17` — re-exports
- `crates/sd1cli/src/main.rs:119-144` — InspectSysex command definition and dispatch
- `crates/sd1cli/src/main.rs:372-435` — `cmd_inspect_sysex` implementation
- `/tmp/piano_test2.img` — test disk with PIANO.SYX in interleaved format (needs emulator verification)

## Action Items & Next Steps

1. **IMMEDIATE: Verify piano_test2.img in emulator**
   - Load `/tmp/piano_test2.img` in SD-1 emulator
   - Run `cargo run -p sd1cli -- inspect-sysex "/Volumes/Aux Brain/Music, canonical/Ensoniq/BANKS/PIANO.SYX"` to see expected names
   - Compare: should see 24 named patches in slots 0–23, then 6 blank, then 6 named (30–35), then all blank
   - If names match, AllPrograms write is complete ✓

2. **Fix cmd_extract for SixtyPrograms**
   - Currently `cmd_extract` reads raw on-disk bytes and wraps as SysEx
   - For SixtyPrograms type: must call `deinterleave_sixty_programs()` on the raw data before wrapping
   - `crates/sd1cli/src/main.rs:310-347` — the extract function to update

3. **Investigate AllPresets format**
   - Likely same byte-interleaving for 20 presets of 48 bytes each
   - Test when a TwentyPresets SysEx file is available

4. **file_number field** (low priority)
   - Reference disk: file_number increments per file type (second SixtyPrograms = 1, etc.)
   - Our code always writes 0; verify whether SD-1 cares

5. **Commit current state**
   - `interleave_sixty_programs` fix + `inspect-sysex` command not yet committed

## Other Notes

**Test commands:**
```
cargo test                                     # 50 tests must pass
cargo run -p sd1cli -- inspect-sysex <file>   # examine SysEx before writing
cargo run -p sd1cli -- create /tmp/test.img
cargo run -p sd1cli -- write /tmp/test.img <file.syx>
cargo run -p sd1cli -- list /tmp/test.img
```

**SysEx test files:**
- `/Volumes/Aux Brain/Music, canonical/Ensoniq/BANKS/PIANO.SYX` — AllPrograms, 30 real patches out of 60
- `/Volumes/Aux Brain/Music, canonical/Ensoniq/Ensoniq SD VFX presets/singles/sabresaw.syx` — OneProgram, "SABRE SAW", confirmed working

**SD-1 program memory layout:**
- 60 programs total in a SixtyPrograms bank
- SD-1 shows them in 10 banks of 6 programs each
- Slots 0–5 = bank 1, slots 6–11 = bank 2, etc.

**Key offsets in OneProgram format (530 bytes):**
- Bytes 0–497: patch parameters
- Bytes 498–508: program name (11 bytes, ASCII, space-padded)
- Bytes 509–529: additional parameters

**Uncommitted changes at handoff:**
- `crates/sd1disk/src/types.rs` — interleave functions
- `crates/sd1disk/src/lib.rs` — re-exports
- `crates/sd1cli/src/main.rs` — uppercase fix (committed b1f2c63) + interleave call + inspect-sysex command
