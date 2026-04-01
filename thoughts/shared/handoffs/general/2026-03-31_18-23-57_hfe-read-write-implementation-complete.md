---
date: 2026-03-31T18:23:57-07:00
session_name: general
researcher: Claude
git_commit: 3536ab9
branch: main
repository: sd1diskutil
topic: "HFE v1 Read/Write Support — Implementation Complete"
tags: [hfe, mfm, implementation, complete]
status: complete
last_updated: 2026-03-31
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: HFE v1 read/write fully implemented and merged to main

## Task(s)

1. **Write implementation plan from HFE design spec — COMPLETED**
   Plan saved at `docs/superpowers/plans/2026-03-31-hfe-read-write.md`. 9 tasks, TDD throughout.

2. **Implement HFE v1 read/write support — COMPLETED**
   All code merged to `main` in 4 commits. 74 tests passing, 0 failures.
   Validated against `Ensoniq.hfe` (real hardware-written SD-1 disk).

## Critical References

- `docs/superpowers/specs/2026-03-30-hfe-support-design.md` — approved design spec (read first)
- `crates/sd1disk/src/hfe.rs` — full implementation (771 lines)
- `docs/superpowers/plans/2026-03-31-hfe-read-write.md` — implementation plan (reference for next work)

## Recent changes

- `crates/sd1disk/src/error.rs` — added 3 new variants: `InvalidHfe(&'static str)`, `HfeCrcMismatch { track, side, sector }`, `HfeMissingSector { track, side, sector }` with Display arms and tests (commit `6d6fc66`)
- `crates/sd1disk/src/hfe.rs` — new file, 771 lines: full HFE v1 read/write, MFM encode/decode, 14 unit tests (commit `167ef8c`, fixed `3536ab9`)
- `crates/sd1disk/src/lib.rs` — added `pub mod hfe; pub use hfe::{read_hfe, write_hfe};`
- `crates/sd1cli/src/main.rs` — added `HfeToImg` and `ImgToHfe` CLI subcommands with handler functions (commit `7bf0a47`)

## Learnings

### HFE format
- Magic: `HXCPICFE`, format revision byte at offset 8 (must be 0 for v1)
- Header offset 17 = `dnu` ("do not use") field — must be `0xFF`, not `0x00`
- Track lookup table at offset `track_list_block × 512`; each entry is 4 bytes: `u16 block_offset` + `u16 byte_length` (little-endian)
- Side 0 and side 1 are interleaved in 256-byte chunks within each track's storage block
- `byte_length` in the TLT = 25,044 (actual content bytes); track storage is 49 × 512 = 25,088 (last 44 bytes padding)

### MFM encoding
- HFE stores bits LSB-first per byte (bit 0 = oldest bit encountered by read head)
- A1* sync mark = `[0x22, 0x91]` in HFE bytes; after emitting, set `prev_bit = 1`
- `encode_byte`: for each data bit `d` (MSB first), clock `c = !(prev_bit | d)`. Emit `(c, d)` time-ordered, pack LSB-first
- Data bits in MFM stream are at odd time positions (1, 3, 5, 7, 9, 11, 13, 15)
- CRC16-CCITT: poly 0x1021, init 0xFFFF

### Track geometry (per side, 12,522 encoded bytes)
- Fixed preamble: Gap4a(80×0x4E→160B) + Sync(12×0x00→24B) + Gap1(50×0x4E→100B) = 284B
- Per sector fixed (no gap3): 1148 encoded bytes
- Total fixed: 284 + 10×1148 = 11,764B. Slack: 12,522 − 11,764 = 758B for 10 gap3s
- Sectors 0–8: gap3 = 75 encoded bytes each (37 data bytes × 2); sector 9 absorbs remainder

### Block mapping (confirmed against blank_image.img and Ensoniq.hfe)
```
block = track×20 + side×10 + sector
track  = block / 20
side   = (block / 10) % 2
sector = block % 10
```
Sectors are 0–9 (Ensoniq), **not** 1–10 (PC).

### Ensoniq.hfe (real disk at /Users/joemcmahon/Downloads/Ensoniq.hfe)
- Contains OMNIVERSE (OneProgram), SOPRANO-SAX (OneProgram), 60-PRG-FILE (SixtyPrograms)
- Free block count: 1510
- SubDir 3 has 2 garbage entries (blank names, huge byte counts, 0 blocks) — pre-existing, unrelated to HFE

### Implementation approach that worked
- Subagent-driven development on `feat/hfe-support` worktree
- The first implementer subagent ran long and wrote the entire 771-line hfe.rs (Tasks 2–8) plus CLI (Task 8) in a single pass before hitting context limits
- Final code reviewer caught: (1) header offset 17 = 0x00 should be 0xFF, (2) inaccurate arithmetic comment — both fixed

## Post-Mortem

### What Worked
- **Worktree isolation**: `feat/hfe-support` worktree prevented any risk to main during development
- **Plan-first approach**: Writing the complete plan with concrete code before any implementation meant the subagent had everything it needed to proceed without questions
- **Code reviewer catch**: The final reviewer caught the `header[17] = 0x00` bug that would have produced non-conformant HFE files; the fix is a one-liner at `hfe.rs:544`
- **Integration test against real hardware disk**: Running `hfe-to-img` on `Ensoniq.hfe` and getting correct output (OMNIVERSE, SOPRANO-SAX, 60-PRG-FILE, 1510 free blocks) gave high confidence the implementation is correct

### What Failed
- Task 2 subagent asked "Shall I proceed?" rather than proceeding immediately — minor friction, required a confirmation message
- Subagent hit API rate limit mid-session; files were written but not committed; required manual inspection and commit on resume

### Key Decisions
- **HFE in `sd1disk` crate, not a new crate**: HFE is a serialization format for `DiskImage`; belongs alongside `image.rs`. New crate would be premature.
- **Two explicit CLI commands** (`hfe-to-img`, `img-to-hfe`): mirrors existing verb-per-operation pattern; avoids magic extension detection.
- **`extract_side` returns silently truncated last chunk**: Last 22 bytes of side 1's final chunk fall in the 44-byte padding zone of the interleaved track. This is benign for all Ensoniq SD-1 disks (data ends well before the padding zone). Noted by code reviewer as a latent issue for third-party HFE with data near the track boundary.
- **IDAM track/side fields not re-validated**: CRC catches corruption; field mismatch for valid-CRC transposed IDAMs is out of scope for this implementation.

## Artifacts

- `docs/superpowers/plans/2026-03-31-hfe-read-write.md` — implementation plan (now complete)
- `docs/superpowers/specs/2026-03-30-hfe-support-design.md` — design spec (reference)
- `crates/sd1disk/src/hfe.rs` — full implementation
- `crates/sd1disk/src/error.rs:28-37` — 3 new HFE error variants
- `crates/sd1cli/src/main.rs:128-175` — HfeToImg/ImgToHfe command variants and handlers

## Action Items & Next Steps

This feature is complete. Possible follow-on work:

1. **Sojus VST3 fixed `esq16_dsk.cpp` integration**: Once released, users can use `sd1cli img-to-hfe` + Sojus's fixed plugin. No code changes needed.
2. **HFE v2/v3 support**: Not needed for Ensoniq SD-1; out of scope per spec.
3. **Auto-detection of `.hfe` vs `.img`**: Explicitly out of scope per spec; would require adding magic detection to `DiskImage::open`.
4. **`extract_side` defensive assert**: The reviewer suggested adding `debug_assert_eq!(side_stream.len(), SIDE_LEN)` in `decode_track_side` to surface the 22-byte truncation in debug builds. Low priority.

## Other Notes

- The Python MFM decoder written during the prior session (not committed) is described in the previous handoff at `thoughts/shared/handoffs/general/2026-03-30_18-11-06_hfe-read-write-design-spec.md`. Useful if the Rust implementation ever needs debugging.
- `blank_image.img` embedded in `sd1disk` is a real hardware-formatted disk and is ground truth for block layout.
- The Sojus VST3 MAME bug (corrupted `.img` files) is **not repairable** — sector 0 data is destroyed. The correct mitigation is to use `.hfe` output from Sojus (unaffected). This is documented in the spec and in the `img-to-hfe` help text.
- Test run command: `cargo test` (from workspace root) — 74 tests, ~0.7s
