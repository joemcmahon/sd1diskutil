Recovering a damaged Ensoniq disk...well, trying, anyway

I got a mail in early September from Alex Crabb, who had downloaded the sd1diskutil code from GitHub, and was trying to image some 35-year-old disks from his dad's SD-1. (Pause while I feel old for a minute.)

He was able, using a GreaseWeazel setup, to get some MFM flux transition files from the one disk he could not get to read, and aske me if
I could see if we could get anything off of it.

We had both an HFE file (which sd1diskutil DOES know how to read directly) and an SCP multi-pass read one, which at the time I got the files, it did *not* know how to read. 

---

## Step 1 — Try the HFE directly

`sd1cli hfe-to-img` failed immediately:

```
Error: HFE missing sector at track 0 side 0 sector 0
```

Raw inspection of the HFE showed only **255 of 1,600 sectors** readable (15.9%). This was pretty bad news already, and I was worried we'd
not be able to get anything at all off the disk. Fortunately, there were no CRC errors on what was there — the data that survived was clean, just very sparse. The damage pattern was worst on early tracks (0–15), exactly where the OS, FAT, and
directory structures live. Ouch.

BUT! We could see some files!

```
ALEX 96      SixtySequences   59 blocks  28,193 bytes
MUZAK 3      OneSequence      12 blocks   5,332 bytes
```

Both files in subdirectory 0 (blocks 15–16), which survived intact. Optimism surged.

---

## Step 2 — Decode the SCP

As mentioned, the codebase had no SCP support. No problem. I threw Claude at ait, and we were able to reverse-engineer the format:

- Header: `"SCP"` + metadata at fixed offsets, track table at byte 16
- Track entries: `"TRK\0"` (4-byte magic), then 3 × 12-byte revolution headers
- Revolution header: index time (LE u32), flux count (LE u32), data offset from
  TRK start (LE u32)
- Flux data: **big-endian u16** values in 25 ns ticks between flux reversals

Inspecting the first few flux values gave us our in -- ~80, ~160, ~240 ticks mapped
cleanly to 2 µs / 4 µs / 6 µs, which are the three legal MFM inter-flux intervals for 250 kbps DD.
Excellent! We're not fumbling in the dark altogether. Quantizing these to the nearest 80-tick multiple gave us a clean MFM bitstream.

The SD-1 uses standard IBM/ISO MFM: A1\* sync marks (0x4489), IDAM (0xFE),
DAM (0xFB), CRC16-CCITT, so our existing HFE decoder logic ported directly. Lucked out big.

**SCP result: 1,178 / 1,600 sectors (73.6%).**  This was a 3-revolution capture from the disk, and it recovered a lot more than the single-pass HFE.

Claude's script to do this was saved as `tools/scp_to_img.py`.

Lots more sectors! Was it enough?

Alas, it was not. Still a ton of damage where we really needed it: in the directories.

---

## Step 3 — Merge SCP + HFE

We did our won Hail Mary here, on Claude suggestion, and tried merging the HFE MFM decode with the SCP one. They're both from the same Greaseweazle session, and the guess was maybe one had data the other did not.

Cross-checking found **109 sectors the HFE had that the SCP missed** — mostly whole tracks (T35S1, T51S0, T55S0, T57, T59S1, T61S1, T77S1, T78S0) where the SCP flux decoder apparently lost sync but the HFE decode succeeded. Would this image make the cut?

**Merged result: 1,287 / 1,600 sectors (80.4%).**

Saved this new tool as `tools/merge_scp_hfe.py`.

---

## Step 4 — Assess the files

Sadly, no.

Despite 80% overall recovery, both files were in the most-damaged zone (early tracks 0–14):

| File | Blocks | Readable | Status |
|---|---|---|---|
| ALEX 96 | 59 | 19 (32%) | Severely damaged |
| MUZAK 3 | 12 |  5 (42%) | Unrecoverable |

DRAT.

**FAT damage** (blocks 5–14): FAT block 6 (covering entries ~171–340) was
unrecovered, breaking MUZAK 3's chain after the first block.  The contiguous
block count in the directory entry let us bypass the FAT and work directly
with physical blocks.

**MUZAK 3 (OneSequence):** 7 of 12 blocks missing, including a contiguous run
from bytes 512–2559.  The SD-1 cannot load a sequence with holes.  Unrecoverable.

**ALEX 96 (SixtySequences):** 40 of 59 blocks zero.  The 60-slot header region
(blocks 23–44) was mostly destroyed; only headers for slots 0 and 1 survived
intact.  Slot 1 had a readable header but its event data fell in a missing block.

Saved this recovery extractor as `tools/triage_sequences.py`.

---

## Step 5 — Extract the one intact sequence

SixtySequences layout places the event pool at file offset 11,776, with
sequences stored in slot order, each padded to a 512-byte boundary.  Slot 0's
event data (70 bytes) lands in the very first event-pool block (disk block 46),
which was recovered.

Wrapped the 70 event bytes as a SingleSequence SysEx
(`F0 0F 05 00 00 09 [nybblized] F7`) and saved to `extracted/ALEX_96_slot00.syx`.

Saved this very custom tool as as `tools/extract_intact_sequences.py`.

---

## The final breakdown

- **1 sequence** from ALEX 96, slot 0, 70 bytes — valid, loadable by SD-1 hardware.
- Everything else on the disk is unrecoverable.

Had to report this sad new to Alex, who said, "I am going to check the Logic projects I did a long time ago in early 2025 because I’m almost positive that disk was one of the ones I was able to load onto my dad’s SD-1 and play the sequence into my DAW, track by track, syncing everything up to the count off clicks because I could not for the life of me figure out how to convert the data to midi without having to go the Giebler route."

So, in summary good news! The data had been saved elsewhere by other means, and the failure to read the disk wasn't a complete loss.

---

## Lessons / Notes for Future Recovery Attempts

The failure is consistent with **magnetic decay from the outside in**, very common as floppies age: early
tracks (0–15) are worst, later tracks (30+) are largely intact, outermost tracks (55–78) have scattered whole-track losses.  The critical OS/FAT/directory structures on tracks 0–1 took the worst hit.

The disk was readable enough to identify and partially decode, but the files
themselves were sadly stored in the most vulnerable zone.

So now we know what to do if we want to try recovering old disks in the future:

1. **Always capture SCP + HFE** in the same Greaseweazle session.  
   The two decoders have complementary failure modes and together recovered 109 more
   sectors than either alone.

2. **More SCP revolutions help.**  
   This file had 3; 5 or more is better for marginal sectors.  Greaseweazle supports `--revs N`.
   There's no reason not to do twn or twenty other than total file size.

3. **The contiguous block count in the directory survives FAT corruption.**
   When FAT chains are damaged, the `contiguous_blocks` field in the
   directory entry lets you still locate file data.

4. **Early track damage is the worst case** for SD-1 disks.  
   The OS loader, FAT (blocks 5–14), and directory (blocks 15–22) all live on tracks 0–1.
   A disk that looks mostly readable (80%) can still have completely inaccessible files if those tracks have failed.

5. **SCP format gotcha:** the `"TRK"` magic is 4 bytes (`TRK\0`), not 3.
   The flux count and data offset are both relative to the TRK header start,
   not the file start.  Flux values are **big-endian** u16 despite the rest of
   the SCP header being little-endian.

---

## Final thoughts

Despite our not getting the data, at least Alex was able to find out what was on there (at least partially), and 
know he had it from other sources.

It wasn't a completely useless exercise, as we've been able to add some more recovery tools to the library's
arsenal.

I did find it interesting that Claude generally prefers to use Python over reusing the library code; I suspect that's
because it's easier to reason from a much larger corpus of Python than Rust.

In any case, we've significantly expanded the capabilities of the library to recovery from just format conversion, and
that was a worthwhile thing to spend time doing.