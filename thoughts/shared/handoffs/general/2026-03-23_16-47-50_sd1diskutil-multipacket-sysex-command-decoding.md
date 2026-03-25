---
date: 2026-03-23T23:47:50Z
session_name: general
researcher: Claude
git_commit: f9d885b
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — multi-packet SysEx, Command/Error types, inspect-sysex improvements"
tags: [rust, ensoniq, sd-1, disk-image, sysex, multi-packet, command-message, inspect]
status: complete
last_updated: 2026-03-23
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: Multi-packet SysEx support; Command/Error message types; inspect-sysex improvements

## Task(s)

**COMPLETED: AllPrograms write verified working in emulator**
- User confirmed piano_test2.img loaded correctly in SD-1 emulator
- Programs land in odd bank order due to interleaving (patch 0 → bank 0 slot 1, patch 2 → bank 5 slot 1, etc.) — this is correct hardware behavior, not a bug

**COMPLETED: cmd_extract fixed for SixtyPrograms**
- Added SixtyPrograms arm to cmd_extract: calls deinterleave_sixty_programs() then wraps as AllPrograms SysEx
- Committed in f9d885b

**COMPLETED: Multi-packet SysEx support**
- Added SysExPacket::parse_all() to split concatenated SysEx files on F0/F7 boundaries
- "seq-*" files are full dumps: AllPrograms + AllPresets + Command(0x0C) + AllSequences
- Updated inspect-sysex to iterate all packets with per-packet headers

**COMPLETED: Command and Error message types**
- Added Command (0x00) and Error (0x01) variants to MessageType enum
- SD-1 full dumps contain a Command 0x0C packet (AllSequenceMemoryDump) announcing sequence data size
- print_command_payload() decodes all 15 command types; 0x0B/0x0C show sequence data size

**COMPLETED: Model byte relaxed**
- AllSequences packets use model byte 0x01 (SD-1-specific); programs/presets use 0x00 (VFX-compatible)
- Spec explains: VFX Model ID in header is intentionally 0x00 for cross-device compatibility; SD-1 uses 0x01 for non-shared types
- SysExPacket now stores model byte; validation removed; to_bytes() preserves the original model byte

**COMMITTED: all above changes**
- Two commits: 4497e16 (AllPrograms interleave + inspect-sysex wiring) and f9d885b (extract fix + CLI wiring)
- Remaining uncommitted: multi-packet + Command/Error + model byte changes (not yet committed at handoff time — need to commit)

**OPEN: AllPresets write path verification**
- AllPresets: 20 × 48 bytes = 960 bytes payload (confirmed from spec and live data)
- Preset structure per spec: 3 × 11-byte track params + 3-byte track status + 11-byte effect params + 1 spare = 48 bytes
- TwentyPresets write path exists in cmd_write but not yet verified against emulator
- Preset data available in every "seq-*" full dump file

**OPEN: file_number field always 0**
- Transoniq Hacker article clarifies: file_number determines bank number AND position of file on keyboard display
- For multiple SixtyPrograms files: file_number 0 = bank 1, file_number 1 = bank 2, etc.
- Currently always writes 0; a second AllPrograms write would get file_number=0 too, which may conflict

**OPEN: AllSequences format**
- Full dump structure now understood: 240-byte pointer table + sequence data + sequence headers + global params
- SD-1 spec says sequence data format "not currently documented"
- User has Transoniq Hacker archive with a follow-up article on VFX-SD sequence format — not yet retrieved

**OPEN: file type codes 0x14 and 0x15**
- Transoniq Hacker article lists type 20 (0x14) = SD-1/VFX-SD SysEx file, type 21 (0x15) = truncated (probably "Song" or "Sequence")
- Need to compare against FileType enum in directory.rs

**OPEN: Commit current session changes**
- Uncommitted: multi-packet support, Command/Error types, model byte changes, inspect-sysex improvements

## Critical References

- `crates/sd1disk/src/sysex.rs` — SysExPacket, MessageType, parse_all()
- `crates/sd1cli/src/main.rs` — cmd_inspect_sysex, print_command_payload
- Transoniq Hacker archive (local, user has it) — contains VFX-SD sequence format article

## Recent Changes

- `crates/sd1disk/src/sysex.rs:10-18` — Added Command, Error variants to MessageType
- `crates/sd1disk/src/sysex.rs:63-67` — Added `model: u8` field to SysExPacket
- `crates/sd1disk/src/sysex.rs:97-125` — parse_all() implementation
- `crates/sd1disk/src/sysex.rs` — removed VFX_MODEL constant, removed model byte validation
- `crates/sd1cli/src/main.rs:378-457` — cmd_inspect_sysex rewritten to use parse_all(), multi-packet output
- `crates/sd1cli/src/main.rs:459-495` — print_command_payload() helper added
- `crates/sd1disk/src/types.rs:50,95,130,199,208,231,241` — added `model: 0` to all SysExPacket struct literals
- `crates/sd1disk/tests/operations_tests.rs:14` — added `model: 0`

## Learnings

### SD-1 full dump structure (CONFIRMED)
"seq-*" files from SysEx Librarian are concatenated 4-packet dumps:
1. AllPrograms (0x03, model=0x00, 31800 bytes)
2. AllPresets (0x05, model=0x00, 960 bytes = 20 × 48)
3. Command (0x00, model=0x00, 5 bytes) — command type 0x0C with 4-byte sequence data size
4. AllSequences (0x0A, model=0x01, variable)

### Model byte: VFX-compatible vs SD-1-specific
From the spec: "The VFX Model ID Code in this header is different from the SD-1 Family Member (Model ID) code in the Device ID message in order to allow transfer of common messages between other VFX Product Family members and the SD-1."
- Model 0x00 = VFX-compatible (programs, presets, commands)
- Model 0x01 = SD-1-specific (sequences, which the VFX doesn't have in the same format)

### AllPresets format (confirmed from spec)
Each of 20 presets is exactly 48 bytes:
- Bytes 0–10: Track 0 parameters (packed bit fields, each byte inverted)
- Bytes 11–21: Track 1 parameters
- Bytes 22–32: Track 2 parameters
- Bytes 33–35: Track status array (3 bytes)
- Bytes 36–46: Effect parameters (11 bytes)
- Byte 47: spare

### file_number encodes keyboard display position
From Transoniq Hacker article: file_number is not just a counter — it determines where the file appears on the keyboard's bank display. For SixtyPrograms: file_number 0 = bank 1 slot, file_number 1 = bank 2 slot, etc.

### SD-1 disk format (from Transoniq Hacker article)
- Disk: 80 tracks × 2 heads × 10 sectors × 512 bytes
- Block formula: Block = Track × 20 + Head × 10 + Sector
- FAT sentinel bytes: last 2 bytes of empty FAT block = 0x46 0x42 ("FB")
- Directory sentinel: last 2 bytes of empty directory sector = 0x44 0x52 ("DR")
- First 23 blocks reserved (VFX-SD/SD-1)
- 4 sub-directories, 39 entries each = 156 files max
- FAT entries: 0=free, 1=end-of-file, 2=bad block, otherwise=next block number

## Post-Mortem

### What Worked
- Using inspect-sysex to examine actual files before implementing anything — revealed multi-packet structure immediately
- Storing `model` byte in SysExPacket rather than validating it — simple change, handles all edge cases
- Spec text (even OCR'd) provided exact explanation for model byte behavior
- Python one-liners to analyze binary file structure (finding F0 positions, reading raw bytes)

### What Failed
- Initial assumption that all SysEx files are single-packet — "seq-*" files are 4-packet dumps
- Strict model byte validation — rejected valid SD-1 AllSequences packets (model=0x01)

### Key Decisions
- **parse_all() as associated function on SysExPacket**: Consistent with parse(); callers use SysExPacket::parse_all()
- **store model byte, don't validate**: The spec clarifies the intentional difference; validating breaks valid packets
- **print_command_payload as separate fn**: Keeps cmd_inspect_sysex readable; command decoding is complex enough to deserve its own function

## Artifacts

- `crates/sd1disk/src/sysex.rs` — MessageType enum, SysExPacket struct, parse_all()
- `crates/sd1disk/src/types.rs` — to_sysex() methods (all updated with model: 0)
- `crates/sd1cli/src/main.rs` — cmd_inspect_sysex, print_command_payload
- `crates/sd1disk/tests/operations_tests.rs` — test helper updated

## Action Items & Next Steps

1. **Commit current session changes** — multi-packet + Command/Error + model byte + inspect improvements
2. **Find VFX-SD sequence format article in Transoniq Hacker archive** — user has the full archive; look for the follow-up article to the disk format article (author mentioned "in my next article I will cover the format used for the VFX-SD Sequences")
3. **Verify AllPresets write path** — extract preset packet from a "seq-*" file and write to a test image; load in emulator
4. **Implement file_number incrementing** — when writing a second file of the same type, file_number should increment; check reference disk (disk_with_everything.img) for how file_number is assigned
5. **Check file type codes** — compare FileType enum values against 0x14 (SysEx) and 0x15 (unknown) from Transoniq Hacker article; verify our type_info byte is correct

## Other Notes

**SysEx test files:**
- Full dumps (AllPrograms + AllPresets + Command + AllSequences): `$BASE/seq-*.syx`, `$BASE/Ballistic/seq-*.syx`, `$BASE/Ocean Music/seq-*.syx`, `$BASE/Shatterday/seq-*.syx`, `$BASE/Nocturne/seq-*.syx`
  - BASE = `/Volumes/Backblaze_MacEx4TB57422399/! Aux brain backup/Music, canonical/Ableton/Sets/SysEx Librarian`
- AllPrograms banks: `$BASE/01_PRG.SYX` through `46_PRG.SYX`, `banks/bank-101.syx` etc.
- Single patches: `patch-*.syx` files (OneProgram format, verified working)
- Reference disk: `disk_with_everything.img` — 49-file SD-1 disk, authoritative format reference

**Test commands:**
```
cargo test                                     # 50 tests must pass
cargo run -p sd1cli -- inspect-sysex <file>   # examine SysEx (now handles multi-packet)
cargo run -p sd1cli -- create /tmp/test.img
cargo run -p sd1cli -- write /tmp/test.img <file.syx>
cargo run -p sd1cli -- list /tmp/test.img
```

**SD-1 AllSequences data size note:**
The Command 0x0C size field (e.g., 27060 bytes for Ascension Island) is just the raw sequence event data. The full AllSequences payload (38,489 bytes) also includes the 240-byte pointer table, sequence headers, and global parameters. The discrepancy is expected.
