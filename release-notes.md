## v1.19 — Lossless sequence round trips (ThirtySequences type, embedded programs); recovery extractor rework

### sd1cli: lossless sequence round trips

- **Bug:** `sd1cli extract` followed by `sd1cli write` turned a ThirtySequences file into SixtySequences (e.g. ROCK-BEATS: 35 → 46 blocks). AllSequences SysEx doesn't record which file type it came from, and `extract` emits the hardware-compatible 60-slot format for both, so v1.18's size check (short payload ⇒ ThirtySequences) never fired. The type can't be inferred from content either: all five SixtySequences files on the factory Sequencer OS disk use only slots 0–29.
- **New `sd1cli write --as auto|thirty|sixty`.** `--as thirty` writes the SysEx as a ThirtySequences file (refused if any of slots 30–59 is defined); `--as sixty` forces SixtySequences; `auto` (the default) behaves as before.
- **`sd1cli extract`** now prints a hint when the source is a ThirtySequences file, showing the `--as thirty` command to write it back.
- **Bug: embedded programs were dropped.** `extract` emitted only the AllSequences packet, so the 60 programs embedded in a sequence file were lost on extract → write. On the factory Sequencer OS disk that hit five files: COUNTRY-\*, SD1-PALETTE, PETROUCHKA\*, BASSICS-\* (SixtySequences, programs before the sequence data) and 30-GROOVES (ThirtySequences, programs after it). `extract` now emits **AllPrograms + AllSequences** for these, and `write` (which already embeds programs when both packets are present) puts them back byte-for-byte. **`extract --no-programs`** keeps the old AllSequences-only output (verified byte-identical to v1.18 for every sequence file on the factory disk); `sd1cli-ffi` has the same flag.
- **Programs flag on ThirtySequences:** `file_type_info` now sets bit `0x20` for ThirtySequences with embedded programs as well as SixtySequences, matching real hardware (the factory 30-GROOVES entry is `0x20`).
- **New library/FFI function `embedded_programs` / `sd1_embedded_programs`** returns a sequence file's embedded programs as an AllPrograms payload; `sd1cli` and `sd1cli-ffi` both use it, so their extract output stays identical.
- Known limit: the 456 bytes after a ThirtySequences file's programs hold SD-1 RAM residue on hardware-written disks; SysEx can't carry it and `write` zeros it (like the last 2 bytes of each 188-byte header).

### Recovery extractor features

- **`tools/extract_intact_sequences.py`** now extracts intact sequences from **ThirtySequences** files as well as SixtySequences.

### Recovery extractor bug fixes

- **Embedded programs:** SixtySequences files with programs embedded (directory `type_info` bit `0x20`) keep their event data at byte 44032, not 11776. The extractor always read from 11776, so for these files it wrapped program data as sequences, and `sd1cli write` accepted them without complaint. **Any `.syx` previously extracted from such a file with this tool is wrong and should be re-extracted.**
- **Fragmented files:** the extractor read files as one contiguous run from `first_block`. It now takes the first `contiguous_blocks` from the directory entry and follows the FAT chain for the rest. Blocks the chain can't account for (unrecovered FAT block, corrupt entry) are treated as unrecoverable rather than guessed.
- **Directory lookup:** only subdirectory 0 was searched, and entries from slot 19 on were misread. All four subdirectories (39 entries each) and the VST3 block-1 directory are now searched, with only valid file types accepted, matching `sd1cli list`.

### Testing

- Verified byte-for-byte against `sd1cli extract` on all 11 sequence files of the factory Sequencer OS v4.10 disk: 164/164 sequences (previously 49 wrong, 14 missing).
- `ascension.img` SEQ-ASCESEQ (embedded programs): 20/20; MUZAK3 `ALEX 96` output unchanged.
- Damage tests: unrecovered FAT block under a fragmented file yields no output (never wrong output); a zeroed data block skips only the affected slot.
- 175 Rust tests, all passing (sd1disk: 124 + 11 integration, sd1ffi: 32, sd1cli: 8). New: `--as thirty` round trip, default unchanged, slots 30–59 refusal, extract hint; embedded programs survive extract → write for COUNTRY-\* (SixtySequences) and 30-GROOVES (ThirtySequences); `embedded_programs` / `sd1_embedded_programs` unit tests.
- All five embedded-programs files on the factory disk round-trip with programs byte-identical, flag and block count preserved, and identical SysEx on re-extract.
- `scripts/test-equivalence.sh` (sd1cli vs sd1cli-ffi) and `scripts/verify-roundtrip.sh`: 36 passed, 0 failed on `disk_with_everything.img`.

## v1.18 — ThirtySequences: zeroed global section and CLI write dispatch

_Reconstructed 2026-11-03 from the commit history; the GitHub release had no notes._

### Bug fixes

- **`thirty_sequences_to_disk`** no longer writes `252 + Σ ds` into the 21-byte global section at offset 5640. Every genuine ThirtySequences file written by SD-1 hardware has zeros there; the non-zero value (introduced in v1.17) caused **System Error 29** on load. The global section is now left zeroed.
- **`sd1cli write`** always routed AllSequences SysEx to `allsequences_to_disk` and tagged the result SixtySequences. Payloads shorter than 11429 bytes (the 30-slot header region) are now written as ThirtySequences.

### Testing

- Three real-hardware reference files added as fixtures (`ROCK-BEATS-real-disk.bin`, `SWING-SHUFL-real-disk.bin`, `30-GROOVES-real-disk.bin`), with integration tests that round-trip them through `disk_to_thirty_sequences` → `thirty_sequences_to_disk` and check that the global is zero, header bytes `[0..186]` match, and event data is identical.

## v1.17 — ThirtySequences declared-size field

_Reconstructed 2026-11-03 from the commit history; the GitHub release had no notes._

### Bug fixes

- **`thirty_sequences_to_disk`** copied the SysEx global verbatim without updating the declared event-area size (`global[2..6]`), so files written with a zeroed or incorrect global could cause SD-1 system error :29/:129 on load. It now always writes `252 + Σ ds`. **Superseded in v1.18:** real ThirtySequences files keep this section zeroed, and v1.18 does so.
- **`disk_to_thirty_sequences`** synthesized the wrong declared size (`Σ ds + 12` instead of `252 + Σ ds`) when patching a zeroed on-disk global.

## v1.16 — Sequence data length from slot headers; hardware SysEx refactor

_Reconstructed 2026-11-03 from the commit history; the GitHub release had no notes._

### Bug fixes

- **`allsequences_to_disk`** computed the sequence data length from the declared size in the SysEx global section. When importing from a disk image with a zeroed global (a known hardware quirk) that value is 0, so no sequence data was written. The length now comes from the per-slot `ds` values in the headers, which are always authoritative, and the correct declared size (`252 + Σ ds`) is written to the on-disk global on every conversion.
- **Hardware SysEx round-trip stability:** bytes 186–187 of each 188-byte header are now normalized to zero when converting hardware SysEx to disk; the on-disk format requires zeros there, but real hardware may not write them.
- **Slots 58 and 59** are both recognized as lacking pointer-table entries in hardware SysEx (previously only slot 59 was handled).

### Internal

- Hardware SysEx functions (`disk_to_allsequences_hw_sysex`, `disk_to_thirty_sequences_hw_sysex`, `allsequences_hardware_sysex_to_disk`) refactored onto shared `HW_*` constants and a `build_hw_allsequences_sysex` helper, replacing the earlier pool-based approach.

## v1.15 — Fix hardware SysEx global declared-size field

### Bug fixes

- **`disk_to_allsequences_hw_sysex`** / **`disk_to_thirty_sequences_hw_sysex`**: `sysex_global[10..14]` was written as `EVENT_LEAD_ZEROS + sum_ds` instead of the actual pool size. The correct value is `pool.len()` — verified against the MAME `4.syx` hardware dump where `global[10:14] == pool.len()`. The previous formula underreported the pool size when stale pool bytes were present, causing the firmware to miscalculate the event data boundary.

### Testing

- 117 unit/integration tests, all passing (sd1disk: 117, sd1cli: 11, sd1ffi: 30)

## v1.14 — Fix hardware SysEx header format and stale ds overflow

### Bug fixes

- **`disk_to_allsequences_hw_sysex`** / **`disk_to_thirty_sequences_hw_sysex`**: Two bugs caused the hardware SysEx output to be rejected by real SD-1 firmware (sequences loaded as empty) and, on native Ensoniq disks, to produce ~1.5 GB output files.

  **Wrong header format (headers loaded as empty):** The 186-byte per-slot sequence headers in the SysEx payload were taken as independent slices (first 186 bytes of each 188-byte disk header). The hardware firmware expects them as a sliding window over the concatenated disk header region starting at byte 112: `hardware_header[slot] = disk_headers_flat[112 + slot*186 : 112 + (slot+1)*186]`. Verified byte-for-byte against a MAME hardware dump of SD1-PALETTE (20/20 defined slots match).

  **Stale ds overflow (~1.5 GB output):** Native Ensoniq disks fill undefined sequence headers entirely with `0xFF`, including the 3-byte `ds` (event data size) field. Reading these as `0xFFFFFF` (16,777,215 bytes per undefined slot) caused the pool section to reference hundreds of megabytes of non-existent data, producing multi-gigabyte nibble-encoded SysEx. Fixed by clamping `ds = 0` when the raw value is `0xFFFFFF`.

  **ThirtySequences bounds:** The sliding window for ThirtySequences reads from `disk` (raw on-disk bytes, ≥6,144 bytes) rather than `disk_headers` (5,640 bytes) because the window end for the last slot (5,692 bytes) exceeds `disk_headers` but fits within `disk`.

- Integration test `hw_sysex_headers_match_real_hardware_dump` added: verifies the fix against `disk_with_everything.img` + `4.syx` (real SD-1 hardware dump); skips gracefully if reference files are absent.

### Testing

- 117 unit/integration tests, all passing (sd1disk: 117, sd1cli: 11, sd1ffi: 30)
- `hw_sysex_headers_match_real_hardware_dump` passes against real reference files

## v1.13 — Explicit hardware SysEx export; lossless default restored

### Breaking change (reverts v1.12 behavior)

- **`disk_to_allsequences`** / **`sd1_disk_to_allsequences`**: Reverted to lossless library-format payload (all 60 sequence slots, including slot 59). Output feeds back into `allsequences_to_disk` for VST-to-VST and image-to-image transfers without data loss. This is the correct default for VST use. Not hardware-compatible — do not send directly to a real SD-1.

- **`disk_to_thirty_sequences`** / **`sd1_disk_to_thirty_sequences`**: Same revert. Lossless, all 60 slots (30 defined + 30 undefined), not hardware-compatible.

### New features

- **`disk_to_allsequences_hw_sysex(disk, has_programs, allow_slot59_loss)`** / **`sd1_disk_to_allsequences_hw_sysex`**: Explicit hardware-compatible AllSequences SysEx export (`F0 0F 05 00 00 0A … F7`, nibble-encoded). Slot 59 is always undefined in hardware SysEx format (hardware ptr-table limitation — the SD-1 itself cannot capture slot 59 in an AllSequences dump). If slot 59 has sequence data and `allow_slot59_loss` is `false`, returns `SD1_ERR_SLOT59_HAS_DATA (-17)` so the caller can warn the user. Pass `allow_slot59_loss = true` to proceed after warning.

- **`disk_to_thirty_sequences_hw_sysex(disk, allow_slot59_loss)`** / **`sd1_disk_to_thirty_sequences_hw_sysex`**: Same for ThirtySequences on-disk format.

- **`SD1_ERR_SLOT59_HAS_DATA` (-17)**: New error code returned when a hardware SysEx export is attempted with slot 59 populated and `allow_slot59_loss = false`.

### Background

v1.12 made `disk_to_allsequences` emit hardware SysEx by default. This was lossy: the SD-1 hardware SysEx format has no ptr-table entry for slot 59, so any sequence data there is silently dropped. For VST users moving data between images, silent data loss is unacceptable. v1.13 restores the lossless default and makes hardware export an explicit, opt-in call with a clear error when lossy conversion would occur.

Empirical check against all Shatterday and Ocean Music hardware dumps confirmed slot 59 is always 0xFF in genuine SD-1 hardware dumps — the hardware itself never populates it in AllSequences SysEx.

### Testing

- 157 unit/integration tests, all passing (116 sd1disk, 11 sd1cli, 30 sd1ffi)
- 6 new tests: slot 59 empty/blocked/allowed cases for both new hw_sysex functions

## v1.12 — Hardware-compatible SysEx export

### Breaking change (SysEx output format)

- **`disk_to_allsequences`** / **`sd1_disk_to_allsequences`**: Previously returned a raw library-internal AllSequences payload. Now returns a complete hardware-compatible SD-1 AllSequences SysEx (`F0 0F 05 00 00 0A … F7`, nibble-encoded), ready to send directly to a real SD-1 in load mode via any SysEx librarian. Callers that previously fed the return value into `allsequences_to_disk` must now feed it into `allsequences_sysex_to_disk` instead.

- **`disk_to_thirty_sequences`** / **`sd1_disk_to_thirty_sequences`**: Same change. Returns full hardware-compatible SysEx.

- **Hardware ptr-table layout**: `ptr_table[0]` is set to `0x00049000` (SD-1 68000 RAM base). Entries 1–59 are pool offsets (preamble-relative) for sequences 0–58. Sequence slot 59 is always undefined (`0xFF` header) — hardware limitation; the SD-1 has no ptr-table entry for slot 59.

- **Pool layout**: output pool includes the 21-byte hardware preamble (12 lead zeros + 9 zero pool-manager state bytes) and 12-byte trailing sentinel, matching what the hardware generates on dump.

- **SysEx files exported before v1.12** used a library-internal format that real SD-1 hardware cannot load. They can still be imported (the auto-detect path handles them) and will be re-exported in the correct format automatically.

### Testing

- 151 unit/integration tests, all passing (round-trip tests updated to use `allsequences_sysex_to_disk` for the reverse leg)
- Verified hardware-format output parses correctly via `allsequences_sysex_to_disk` auto-detect path

## v1.11 — Auto-detecting AllSequences SysEx → disk conversion

### New features

- **`allsequences_sysex_to_disk`** / **`sd1_allsequences_sysex_to_disk`**: Single entry point that converts any AllSequences SysEx file to on-disk SixtySequences format without the caller needing to know the source format. Detection is based on `decoded[0..4]` (the first ptr-table entry after nibble-decode): a non-zero value indicates a hardware RAM dump (base 68000 address, e.g. `0x00049000`); zero indicates a library-generated file where ptr-table offsets are cumulative starting at 0. Multi-message files are supported. Dispatches to `allsequences_hardware_sysex_to_disk` or `allsequences_to_disk` accordingly.

### Testing

- 151 unit/integration tests, all passing
- Auto-detect verified against both real hardware dumps (4.syx, Shatterday seq-DB) and synthetic library-generated SysEx; in all cases produces output identical to calling the format-specific function directly

## v1.10 — Hardware SysEx import: fix declared size for files with stale ptr table entries

### Bug fixes

- **`allsequences_hardware_sysex_to_disk` declared size incorrect for files with deleted sequences**: Hardware ptr tables retain non-zero pool offsets for sequences that were subsequently deleted (header marked 0xFF). The `clean_declared` value was computed from all non-zero ptr table entries, making it larger than the event data actually written. Round-trip via `allsequences_to_disk` then failed with `"event data too short for declared seq_data_len"`. Fixed by filtering `sum_ds` to only count slots whose sysex header byte[0] is not 0xFF, matching the same guard used in `padded_total` and the write loop.

### Testing

- 147 unit/integration tests, all passing
- Verified against Shatterday `seq-DB final (all).syx`: a real multi-message file that exhibits stale ptr table entries; now converts and round-trips correctly

## v1.9 — Hardware SysEx import: convert SD-1 RAM dumps to disk format

### New features

- **`allsequences_hardware_sysex_to_disk`**: Converts a hardware AllSequences SysEx dump (nibble-encoded RAM dump as captured by SysEx Librarian or any MIDI librarian) directly to the SD-1 on-disk SixtySequences format. Multi-message files are supported; the function scans for the first AllSequences (0x0A) message and processes it.

- **`decode_sysex_nibbles`**: Public utility that nibble-decodes any SD-1 hardware SysEx data section (`hi << 4 | lo` for each byte pair). Exposed in both the Rust crate and the C FFI.

### Technical details

Hardware AllSequences dumps differ from our generated SysEx in three ways:

1. **Ptr table**: entry 0 is the base RAM address (discarded); entries 1–59 are pool offsets for seqs 0–58 (seq 59 has no ptr table entry and is always undefined in this format).

2. **Event pool preamble**: the hardware pool begins with a 21-byte preamble (12 EVENT_LEAD_ZEROS + 9 bytes of pool-manager state) before sequence data. This preamble is stripped during conversion; the on-disk event section is rebuilt as clean 12-byte lead zeros + packed sequence bytes.

3. **Declared size**: the hardware global's declared field includes the pool preamble. The conversion writes a clean `declared = EVENT_LEAD_ZEROS + sum(ds)` to the disk global, matching what `allsequences_to_disk` produces.

Ds values are derived from the ptr table (not the sequence headers), so conversion is correct even if the hardware headers carry stale or zero ds fields.

### C FFI

- `sd1_decode_sysex_nibbles(data, len, out, out_len) → i32`
- `sd1_allsequences_hardware_sysex_to_disk(raw, raw_len, interleaved_progs, progs_len, out, out_len) → i32`

Both declared in `sd1disk.h`.

### Testing

- 146 unit/integration tests, all passing (up from 135)
- Verified round-trip (hardware SysEx → disk → SysEx → disk) byte-for-byte identical using `~/Downloads/4.syx` (real Ensoniq SD-1 hardware dump, 21 defined sequences)
- Synthetic tests cover: error cases, ds computation, clean declared value, event data preservation, multi-message file scanning

## v1.8 — Hardware format fix: separate SysEx and on-disk header/global sizes

### Bug fixes

- **SD1_ERR_INVALID_SYSEX on all Type:18/19 disk files**: `disk_to_allsequences` and `disk_to_thirty_sequences` failed on every real SD-1 disk file. Root cause: v1.7 correctly fixed SysEx header/global sizes (186/29) but mistakenly applied those same constants to the disk read/write path. The SD-1 on-disk format uses **188-byte headers** (186 SysEx bytes + 2 trailing bytes per slot) and a **21-byte global section** (SysEx global[8..29], stripping 8 bytes of SD-1-internal RAM state not stored on disk). With 186-byte stride, every header past slot 0 was read at the wrong offset, yielding garbage ds values and immediate out-of-bounds errors.

- All four conversion functions now use separate `SYSEX_HEADER_SIZE`/`DISK_HEADER_SIZE` (186/188) and `SYSEX_GLOBAL_SIZE`/`DISK_GLOBAL_SIZE` (29/21) constants. SysEx→disk expands each 186-byte header to 188 bytes (2 trailing zeros) and strips 8 internal bytes from the global. Disk→SysEx strips the trailing 2 bytes per header and prepends 8 zero bytes to the global.

- The bug was pre-existing and went undetected because the unit tests only exercised the library's own round-trip (both sides used the same wrong constants).

### Testing

- 135 unit/integration tests, all passing
- Verified against SWING+SHUFL, ROCK-BEATS, COUNTRY-* (ThirtySequences and SixtySequences) from a real Ensoniq SD1 disk image: all extract without error
- Round-trip (extract → write to blank disk → re-extract) confirmed byte-for-byte identical for both ThirtySequences and SixtySequences files

## v1.7 — Hardware format fix: correct HEADER_SIZE and GLOBAL_SIZE

### Bug fixes

- **System Error 29 on SD-1 hardware**: The SD-1 sequence headers are 186 bytes, not 188, and the global section is 29 bytes, not 21. These wrong constants caused every AllSequences and ThirtySequences SysEx file generated by this library to be rejected by real SD-1 hardware with System Error 29. The SD-1 loads pointer tables and header blocks at exact byte offsets — there is no self-describing length field it can use to compensate.

- **Declared event-area size field corrected**: The size field used by the SD-1 to locate event data is at `global[10..14]` (a BE u32 equal to `EVENT_LEAD_ZEROS + packed_events_size`). The old code read `global[2..6]` and subtracted 252 (`0xFC`). Both access the same physical bytes in the payload, but the wrong subtraction produced a truncated event data window, causing further corruption downstream.

- The bug went undetected in round-trip tests because both the writer and reader used the same wrong constants — only real SD-1 hardware detected the mismatch.

### Testing

- 135 unit/integration tests, all passing
- Verified against a real SD-1 hardware dump (`4.syx`): header strides, global offsets, and declared sizes all confirmed correct at 186/29

## v1.6 — AllSequences pointer table fix + FFI equivalence testing

### Bug fixes

- **AllSequences and ThirtySequences pointer table was all zeros**: The 60-entry pointer table at bytes 0–239 of the AllSequences SysEx payload was being emitted as 240 zero bytes. The SD-1 firmware and MAME use this table directly to locate per-sequence event data — they do not rebuild it from headers on receive. The table is now computed as cumulative 4-byte big-endian byte offsets into the event data area. For ThirtySequences, slots 30–59 all receive the total event data size as their value.

### New features

- **`sd1cli-ffi` binary**: New Rust binary that calls the `sd1ffi` C API functions directly (via rlib linkage) for all disk operations. Produces identical SysEx output to `sd1cli`, confirmed across 36 files covering all supported file types on a reference disk.

### Testing

- 135 unit/integration tests, all passing
- New `scripts/test-equivalence.sh`: extracts every SysEx-capable file from a reference disk using both `sd1cli` and `sd1cli-ffi`, diffs outputs byte-for-byte; 36/36 pass

## v1.5 — Fix sd1_disk_extract truncation for sequence files

### Bug fix

`sd1_disk_extract` now returns the full block-aligned FAT chain data for `SD1_FILE_THIRTY_SEQUENCES` and `SD1_FILE_SIXTY_SEQUENCES` files, instead of truncating to `size_bytes`.

**Root cause:** Hardware-written sequence files store `size_bytes` as the unpadded logical size. Sequence event data is block-padded on disk, so later slots have data at block-aligned offsets that fall past `size_bytes`. Truncating to `size_bytes` before passing to `sd1_disk_to_thirty_sequences` or `sd1_disk_to_allsequences` caused `SD1_ERR_INVALID_SYSEX` for any file where block-padded event data extended past the logical size.

**Impact:** Any caller using `sd1_disk_extract` → `sd1_disk_to_thirty_sequences` / `sd1_disk_to_allsequences` on hardware-written disk images. Files written by sd1diskutil itself were unaffected (our writer sets `size_bytes` to the full padded length). The CLI (`sd1cli extract`) was also unaffected — it already skipped truncation for these types.

### Changes
- `crates/sd1ffi/src/lib.rs` — skip truncation for ThirtySequences/SixtySequences in `sd1_disk_extract`
- `docs/sd1ffi-api.md` — document the block-aligned return size behaviour
- Regression test added

## v1.4.1 — Documentation and tooling additions

### API documentation

Added `sd1_disk_to_thirty_sequences` and `sd1_thirty_sequences_to_disk` to the C/C++ API reference (`docs/sd1ffi-api.md`), including layout notes explaining the key differences from SixtySequences:

- Sequence data always starts at offset 6144 (independent of whether programs are present)
- When programs are embedded, they appear *after* the sequence data rather than before it
- No `has_programs` flag needed for decoding (offset is fixed)

### Torture test script

Added `scripts/torture-write.sh`: recursively writes a directory of SysEx files to SD-1 disk images, rolling over to a new image on `Disk full` or `Directory full`. Validated against 1,083 real-world SD-1 SysEx dumps from a hardware instrument — 1,081 written successfully across 14 disk images, 2 skipped (malformed/non-SD-1 data), 0 failures.

## v1.4 — ThirtySequences support + extraction fixes

### New features

- **ThirtySequences (0x12) read**: `extract` now correctly decodes ThirtySequences files into a 60-slot AllSequences SysEx payload (slots 0–29 from disk; slots 30–59 set to undefined). Previously these were incorrectly wrapped as SingleSequence SysEx.
- **ThirtySequences write**: New `thirty_sequences_to_disk` function converts an AllSequences SysEx payload back to on-disk ThirtySequences format. Programs (if any) are placed after sequence data, matching real hardware layout.
- **TwentyPresets extract**: `extract` now correctly produces AllPresets SysEx from TwentyPresets disk files. Previously TwentyPresets could be written to disk but not extracted.

### Bug fixes

- **CLASS-PNO-\* and other SixtySequences files failed to extract**: The CLI was truncating raw block-chain data to `size_bytes` before decoding. The SD-1 stores the logical SysEx payload length in `size_bytes`, not the on-disk block size. Sequence decoders now receive full block-aligned data.
- **ThirtySequences write-back panicked on real hardware files**: Files written by real SD-1 hardware sometimes have an all-zeros global section (e.g. ROCK-BEATS). The decoder now synthesizes a correct `size_sum` from header slot data when the on-disk global is zeroed, preventing a panic in `allsequences_to_disk`.

### FFI (sd1ffi)

- `sd1_disk_to_thirty_sequences`: decode on-disk ThirtySequences to AllSequences SysEx payload
- `sd1_thirty_sequences_to_disk`: encode AllSequences SysEx payload to on-disk ThirtySequences format

### Testing

- 134 unit/integration tests, all passing
- New `scripts/verify-roundtrip.sh`: round-trips every SysEx-capable file on a disk image (extract → write to blank disk → extract → byte-compare); 36/36 pass on the reference disk

## v1.3 — sd1ffi file-type constants

_Reconstructed 2026-11-03 from the commit history; the GitHub release had no notes._

- `SD1_FILE_*` constants added to the `sd1ffi` C header, with C++ usage examples.
- Unit tests for `sd1_int0_programs` and `sd1_rom_all_programs`.

## v1.2 — sd1ffi: C API for the disk library

_Reconstructed 2026-11-03 from the commit history; the GitHub release had no notes._

- New **`sd1ffi`** crate exposing the `sd1disk` library through a C FFI, with non-prefixed typedef aliases and C/C++ smoke tests.
- Release workflow now packages the `sd1ffi` static library and header alongside `sd1cli`.
- README: pre-built binary downloads and macOS `xattr` instructions.

## v1.1.1 — Release automation

_Reconstructed 2026-11-03 from the commit history; the GitHub release had no notes._

- GitHub Actions release workflow builds cross-platform binaries on tag push (macOS x86_64 via `macos-latest`, Node.js 24 for Actions).
- Updated the list of filename characters the SD-1 can enter.

## v1.1 — "unified front"

Version 1.1 integrates the last function that was still written in Python into the Rust code

 - Adds the dump_programs command, a port of a Python debugging script that's useful to take a peek at 60 sequence/program files: shows all 60 programs embedded in a `SixtySequences+Programs` disk file, and decodes the track-program assignments for the first three defined sequences.

## v1.0 — "blank slate"

This is the first release of the code. Should work on MacOS and Linux; may need tweaks for Windows.
