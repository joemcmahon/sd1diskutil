# Case Study: MUZAK3 Disk Recovery (2026-09-19)

A user provided two flux captures of a suspected damaged Ensoniq SD-1 floppy:
`MUZAK3.hfe` and `sd1_muzak3.scp`, both produced by a Greaseweazle reader in
the same session.

---

## What Was On The Disk

```
ALEX 96      SixtySequences   59 blocks  28,193 bytes
MUZAK 3      OneSequence      12 blocks   5,332 bytes
```

Both files in subdirectory 0 (blocks 15–16), which survived intact.

---

## Step 1 — Try the HFE directly

`sd1cli hfe-to-img` failed immediately:

```
Error: HFE missing sector at track 0 side 0 sector 0
```

Raw inspection of the HFE showed only **255 of 1,600 sectors** readable (15.9%).
No CRC errors on what was there — the data that survived was clean, just very sparse.
The damage pattern was worst on early tracks (0–15), exactly where the OS, FAT, and
directory structures live.

---

## Step 2 — Decode the SCP

The codebase had no SCP support.  We reverse-engineered the format from the file:

- Header: `"SCP"` + metadata at fixed offsets, track table at byte 16
- Track entries: `"TRK\0"` (4-byte magic), then 3 × 12-byte revolution headers
- Revolution header: index time (LE u32), flux count (LE u32), data offset from
  TRK start (LE u32)
- Flux data: **big-endian u16** values in 25 ns ticks between flux reversals

Key insight from inspecting the first flux values (~80, ~160, ~240 ticks):
these map cleanly to 2 µs / 4 µs / 6 µs — the three legal MFM inter-flux
intervals for 250 kbps DD.  Quantizing each value to the nearest 80-tick
multiple gives a clean MFM bitstream.

The SD-1 uses standard IBM/ISO MFM: A1\* sync marks (0x4489), IDAM (0xFE),
DAM (0xFB), CRC16-CCITT.  Our existing HFE decoder logic ported directly.

**SCP result: 1,178 / 1,600 sectors (73.6%).**  The 3-revolution capture
recovered far more than the single-pass HFE.

Saved as `tools/scp_to_img.py`.

---

## Step 3 — Merge SCP + HFE

The HFE is an independent MFM decode from the same Greaseweazle session.
Where its decoder succeeded and our SCP decoder did not, it provides
complementary data.

Cross-checking found **109 sectors the HFE had that the SCP missed** — mostly
whole tracks (T35S1, T51S0, T55S0, T57, T59S1, T61S1, T77S1, T78S0) where
the SCP flux decoder apparently lost sync but the HFE decode succeeded.

**Merged result: 1,287 / 1,600 sectors (80.4%).**

Saved as `tools/merge_scp_hfe.py`.

---

## Step 4 — Assess the files

Despite 80% overall recovery, both files were in the most-damaged zone
(early tracks 0–14):

| File | Blocks | Readable | Status |
|---|---|---|---|
| ALEX 96 | 59 | 19 (32%) | Severely damaged |
| MUZAK 3 | 12 |  5 (42%) | Unrecoverable |

**FAT damage** (blocks 5–14): FAT block 6 (covering entries ~171–340) was
unrecovered, breaking MUZAK 3's chain after the first block.  The contiguous
block count in the directory entry let us bypass the FAT and work directly
with physical blocks.

**MUZAK 3 (OneSequence):** 7 of 12 blocks missing, including a contiguous run
from bytes 512–2559.  The SD-1 cannot load a sequence with holes.  Unrecoverable.

**ALEX 96 (SixtySequences):** 40 of 59 blocks zero.  The 60-slot header region
(blocks 23–44) was mostly destroyed; only headers for slots 0 and 1 survived
intact.  Slot 1 had a readable header but its event data fell in a missing block.

Saved as `tools/triage_sequences.py`.

---

## Step 5 — Extract the one intact sequence

SixtySequences layout places the event pool at file offset 11,776, with
sequences stored in slot order, each padded to a 512-byte boundary.  Slot 0's
event data (70 bytes) lands in the very first event-pool block (disk block 46),
which was recovered.

Wrapped the 70 event bytes as a SingleSequence SysEx
(`F0 0F 05 00 00 09 [nybblized] F7`) and saved to `extracted/ALEX_96_slot00.syx`.

Saved as `tools/extract_intact_sequences.py`.

---

## What Was Recovered

- **1 sequence** from ALEX 96, slot 0, 70 bytes — valid, loadable by SD-1 hardware.
- Everything else on the disk is unrecoverable.

---

## Damage Pattern

The failure is consistent with **magnetic decay from the outside in**: early
tracks (0–15) are worst, later tracks (30+) are largely intact, outermost
tracks (55–78) have scattered whole-track losses.  The critical OS/FAT/directory
structures on tracks 0–1 took the worst hit.

The disk was readable enough to identify and partially decode, but the files
themselves were stored in the most vulnerable zone.

---

## Lessons / Notes for Future Recovery Attempts

1. **Always capture SCP + HFE** in the same Greaseweazle session.  The two
   decoders have complementary failure modes and together recovered 109 more
   sectors than either alone.

2. **More SCP revolutions help.**  This file had 3; 5 or more is better for
   marginal sectors.  Greaseweazle supports `--revs N`.

3. **The contiguous block count in the directory survives FAT corruption.**
   When FAT chains are damaged, the `contiguous_blocks` field in the
   directory entry lets you still locate file data.

4. **Early track damage is the worst case** for SD-1 disks.  The OS loader,
   FAT (blocks 5–14), and directory (blocks 15–22) are all on tracks 0–1.
   A disk that looks mostly readable (80%) can still have completely
   inaccessible files if those tracks are hit.

5. **SCP format gotcha:** the `"TRK"` magic is 4 bytes (`TRK\0`), not 3.
   The flux count and data offset are both relative to the TRK header start,
   not the file start.  Flux values are **big-endian** u16 despite the rest of
   the SCP header being little-endian.
