---
date: 2026-03-24T04:54:46Z
session_name: general
researcher: Claude
git_commit: 9ca196c
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Utility — AllSequences on-disk format research; binary analysis in progress"
tags: [rust, ensoniq, sd-1, disk-image, sysex, allsequences, format, reverse-engineering]
status: in_progress
last_updated: 2026-03-24
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: AllSequences format research — structural analysis done, pointer table interpretation pending

## Task(s)

**COMPLETED: Found the Giebler VFX-SD Sequencer File Formats articles**
- Transoniq Hacker Issue #77 (Nov 1991) — Part I: file layouts, header format, track offset table
- Transoniq Hacker Issue #78 (Dec 1991) — Part II: event types, track data format
- Full reference doc saved to `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md`

**IN PROGRESS: Binary analysis of SysEx payload vs on-disk format**
- SysEx file: `/Volumes/Aux Brain/Ready for SSD/Music/Music related/Music/SysEx Librarian/sequences/seq-countryseq.syx`
- On-disk reference: `disk_with_everything.img`, COUNTRY-* at block 1360, size=58983 bytes
- Key structural findings below; pointer table interpretation still unclear

**NOT STARTED: Implement AllSequences write in Rust**

## Critical References

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — AUTHORITATIVE FORMAT SPEC (from Giebler articles)
- `crates/sd1cli/src/main.rs:264-268` — AllSequences skip/warning to be replaced
- `disk_with_everything.img` — reference disk (COUNTRY-* = SixtySequences+Programs)

## Binary Analysis Findings

### SysEx AllSequences payload structure (seq-countryseq.syx, 2 packets)
- Packet 0: Command type (0x00), 5-byte payload (announces sequence data)
- Packet 1: AllSequences type (0x0A), model=0x01, denybblized payload = **35885 bytes**

**Payload layout (CONFIRMED):**
```
Bytes 0–239:     "Pointer table" (60 × 4-byte values, interpretation unclear — see below)
Bytes 240–24583: Sequence event data (24344 bytes)
Bytes 24584–35864: 60 × 188-byte sequence headers (IDENTICAL FORMAT to on-disk headers)
Bytes 35864–35885: 21-byte global section (IDENTICAL to on-disk global)
```

Global section breakdown (last 21 bytes):
- [0–1]: Current selected sequence number (2 bytes BE)
- [2–5]: Sum of all seq data sizes + 0xFC (4 bytes BE)
- [6–20]: Global sequencer information (15 bytes)

### SysEx Sequence Headers (bytes 24584–35864)
Same 188-byte format as on-disk. Defined sequences found:
| Slot | Loc | Name | Data Size | Ptr |
|------|-----|------|-----------|-----|
| 0 | 0 | `$ COUNTRY  ` | 170 | 0x019000 (OUT OF RANGE) |
| 1 | 1 | (binary name) | 1318 | 21 |
| 2 | 59 | (binary name) | 2400 | 252 |
| 3 | 3 | (binary name) | 4220 | 422 |
| 4 | 4 | (binary name) | 1134 | 1740 |
| 5 | 59 | (binary name) | 2340 | 4140 |
| 6 | 59 | (binary name) | 2948 | 8360 |
| 7 | 7 | (binary name) | 2864 | 9494 |
| 8 | 8 | `$ACT 2     ` | 170 | 11834 |
| 54 | 54 | `SEQUENCE-54` | 64 | 0 (undefined) |
| 55 | 54 | (binary name) | 6576 | 0 (undefined) |

Note: Slot 0 and Slot 8 are SONGS (name starts with `$` = 0x24). Their ptr=0x019000 is out-of-range.

### On-disk COUNTRY-* structure
COUNTRY-* is actually a **Sixty Seq + 60 Programs** file (not just sequences):
```
Bytes 0–11279:    60 × 188-byte sequence headers
Bytes 11280–11285: Global section (curr_seq=0, sum=16134)
Bytes 11286–11300: Global sequencer info
Bytes 11301–11775: Zeros (475 bytes)
Bytes 11776–43575: 60 interleaved programs (530 × 60 = 31800 bytes)
Bytes 43576–44031: Zeros (456 bytes)
Bytes 44032+:     Sequence data section
End-of-Track events at: 44200, 44616, 44730, 45096, 45130, 45304, 45450, 45540, 45640, 46386, 46548, 46926, 47760, 47906, 48200, 48350
```

### Pointer table mystery (UNCONFIRMED)
The 60 × 4-byte values at bytes 0–239 of the SysEx payload do NOT have an obvious consistent interpretation:
- Slot 0: 0x019000 = 102400 — clearly not a byte offset (payload is only 35885 bytes)
- Slots 1-10, 56-57: values 21–17880 — could be byte offsets into the event data section

**Hypothesis A** (offset from byte 240): ptr + 240 = absolute offset in payload. Slot 2 ptr=252 → event data at payload[492].

**Hypothesis B** (absolute offset from byte 0): ptr = absolute offset in payload. Slot 2 ptr=252 → data at payload[252].

**Key observed fact:** `on-disk[44032:44036] = SysEx[252:256] = 00 00 00 aa` (both have `0x0000AA = 170`). This 170 = data_size of Slot 0 header (the Country Song). This may mean:
- The on-disk sequence section starts at the same position as the POINTER for Slot 2 in the SysEx (both are 252 bytes in from their respective sections)
- OR it's a coincidence (both encode data_size=170 at different positions)

**Critical observation:** Seq 3 ptr (422) = Seq 2 ptr (252) + 170 = 422. This means:
- IF ptrs are absolute byte offsets, seq 2's data ends at 252+170=422 where seq 3 starts
- Size of seq 2 block = 170 bytes
- But seq 2's HEADER says data_size=2400. MISMATCH.
- This means slot 2's header data_size ≠ the data block size at its pointer location
- OR the pointer table slot numbers != the header slot numbers (the table may be indexed differently)

### Next binary analysis needed
1. Compare `SysEx[240:]` bytes directly with `on-disk[44032:]` bytes to see if they're the same raw data
2. Check if SysEx sequence data is the SAME bytes as on-disk, just shifted
3. Identify the relationship between pointer values and actual data positions

## Learnings

### On-disk SixtySequences may contain programs
COUNTRY-* is type 0x13 (SixtySequences) but contains 60 programs embedded at byte 11776-43575. The Giebler article describes both "No Programs" and "60 Programs" variants; both have file_type = SixtySequences. For AllSequences SysEx write (which has no programs), use the **No Programs** layout.

### Sequence header format confirmed identical between SysEx and on-disk
Same 188-byte layout described in the Giebler article. Name starts at byte 2, data_size at bytes 183-185.

### Song sequences use name[0] = 0x24 ('$')
The `$ COUNTRY  ` name (byte 2 = 0x24) signals a Song. Songs have a special internal memory address (0x019000) that is likely NOT included in the SysEx AllSequences dump. Both Slot 0 and Slot 8 are Songs with out-of-range ptrs.

### SysEx AllSequences is 2-packet
- Packet 0: Command (type 0x00) announces sequence data
- Packet 1: AllSequences (type 0x0A, model 0x01) contains all data

## Post-Mortem

### What Worked
- Finding Giebler articles: Issue #71 TOC showed "Ensoniq Floppy Diskette Formats" in #73, and #75 had Part III. #77 had the sequencer article. Quick cover-scan method worked.
- Header format confirmed identical between SysEx and on-disk (byte-for-byte match at first header)
- On-disk structure of COUNTRY-* confirmed to have programs section at 11776 (not just sequences)

### What Failed
- Pointer table interpretation: multiple hypotheses, no definitive answer yet
- Context ran out before completing binary comparison of SysEx event data vs on-disk event data

### Key Decisions
- **Research doc saved**: `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — complete format spec
- **Confirmed binary approach**: Compare SysEx and on-disk bytes directly rather than speculating about pointer semantics

## Artifacts

- `thoughts/shared/research/vfxsd-sd1-sequencer-file-formats.md` — complete Giebler article content

## Action Items & Next Steps

### Immediate: Complete binary analysis
```python
# Load SysEx payload
# Load on-disk COUNTRY-* at disk[1360*512 : 1360*512 + 58983]
# Compare SysEx[240:] with disk[44032:]
# If they match directly: SysEx event data = on-disk event data (just copy!)
# If shifted: find the exact offset
# Key: look for 00 00 00 aa 00 00 00 00 [60 bytes zeros] in both
```

### Then: Plan AllSequences write implementation

From what we know, the likely conversion from SysEx → on-disk is:

1. **Extract headers**: SysEx[-11301:-21] = 60 × 188-byte headers → write to on-disk[0:11280]
2. **Extract global section**: SysEx[-21:] = 21 bytes → write to on-disk[11280:11301]
3. **Write zeros**: on-disk[11301:11776] = 475 zeros
4. **Write sequence event data**: SysEx[240:] (or adjusted offset) → on-disk[11776:]

The implementation should be in a new function in `crates/sd1disk/src/types.rs`:
```rust
pub fn allsequences_to_disk(sysex_payload: &[u8]) -> Result<Vec<u8>>
```

Then wire it up in `crates/sd1cli/src/main.rs:264-268` (replacing the skip/warning).

### file_number incrementing (low priority)
Still writing `file_number: 0` always. Not a crash risk.

## Other Notes

**SysEx test files:**
- Full dumps: `/Volumes/Aux Brain/Ready for SSD/Music/Music related/Music/SysEx Librarian/seq-*.syx`
- Sequence-only: same path + `sequences/seq-countryseq.syx`, `seq-rockseq1.syx`, `seq-playseq1.syx`

**Test commands:**
```
cargo test                                      # 50 tests must pass
cargo run -p sd1cli -- inspect-sysex <file>
cargo run -p sd1cli -- write /tmp/test.img <file.syx> [--name NAME]
cargo run -p sd1cli -- list /tmp/test.img
```

**On-disk reference files (disk_with_everything.img):**
- COUNTRY-* (SixtySequences+Programs): SubDir0 slot37, first_block=1360, size=58983
- ROCK-BEATS (ThirtySequences): SubDir0 slot38, first_block=810, size=12118
- SD1-PALETTE (SixtySequences): SubDir1 slot4, first_block=979, size=77057
- CLASS-PNO-* (SixtySequences): SubDir1 slot7, first_block=1455, size=12877
- BASSICS-* (SixtySequences): SubDir1 slot8, first_block=1482, size=46951

**Reference disk sequence file sizes suggest:**
- ROCK-BEATS (ThirtySequences, 12118 bytes): No programs → 12118 - 6144 = ~5974 bytes of seq data
- CLASS-PNO-* (SixtySequences, 12877 bytes): Small → likely No Programs → 12877 - 11776 = ~1101 bytes

**Pointer table alternative interpretation not yet tried:**
What if the 60 × 4-byte "pointer table" is actually the TRACK OFFSET TABLE for the first (Song) sequence's data block? 60 entries × 4 bytes = 240 bytes. But Giebler says track offset table = 15 × 4 = 60 bytes per sequence, so 60 entries doesn't match.

**Recommended next step:**
Just compare the raw bytes:
```python
with open('disk_with_everything.img','rb') as f:
    f.seek(1360*512 + 44032)
    disk_seqdata = f.read(14951)  # 58983 - 44032

# denybblize SysEx...
sysex_seqdata = payload[240:]  # or payload[0:]

# Try different offsets
for offset in range(300):
    if disk_seqdata[:50] == sysex_seqdata[offset:offset+50]:
        print(f"Match at offset {offset}")
```
