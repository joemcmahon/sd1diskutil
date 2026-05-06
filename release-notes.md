## AllSequences pointer table fix + FFI equivalence testing

### Bug fixes

- **AllSequences and ThirtySequences pointer table was all zeros**: The 60-entry pointer table at bytes 0–239 of the AllSequences SysEx payload was being emitted as 240 zero bytes. The SD-1 firmware and MAME use this table directly to locate per-sequence event data — they do not rebuild it from headers on receive. The table is now computed as cumulative 4-byte big-endian byte offsets into the event data area. For ThirtySequences, slots 30–59 all receive the total event data size as their value.

### New features

- **`sd1cli-ffi` binary**: New Rust binary that calls the `sd1ffi` C API functions directly (via rlib linkage) for all disk operations. Produces identical SysEx output to `sd1cli`, confirmed across 36 files covering all supported file types on a reference disk.

### Testing

- 135 unit/integration tests, all passing
- New `scripts/test-equivalence.sh`: extracts every SysEx-capable file from a reference disk using both `sd1cli` and `sd1cli-ffi`, diffs outputs byte-for-byte; 36/36 pass
