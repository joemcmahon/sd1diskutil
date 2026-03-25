---
date: 2026-03-24T00:49:38Z
session_name: general
researcher: Claude
git_commit: 9ca196c
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — full-dump write support; AllSequences on-disk format investigation"
tags: [rust, ensoniq, sd-1, disk-image, sysex, multi-packet, allpresets, allsequences, format]
status: complete
last_updated: 2026-03-23
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Full-dump write working for programs+presets; AllSequences format differs from SysEx payload

## Task(s)

**COMPLETED: OneProgram write verified in emulator**
- User confirmed `/tmp/sabre_test.img` (sabresaw.syx, "SABRE SAW") loaded correctly
- OneProgram write path is correct

**COMPLETED: AllPresets write verified in emulator**
- Presets from full dump (`seq-cometary final.syx`) loaded correctly in emulator
- On-disk format for TwentyPresets IS the raw denybblized SysEx payload (same as AllPrograms approach)

**COMPLETED: Multi-packet full-dump write (AllPrograms + AllPresets)**
- `cmd_write` now uses `parse_all()` instead of `parse()`
- Iterates all packets, skips Command and Error housekeeping packets
- For multi-packet files: auto-generates names with type suffixes (base + "PST" for presets, "SEQ" for sequences)
- Prefix capped at 8 chars so suffix fits within 11-char name limit
- AllSequences → `FileType::SixtySequences` (was incorrectly using `Sequence::file_type()` which always returned OneSequence)
- Committed: 9ca196c

**BLOCKED: AllSequences write**
- Writing raw AllSequences SysEx payload to disk causes SD-1 system error 192
- Investigation confirmed: on-disk SixtySequences format is NOT the denybblized SysEx payload
- The on-disk format is the SD-1's native internal sequence memory layout
- AllSequences write currently skipped with `eprintln!("Warning: AllSequences on-disk format not yet implemented; skipping sequence packet")`

**OPEN: file_number incrementing**
- Currently always writes `file_number: 0`
- file_number controls keyboard bank display position (file_number=0 → bank 1, etc.)
- Not a crash risk — only affects which bank slot is displayed on the SD-1 LCD
- Only matters when writing multiple files of the same type; deferred

**OPEN: AllSequences format reverse engineering**
- Need Transoniq Hacker follow-up article on VFX-SD sequence format
- Author of disk format article said "in my next article I will cover the format used for the VFX-SD Sequences"
- User has the full Transoniq Hacker archive locally

## Critical References

- `crates/sd1cli/src/main.rs:213-340` — cmd_write (full multi-packet loop)
- `disk_with_everything.img` — 49-file reference disk, authoritative format
- `crates/sd1disk/src/sysex.rs` — SysExPacket, parse_all(), MessageType

## Recent Changes

- `crates/sd1cli/src/main.rs:218` — switched from `SysExPacket::parse()` to `parse_all()`
- `crates/sd1cli/src/main.rs:229-234` — writable packet filter (skips Command/Error)
- `crates/sd1cli/src/main.rs:243-255` — per-packet type dispatch (AllSequences now skips with warning)
- `crates/sd1cli/src/main.rs:258-272` — multi-packet name generation with type suffixes
- `crates/sd1cli/src/main.rs:243` — AllSequences → FileType::SixtySequences (fixed from OneSequence)

## Learnings

### AllSequences on-disk format vs SysEx payload (CONFIRMED DIFFERENT)
The SysEx AllSequences payload is NOT written directly to disk. Comparison:
- SysEx payload for seq-countryseq.syx: 35885 bytes, starts with 240-byte pointer table
- COUNTRY-* on disk (disk_with_everything.img): 58983 bytes (ratio ≈ 1.644×)
- On-disk starts with: `00 XX 24` (3-byte header where byte[2] is always 0x24=36) + 11-byte internal sequence name + raw event data
- SysEx starts with: 240-byte pointer table of 4-byte offsets into sequence event data

The on-disk format is the SD-1's native internal memory layout. The SD-1 writes this directly when saving to disk and reads it directly when loading. SysEx AllSequences is a completely separate encoding for MIDI transfer.

### AllSequences SysEx structure (from Command packet + payload analysis)
Full dump structure:
1. AllPrograms (0x03, model=0x00, 31800 bytes)
2. AllPresets (0x05, model=0x00, 960 bytes = 20 × 48)
3. Command (0x00, model=0x00, 5 bytes) — type 0x0C announces sequence data size (bytes 1-4)
4. AllSequences (0x0A, model=0x01, variable)

AllSequences payload structure (from Command 0x0C + payload):
- Bytes 0–239: pointer table (60 × 4-byte offsets, big-endian, many zero = empty slot)
- Bytes 240–(240+seq_data_size): raw sequence event data
- Remaining: per-sequence headers + global parameters

### Directory entry parsing — entries span block boundaries
The 4 subdirectories each span 2 consecutive blocks (SUBDIR_START_BLOCK + i*2 for SubDir i).
Entries are accessed via flat byte offset: `base + slot * 26`.
SUBDIR_CAPACITY = 39 entries per subdirectory (39 × 26 = 1014 bytes > 512, so entries CROSS block boundaries).
**Never parse directory entries by iterating individual blocks separately** — always use the flat offset approach matching `directory.rs:entry_offset()`.

### AllPresets on-disk format (CONFIRMED)
Raw denybblized SysEx payload stored directly. 20 × 48 bytes = 960 bytes. Verified working in emulator.

### type_info field value 0x0F vs 0x2F
Reference disk shows some entries with type_info=0x2F (e.g. SD1-PALETTE, PETROUCHKA, BASSICS).
Our write path always uses 0x0F. Emulator accepts both — not a blocking issue.

## Post-Mortem

### What Worked
- Writing programs+presets from full dump: works first try after switching to `parse_all()`
- On-disk byte comparison using Python: immediately showed AllSequences format mismatch
- Using `flat byte offset = base + slot * 26` for directory parsing (matches Rust library)
- Using seq-countryseq.syx + disk_with_everything.img COUNTRY-* for comparison

### What Failed
- Writing AllSequences SysEx payload to disk → system error 192 on SD-1 hardware
- Initial Python directory parsing (block-by-block) → wrong results because entries span block boundaries

### Key Decisions
- **Skip AllSequences with warning, not error**: allows full dumps to write programs+presets successfully while noting sequences are not yet supported
- **Type suffixes PST/SEQ for multi-packet names**: keeps names within 11-char limit; prefix capped at 8 chars
- **FileType::SixtySequences for AllSequences**: correct mapping (was OneSequence, which is wrong for 60-sequence dumps)

## Artifacts

- `crates/sd1cli/src/main.rs` — cmd_write rewritten for multi-packet support

## Action Items & Next Steps

1. **Find VFX-SD sequence format article** in Transoniq Hacker archive
   - Author said "in my next article I will cover the format used for the VFX-SD Sequences"
   - User has full archive locally
   - This is the key to implementing AllSequences write

2. **Implement AllSequences write** once format is understood
   - On-disk format has: `00 XX 24` + 11-byte internal name + raw event data
   - See `disk_with_everything.img` COUNTRY-* (block 1360, 58983 bytes) for reference
   - See SD1-PALETTE (block 979, 77057 bytes), CLASS-PNO-* (block 1455, 12877 bytes)

3. **Implement file_number incrementing** (low priority, display only)
   - Scan existing files of same type, set file_number = count
   - Reference disk shows file_number increments per type: SixtyPrograms: 0,1,2,...

4. **Check FileType 0x15 name** — our enum calls it `SystemSetup`; Transoniq Hacker says it's "truncated" (possibly Song/Sequence). Cosmetic only.

## Other Notes

**Test commands:**
```
cargo test                                      # 50 tests must pass (all passing)
cargo run -p sd1cli -- inspect-sysex <file>    # multi-packet aware
cargo run -p sd1cli -- create /tmp/test.img
cargo run -p sd1cli -- write /tmp/test.img <file.syx> [--name NAME]
cargo run -p sd1cli -- list /tmp/test.img
```

**Full dump write example (verified working for programs+presets):**
```
cargo run -p sd1cli -- write /tmp/fulltest.img "seq-cometary final.syx" --name COMETARY
# Writes: COMETARY (SixtyPrograms), COMETARYPST (TwentyPresets)
# Skips: COMETARYSEQ (AllSequences — format not implemented)
```

**SysEx test files:**
- Full dumps: `$BASE/seq-*.syx`, `$BASE/Ballistic/seq-*.syx`, etc.
  - BASE = `/Volumes/Backblaze_MacEx4TB57422399/! Aux brain backup/Music, canonical/Ableton/Sets/SysEx Librarian`
- Single patches: `$BASE/patch-*.syx` (OneProgram, verified working)
- AllPrograms banks: `$BASE/01_PRG.SYX` through `46_PRG.SYX`
- Sequence-only dumps: `$BASE/sequences/seq-countryseq.syx`, `seq-rockseq1.syx`, `seq-playseq1.syx`

**On-disk SixtySequences header pattern (from reference disk):**
All SixtySequences files start with: `00 XX 24 [11-byte internal name] 00 00 ...`
- Byte 0: always 0x00
- Byte 1: varies per file (0xe9, 0xf9, 0xfb seen) — unknown meaning
- Byte 2: always 0x24 = 36 — possibly a format version or record type marker
- Bytes 3–13: 11-byte internal sequence title (from SD-1 internal memory)

**Reference disk sequence file locations (disk_with_everything.img):**
- COUNTRY-* (SixtySequences): SubDir0 slot37, first_block=1360, size=58983
- ROCK-BEATS (ThirtySequences): SubDir0 slot38, first_block=810, size=12118
- SD1-PALETTE (SixtySequences): SubDir1 slot4, first_block=979, size=77057
- CLASS-PNO-* (SixtySequences): SubDir1 slot7, first_block=1455, size=12877
- BASSICS-* (SixtySequences): SubDir1 slot8, first_block=1482, size=46951
