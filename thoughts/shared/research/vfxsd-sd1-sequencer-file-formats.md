# VFX-SD and SD-1 Sequencer File Formats

**Source:** Transoniq Hacker, Issues #77 (November 1991) and #78 (December 1991)
**Author:** Gary Giebler

---

## File Type Summary

- One Seq/Song File: type 0x11
- Thirty Seq/Song File (no programs): type 0x12
- Sixty Seq/Song File (no programs): type 0x13
- System Exclusive File: type 0x14

---

## One Seq/Song File Layout

| Offset    | Contents |
|-----------|----------|
| 000–187   | Sequence Header (see below) |
| 188–511   | Unused (zeros) |
| 512–575   | Sequence Data Offsets (see Track Offsets below) |
| 576–xxx   | Sequence Track Data |
| xxx–zzz   | Unused (zeros) |

---

## Thirty Seq/Song File Layouts

### No Programs
| Offset      | Contents |
|-------------|----------|
| 0000–5639   | Sequence Headers for 30 sequences (30 × 188 bytes) |
| 5640–6143   | Unused (zeros) |
| 6144–xxxx   | Sequence Data — Offsets & Track Data for defined seqs |
| xxxx–zzzz   | Unused (zeros) |

### 30 Programs
| Offset      | Contents |
|-------------|----------|
| 0000–5639   | Sequence Headers for 30 sequences |
| 5640–6143   | Unused (zeros) |
| 6144–22043  | 30 Programs (530 bytes each) |
| 22044–22527 | Unused (zeros) |
| 22528–xxxxx | Sequence Data — Offsets & Track Data for defined seqs |
| xxxxx–zzzzz | Unused (zeros) |

### 60 Programs
| Offset      | Contents |
|-------------|----------|
| 0000–5639   | Sequence Headers for 30 sequences |
| 5640–6143   | Unused (zeros) |
| 6144–37943  | 60 Programs (530 bytes each — mixed together) |
| 37944–38399 | Unused (zeros) |
| 38400–xxxxx | Sequence Data — Offsets & Track Data for defined seqs |
| xxxxx–zzzzz | Unused (zeros) |

---

## Sixty Seq/Song File Layouts

### No Programs
| Offset        | Contents |
|---------------|----------|
| 00000–11279   | Sequence Headers for 60 sequences (60 × 188 = 11280 bytes) |
| 11280–11281   | Current Selected Sequence Number (2 bytes) |
| 11282–11285   | Sum of All Sequence Data Sizes + 0xFC (4 bytes) |
| 11286–11300   | Global Sequencer Information (15 bytes) |
| 11301–11775   | Unused (zeros, 475 bytes) |
| 11776–xxxxx   | Sequence Data — Offsets & Track Data for defined seqs |
| xxxxx–zzzzz   | Unused (zeros) |

### 60 Programs
| Offset        | Contents |
|---------------|----------|
| 00000–11279   | Sequence Headers for 60 sequences |
| 11280–11281   | Current Selected Sequence Number |
| 11282–11285   | Sum of All Sequence Data Sizes + 0xFC |
| 11286–11300   | Global Sequencer Information |
| 11301–11775   | Unused (zeros) |
| 11776–43575   | 60 Programs (530 bytes each — mixed together) |
| 43576–44031   | Unused (zeros) |
| 44032–xxxxx   | Sequence Data — Offsets & Track Data for defined seqs |
| xxxxx–zzzzz   | Unused (zeros) |

**Note:** Only defined sequences have data in the Sequence Data section. Undefined sequences have no data. Each defined sequence occupies at least one full disk block; remainder is filled with zeros.

---

## Sequence Header Format (188 bytes per sequence)

| Byte(s) | Contents |
|---------|----------|
| 00      | Original Sequence Location (0–59). **0xFF = undefined/empty slot.** |
| 01      | Flags: `[ ?? | ?? | ?? | Fx | CkRc | CkOn | Loop | Song ]`<br>Song=1 if Song (& Effects=Seq); CkRc=Click during Record Only; CkOn=Click On; ??=reserved, currently set=1 |
| 02–12   | Sequence Name — 11 bytes. **If Song, Byte 02 = 0x24 ('$').** MSB of each byte set if corresponding track is NOT muted and contains data. |
| 13      | Unused Name Byte (00 or 20h). MSB = mute for track 12. |
| 14–15   | Sequence Length (Measures/Bars) |
| 16      | Upper Time Signature (1–99) |
| 17      | Lower Time Signature: 0=1, 1=2, 2=4, 3=8, 4=16, 5=32, 6=64 |
| 18–21   | Punch In Location (ticks) — Edit Start Point |
| 22–25   | Punch Out Location (ticks) — Edit Stop Point |
| 26      | Tempo |
| 27      | Number of Song Steps (0xFF if not a Song) |
| 28–38   | Track 1 Parameters (11 bytes) |
| 39–49   | Track 2 Parameters |
| 50–60   | Track 3 Parameters |
| 61–71   | Track 4 Parameters |
| 72–82   | Track 5 Parameters |
| 83–93   | Track 6 Parameters |
| 94–104  | Track 7 Parameters |
| 105–115 | Track 8 Parameters |
| 116–126 | Track 9 Parameters |
| 127–137 | Track 10 Parameters |
| 138–148 | Track 11 Parameters |
| 149–159 | Track 12 Parameters |
| 160     | Current Selected Track Number |
| 161–171 | Current Layered Track Numbers |
| 172–182 | Sequence Effect Parameters |
| 183–185 | Size of Sequence Data Section (3 bytes) |
| 186     | Major Rev. of OS used to write file |
| 187     | Minor Rev. of OS used to write file |

---

## Sequence Data Section (per defined sequence)

Starts immediately after the header block + padding. Each defined sequence contributes one contiguous chunk.

### Track Offsets (60 bytes = 15 × 4-byte entries)

| Entry       | Contents |
|-------------|----------|
| Size of Seq | Total bytes of sequence data including these offsets |
| Start Trk 0 | Offset from start of offsets to track 0 data (conductor — clock events only) |
| Start Trk 1 | Offset to track 1 data |
| Start Trk 2 | Offset to track 2 data |
| Start Trk 3 | Offset to track 3 data |
| Start Trk 4 | Offset to track 4 data |
| Start Trk 5 | Offset to track 5 data |
| Start Trk 6 | Offset to track 6 data |
| Start Trk 7 | Offset to track 7 data |
| Start Trk 8 | Offset to track 8 data |
| Start Trk 9 | Offset to track 9 data |
| Start Trk 10 | Offset to track 10 data |
| Start Trk 11 | Offset to track 11 data |
| Start Trk 12 | Offset to track 12 data |
| Multi-Trk   | **Not used in disk files** (only in internal keyboard memory) |
| Song Track  | Offset to song track data (if this is a Song) |

Offsets are from the start of the Track Offsets block. Track 0 is the conductor track (Clock events only).

### Track Data Format

```
[4-byte track length] [stream of events] ... [80 E9 end-of-track]
```

- 4-byte big-endian length = total bytes of track data including the length field
- Each track **must** start with a Clock event (E6h) to initialize the counter at 01.01.00
- Each track **must** end with End-of-Track event (80 E9h)

---

## Event Format

All events contain an even number of bytes (16-bit words). MSB of first word is **always 1** (marks start of event). MSB of all subsequent words is **always 0**.

### First Event Word

```
Bit:   15 14 13 12 11 10 09 08 | 07 06 05 04 03 02 01 00
Value:  1 d6 d5 d4 d3 d2 d1 d0 | t7 t6 t5 t4 t3 t2 t1 t0
```

- **t7–t0** (low byte): Event type (0–255)
- **d6–d0** (high 7 bits): Clock tick delay before executing this event (0–127 ticks). For longer delays use Wait Event (E6h).

---

## Event Types

### Key Events 0–87 (00h–57h)
88 piano keys A0–C8. Event type = key number (0=A0, 87=C8).

```
Word 1: 1 d6 d5 d4 d3 d2 d1 d0 | t7 t6 t5 t4 t3 t2 t1 t0
Word 2: 0 v4 v3 v2 v1 v0 n9 n8 | n7 n6 n5 n4 n3 n2 n1 n0
[Word 3: 0 cE cD cC cB cA c9 c8 | c7 c6 c5 c4 c3 c2 c1 c0]  ← only if duration > 1023 ticks
```

- **v4–v0**: Velocity (5-bit = 32 values; Ensoniq maps MIDI 0,4,8,12,... → 0–31)
- **n9–n0**: Note duration in ticks (0–1023). If 0 → add Word 3 with duration up to 32767.
- **cE–c0**: Extended duration (MSB cleared, so max 0x7FFF = 32767 ticks)

### Poly Key Pressure Events 88–175 (58h–AFh)
Aftertouch for same 88 keys. Key = event_type − 88.

```
Word 1: 1 d6 d5 d4 d3 d2 d1 d0 | t7 t6 t5 t4 t3 t2 t1 t0
Word 2: 0 xx xx xx xx xx xx xx | xx p6 p5 p4 p3 p2 p1 p0
```
- **p6–p0**: Pressure value (full MIDI range)

### Controller Events 176–199 (B0h–C7h)

```
Word 1: 1 d6 d5 d4 d3 d2 d1 d0 | t7 t6 t5 t4 t3 t2 t1 t0
Word 2: 0 xx xx xx xx xx xx xx | xx v6 v5 v4 v3 v2 v1 v0
```

| Event | Hex  | Controller         | Value |
|-------|------|--------------------|-------|
| 176   | B0h  | Pitch Wheel        | v6–v0 = pitch bend amount |
| 177   | B1h  | Modulation Wheel   | v6–v0 = amount |
| 178   | B2h  | Patch Select       | v6=Left, v5=Right |
| 179   | B3h  | External Controller| v6–v0 = amount |
| 180   | B4h  | Foot Pedal         | v6–v0 = amount |
| 181   | B5h  | Volume             | v6–v0 = amount |
| 182   | B6h  | Sustain Pedal      | 127=on, 0=off |
| 183   | B7h  | Sost Pedal         | 127=on, 0=off |
| 184   | B8h  | Timbre             | v6–v0 = amount |
| 185   | B9h  | Release            | v6–v0 = amount |
| 186   | BAh  | Channel Pressure   | v6–v0 = amount |
| 187   | BBh  | Mix Event          | ??? |
| 188   | BCh  | Mute Event         | ??? |

### Program Change Event 217 (D9h)

```
Word 1: 1 d6 d5 d4 d3 d2 d1 d0 | 1 1 0 1 1 0 0 1
Word 2: 0 xx xx xx xx xx xx xx | xx v6 v5 v4 v3 v2 v1 v0
```
- **v6–v0**: Program number (0–127)

### Mixdown Volume Event 218 (DAh)

```
Word 1: 1 d6 d5 d4 d3 d2 d1 d0 | 1 1 0 1 1 0 1 0
Word 2: 0 xx xx xx xx xx xx xx | xx v6 v5 v4 v3 v2 v1 v0
```

### Mixdown Pan Event 219 (DBh)

```
Word 1: 1 d6 d5 d4 d3 d2 d1 d0 | 1 1 0 1 1 0 1 1
Word 2: 0 xx xx xx xx xx xx xx | xx v6 v5 v4 v3 v2 v1 v0
```

### Wait (Clock) Event 230 (E6h)
For delays > 127 ticks.

```
Word 1: 1 d6 d5 d4 d3 d2 d1 d0 | 1 1 1 0 0 1 1 0
Word 2: 0 cE cD cC cB cA c9 c8 | c7 c6 c5 c4 c3 c2 c1 c0
```
- **cE–c0**: Delay up to 32,767 ticks (0x7FFF). d6–d0 in Word 1 typically 0.

### Song Step Event 231 (E7h)
Only in the Song Track of a Song. First word always `80 E7`.

```
Word 1: 1 0 0 0 0 0 0 0 | 1 1 1 0 0 1 1 1   (= 80E7h)
Word 2: 0 sE sD sC sB sA s9 s8 | s7 s6 s5 s4 s3 s2 s1 s0   (sequence number)
Word 3: 0 xx xx xx xx xx mB mA | m9 m8 m7 m6 m5 m4 m3 m2 m1 m0   (mute bits per track)
Word 4: 0 xx xx xx xx xx tB tA | t9 t8 t7 t6 t5 t4 t3 t2 t1 t0   (transpose bits per track)
Word 5: 0 c6 c5 c4 c3 c2 c1 c0 | xx l6 l5 l4 l3 l2 l1 l0   (transpose count + loop count)
```

### Overdub Event 232 (E8h)
Internal only — **should not appear in disk files**. `80 E8`.

### End of Track Event 233 (E9h)
Must be last event in every track. One word only: `80 E9`.

---

## Key Observations for AllSequences Write Implementation

1. **On-disk "SixtySequences" ≠ denybblized SysEx payload.** Writing SysEx bytes directly causes SD-1 system error 192.

2. **The SysEx AllSequences payload** (from `inspect-sysex`) contains:
   - Bytes 0–239: 60 × 4-byte pointer table (big-endian offsets into event data, 0 = empty)
   - Bytes 240+: raw sequence event data
   - After event data: per-sequence headers + global parameters

3. **The on-disk format** rearranges this as:
   - 11,280 bytes of headers (60 × 188)
   - 4 bytes global info
   - 15 bytes global sequencer info
   - 475 bytes padding
   - Then per-sequence: track offset table + track event data (defined seqs only)

4. **Verified against `disk_with_everything.img`:** COUNTRY-* starts `00 XX 24` = location 0, flags, Song marker ('$') at byte 2 of first sequence header.

5. **Reference disk file sizes:**
   - COUNTRY-* (SixtySequences): 58,983 bytes
   - SD1-PALETTE (SixtySequences): 77,057 bytes
   - CLASS-PNO-* (SixtySequences): 12,877 bytes
   - BASSICS-* (SixtySequences): 46,951 bytes
