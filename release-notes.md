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
