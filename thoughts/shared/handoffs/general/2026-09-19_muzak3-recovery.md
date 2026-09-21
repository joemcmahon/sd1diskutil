# Handoff: MUZAK3 Disk Recovery Session (2026-09-19)

## Status: Complete

Session goal: attempt data recovery from two flux captures of a damaged
Ensoniq SD-1 floppy provided by a user (`MUZAK3.hfe`, `sd1_muzak3.scp`).

---

## What Was Done

### 1. HFE failed immediately
`sd1cli hfe-to-img MUZAK3.hfe` → `HFE missing sector at track 0 side 0 sector 0`.
Manual scan: only 255/1600 sectors readable. File too damaged to use directly.

### 2. SCP decoder written from scratch
SCP format had no existing support in the codebase. Reverse-engineered from
the file and written as `tools/scp_to_img.py`. Key format notes:
- `"TRK\0"` magic is 4 bytes (not 3)
- Flux values are **big-endian** u16, 25 ns ticks
- Data offsets are relative to TRK header start
- 3 revolutions per track captured; each is decoded independently and sectors
  are merged (first clean CRC wins)

SCP result: **1,178 / 1,600 sectors (73.6%)**.

### 3. SCP + HFE merger written
`tools/merge_scp_hfe.py` takes both sources, prefers SCP data, fills gaps from
HFE. HFE contributed 109 additional sectors (mostly whole tracks where SCP lost
sync but HFE decode succeeded).

Merged result: **1,287 / 1,600 sectors (80.4%)**.
Output: `MUZAK3_merged.img`

### 4. File triage
`tools/triage_sequences.py` walks a (possibly partial) disk image and reports
which SixtySequences / OneSequence slots are fully intact.

Both files on disk were in the most-damaged zone (tracks 0–14):
- **MUZAK 3** (OneSequence, 12 blocks): 7 blocks missing mid-file. Unrecoverable.
- **ALEX 96** (SixtySequences, 59 blocks): 40 blocks missing. Only slot 0
  survived (header + event data both intact, 70 bytes).

### 5. Extraction
`tools/extract_intact_sequences.py` extracted slot 0 as a standalone
OneSequence SysEx: `extracted/ALEX_96_slot00.syx` (147 bytes, valid F0…F7).

---

## Files Created / Modified

| Path | Description |
|---|---|
| `tools/scp_to_img.py` | SCP flux → .img converter |
| `tools/merge_scp_hfe.py` | Merge SCP + HFE into best-combined .img |
| `tools/triage_sequences.py` | Identify intact sequences in damaged image |
| `tools/extract_intact_sequences.py` | Extract intact slots as OneSequence SysEx |
| `tools/README.md` | Workflow docs for all four tools |
| `docs/case-studies/2026-09-19-muzak3-recovery.md` | Full case study write-up |
| `extracted/ALEX_96_slot00.syx` | The one recovered sequence |
| `MUZAK3_merged.img` | Best-effort recovered disk image |
| `MUZAK3_recovered.img` | SCP-only recovered disk image (intermediate) |

Source files `MUZAK3.hfe` and `sd1_muzak3.scp` remain in repo root (untracked).

---

## Nothing Is Broken

No existing source files were modified. All new code is in `tools/` (Python
utilities) and `docs/`. Rust codebase and tests are untouched.

---

## If Continuing This Work

- The `tools/` scripts are standalone Python 3.10+, no dependencies beyond stdlib.
- The SCP decoder in `scp_to_img.py` could be ported to Rust and added as a
  `scp-to-img` CLI subcommand — same architecture as the existing `hfe-to-img`.
- The triage/extraction tools assume SixtySequences only; ThirtySequences
  support would need the different header region size (30 × 188 = 5,640 bytes,
  event data starts at 6,144).
- `MUZAK3_merged.img` and `MUZAK3_recovered.img` are untracked — safe to delete
  after the user has `ALEX_96_slot00.syx`.
