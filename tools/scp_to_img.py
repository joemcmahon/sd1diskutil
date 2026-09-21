#!/usr/bin/env python3
"""
scp_to_img.py — Convert a Greaseweazle SCP flux image to a raw 819,200-byte
                 Ensoniq SD-1 disk image (.img).

Usage:
    python3 tools/scp_to_img.py <input.scp> <output.img> [--report]

    --report   Print per-sector recovery summary; do not write output file.

SCP format assumptions (verified against sd1_muzak3.scp):
  - Header at offset 0: "SCP" (3 bytes)
  - Version byte, disk type, num_revolutions, start_track, end_track
  - Flags, bit_cell_width (0=16-bit), heads (0=both), resolution (0=25ns)
  - 4-byte LE checksum
  - Track offset table starts at byte 16 (168 × 4-byte LE offsets)
  - Track data: "TRK\\0" (4 bytes), then per-revolution headers:
      idx_time (4 LE), flux_count (4 LE), data_offset_from_TRK (4 LE)
  - Flux data: big-endian 16-bit values, each = time in 25ns ticks between
    consecutive flux reversals

MFM decoding for Ensoniq SD-1 (DD, 250 kbps, 300 RPM):
  One MFM bit cell = 4µs = 160 ticks.
  Classify each flux interval by nearest multiple of 80 ticks:
    ~80 ticks  (1T, 2µs):  short  → emit bits "11"  (or sometimes "10" depending on polarity)
    ~160 ticks (2T, 4µs):  medium → emit bit  "10"
    ~240 ticks (3T, 6µs):  long   → emit bits "100" (zero between two flux events)
  Actually we use the standard approach: build a raw MFM bitstream where each
  flux transition is a '1' and each silent tick between transitions is a '0',
  then extract data bits at the appropriate cell boundaries.

  Simplified (and robust) approach used here:
    Quantize each flux interval to nearest T (80 ticks):
      round(interval / 80) → number of bit cells
    Emit that many '0' bits followed by a '1' bit (the flux event itself).
  This gives a clean MFM bit stream from which we can extract data bits.

  Data bits sit at ODD positions in the MFM bit stream (0-indexed): the clock
  bits are at even positions and are ignored.

Disk layout (Ensoniq SD-1):
  80 tracks × 2 sides × 10 sectors × 512 bytes = 819,200 bytes.
  Block number = track*20 + side*10 + sector  (0-based sectors).

  SCP stores tracks sequentially; for a 2-sided disk with heads=0 (both),
  the track table has 160 entries: entry 2*t = side 0 of track t,
  entry 2*t+1 = side 1 of track t.
"""

import struct
import sys
from pathlib import Path

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

TICKS_PER_CELL = 80          # 2µs / 25ns = 80 ticks per half-cell
SECTORS_PER_TRACK = 10
SECTOR_SIZE = 512
NUM_TRACKS = 80
NUM_SIDES = 2
IMG_SIZE = NUM_TRACKS * NUM_SIDES * SECTORS_PER_TRACK * SECTOR_SIZE  # 819,200

# A1* sync mark in MFM data-bit stream (every other bit, MSB first):
# Decoded byte 0xA1 = 10100001, but A1* has a missing clock bit that makes
# the raw MFM pattern unique.  We search for the decoded byte sequence
# [A1 A1 A1] after syncing on the special pattern.
#
# In our flux→bit stream, we look for the special 0x4489 MFM word.
# 0x4489 = 0100 0100 1000 1001  (16 raw MFM bits, clock+data interleaved)
# Data bits (odd positions 1,3,5,...,15): 1,0,1,0,0,0,0,1 = 0xA1 ✓
# The missing clock makes position 4 a '0' instead of '1', distinguishing
# it from ordinary 0xA1.
A1_MFM_WORD = 0x4489

# ---------------------------------------------------------------------------
# CRC-16/CCITT
# ---------------------------------------------------------------------------

def crc16(data: bytes) -> int:
    crc = 0xFFFF
    for b in data:
        x = (crc >> 8) ^ b
        x ^= x >> 4
        crc = ((crc << 8) ^ (x << 12) ^ (x << 5) ^ x) & 0xFFFF
    return crc

# ---------------------------------------------------------------------------
# Flux → MFM bit stream
# ---------------------------------------------------------------------------

def flux_to_mfm_bits(flux_values: list[int]) -> bytearray:
    """
    Convert a list of flux interval values (25ns ticks each) to a raw MFM
    bitstream (one bit per element: 0 or 1).

    Each flux interval represents the time until the next magnetic flux
    reversal.  We quantize to the nearest bit-cell boundary (80 ticks) and
    emit that many '0' bits followed by a '1' bit (the reversal).
    """
    bits = bytearray()
    accumulator = 0  # carry-over from previous interval if value > max sensible

    for val in flux_values:
        val += accumulator
        accumulator = 0

        # Overflow sentinel: Greaseweazle emits 0x0000 when the interval
        # exceeds 65535 ticks; the true interval spans multiple such words.
        if val == 0:
            # An actual zero means we should carry forward — treat as a very
            # long gap (index crossing).  Just emit the accumulated value as
            # a long gap and skip.
            bits.extend(b'\x00' * 8)
            continue

        # Quantize to nearest half-cell (80 ticks).
        cells = max(1, round(val / TICKS_PER_CELL))
        # Cap runout to avoid runaway on index-crossing gaps.
        cells = min(cells, 8)

        # Emit (cells-1) zero bits then one '1' bit.
        bits.extend(b'\x00' * (cells - 1))
        bits.append(1)

    return bits

# ---------------------------------------------------------------------------
# MFM bit stream → bytes (data bits only)
# ---------------------------------------------------------------------------

def mfm_bits_to_bytes(bits: bytearray) -> bytearray:
    """
    Extract data bytes from a raw MFM bit stream.
    In MFM, bit pairs are (clock, data); data bits are at odd positions.
    Returns one byte per 16 input bits (8 data bits).
    """
    out = bytearray()
    n = (len(bits) // 16) * 16
    for i in range(0, n, 16):
        byte = 0
        for j in range(8):
            byte = (byte << 1) | bits[i + 2*j + 1]
        out.append(byte)
    return out

# ---------------------------------------------------------------------------
# A1* sync detection in MFM bit stream
# ---------------------------------------------------------------------------

def find_a1_sync(bits: bytearray, start: int = 0) -> int | None:
    """
    Search for the A1* sync mark (0x4489 MFM word) starting at bit offset
    `start` (must be even / on a cell boundary).  Returns the bit offset of
    the first bit of the match, or None.

    We scan every 2 bits (one MFM cell) to find the 16-bit pattern 0x4489.
    """
    target = A1_MFM_WORD
    end = len(bits) - 16
    for i in range(start, end, 2):
        word = 0
        for k in range(16):
            word = (word << 1) | bits[i + k]
        if word == target:
            return i
    return None

# ---------------------------------------------------------------------------
# Sector decoding from MFM bit stream
# ---------------------------------------------------------------------------

def decode_sectors(bits: bytearray, track: int, side: int) -> dict[int, bytes]:
    """
    Scan `bits` for valid IDAM+DAM pairs and return a dict of
    {sector_number: 512-byte data}.  Verifies CRC on both IDAM and DAM.
    """
    found: dict[int, bytes] = {}
    pos = 0

    while pos < len(bits):
        sync = find_a1_sync(bits, pos)
        if sync is None:
            break

        # Skip consecutive A1* marks.
        after = sync
        while True:
            nxt = find_a1_sync(bits, after)
            if nxt == after:
                after += 16  # advance one MFM word (16 bits)
            else:
                break

        # Need at least 16 more bits for marker byte.
        if after + 16 > len(bits):
            break

        # Extract data bytes starting at `after` (one byte = 16 bits).
        def read_bytes(bit_off: int, count: int) -> bytes | None:
            end = bit_off + count * 16
            if end > len(bits):
                return None
            out = bytearray(count)
            for i in range(count):
                byte = 0
                for j in range(8):
                    byte = (byte << 1) | bits[bit_off + i*16 + 2*j + 1]
                out[i] = byte
            return bytes(out)

        marker_bytes = read_bytes(after, 1)
        if marker_bytes is None:
            break
        marker = marker_bytes[0]

        if marker == 0xFE:
            # IDAM: track(1) side(1) sector(1) size_code(1) crc(2) = 6 bytes
            fields = read_bytes(after + 16, 6)
            if fields is None:
                pos = after + 16
                continue
            trk_f, side_f, sec_f, size_f, crc_hi, crc_lo = fields
            stored_crc = (crc_hi << 8) | crc_lo
            calc_crc = crc16(bytes([0xA1, 0xA1, 0xA1, 0xFE, trk_f, side_f, sec_f, size_f]))
            if calc_crc != stored_crc:
                pos = after + 16
                continue
            if sec_f >= SECTORS_PER_TRACK:
                pos = after + 16
                continue

            # Find DAM: scan forward for next A1* sync.
            dam_search = after + 16 + 6*16
            dam_sync = find_a1_sync(bits, dam_search)
            if dam_sync is None:
                pos = after + 16
                continue

            dam_after = dam_sync
            while True:
                nxt = find_a1_sync(bits, dam_after)
                if nxt == dam_after:
                    dam_after += 16
                else:
                    break

            dam_marker_b = read_bytes(dam_after, 1)
            if dam_marker_b is None or dam_marker_b[0] != 0xFB:
                pos = after + 16
                continue

            # DAM data: 512 bytes + 2 CRC bytes.
            dam_data_off = dam_after + 16
            payload = read_bytes(dam_data_off, SECTOR_SIZE + 2)
            if payload is None:
                pos = after + 16
                continue

            sector_data = payload[:SECTOR_SIZE]
            dam_crc_stored = (payload[SECTOR_SIZE] << 8) | payload[SECTOR_SIZE + 1]
            calc_dam_crc = crc16(bytes([0xA1, 0xA1, 0xA1, 0xFB]) + sector_data)
            if calc_dam_crc != dam_crc_stored:
                pos = after + 16
                continue

            if sec_f not in found:
                found[sec_f] = sector_data
            pos = dam_data_off + (SECTOR_SIZE + 2) * 16

        else:
            pos = after + 16

    return found

# ---------------------------------------------------------------------------
# SCP reader
# ---------------------------------------------------------------------------

def parse_scp(path: Path) -> dict:
    """
    Parse an SCP file.  Returns a dict with:
      'num_revs': int
      'start_track': int
      'end_track': int
      'tracks': list of track dicts, each with 'revolutions': list of
                {'idx_ticks': int, 'flux': list[int]}
    """
    data = path.read_bytes()

    assert data[0:3] == b'SCP', "Not an SCP file"
    num_revs    = data[5]
    start_track = data[6]
    end_track   = data[7]

    tracks = []
    num_entries = end_track - start_track + 1

    for entry_idx in range(num_entries):
        tbl_off = 16 + entry_idx * 4
        trk_off = struct.unpack_from('<I', data, tbl_off)[0]

        if trk_off == 0:
            tracks.append({'revolutions': []})
            continue

        assert data[trk_off:trk_off+3] == b'TRK', \
            f"Missing TRK magic at 0x{trk_off:X}"

        revolutions = []
        for rev in range(num_revs):
            base     = trk_off + 4 + rev * 12
            idx_time = struct.unpack_from('<I', data, base)[0]
            cnt      = struct.unpack_from('<I', data, base + 4)[0]
            doff     = struct.unpack_from('<I', data, base + 8)[0]
            abs_off  = trk_off + doff
            flux     = [struct.unpack_from('>H', data, abs_off + i*2)[0]
                        for i in range(cnt)]
            revolutions.append({'idx_ticks': idx_time, 'flux': flux})

        tracks.append({'revolutions': revolutions})

    return {
        'num_revs': num_revs,
        'start_track': start_track,
        'end_track': end_track,
        'tracks': tracks,
    }

# ---------------------------------------------------------------------------
# Main recovery loop
# ---------------------------------------------------------------------------

def recover(scp_path: Path, img_path: Path | None, report: bool) -> None:
    print(f"Reading {scp_path} …")
    scp = parse_scp(scp_path)
    num_revs = scp['num_revs']
    print(f"  {scp['end_track'] - scp['start_track'] + 1} track entries, "
          f"{num_revs} revolutions each")

    img = bytearray(IMG_SIZE)
    recovered   = [[None] * SECTORS_PER_TRACK
                   for _ in range(NUM_TRACKS * NUM_SIDES)]
    sector_rev  = [[-1] * SECTORS_PER_TRACK
                   for _ in range(NUM_TRACKS * NUM_SIDES)]

    total_ok  = 0
    total_bad = 0

    # SCP track entries: entry 2*t = side 0 of track t, 2*t+1 = side 1
    for t in range(NUM_TRACKS):
        for s in range(NUM_SIDES):
            entry_idx = t * NUM_SIDES + s
            flat = t * NUM_SIDES + s

            trk_info = scp['tracks'][entry_idx] if entry_idx < len(scp['tracks']) else None
            if not trk_info or not trk_info['revolutions']:
                total_bad += SECTORS_PER_TRACK
                continue

            for rev_idx, rev in enumerate(trk_info['revolutions']):
                bits = flux_to_mfm_bits(rev['flux'])
                secs = decode_sectors(bits, t, s)
                for sec_num, sec_data in secs.items():
                    if recovered[flat][sec_num] is None:
                        recovered[flat][sec_num] = sec_data
                        sector_rev[flat][sec_num] = rev_idx

            for sec_num in range(SECTORS_PER_TRACK):
                block = t * 20 + s * 10 + sec_num
                if recovered[flat][sec_num] is not None:
                    img[block * SECTOR_SIZE:(block+1) * SECTOR_SIZE] = \
                        recovered[flat][sec_num]
                    total_ok += 1
                else:
                    total_bad += 1

    print(f"\nRecovery: {total_ok}/{NUM_TRACKS*NUM_SIDES*SECTORS_PER_TRACK} sectors "
          f"({100*total_ok/(NUM_TRACKS*NUM_SIDES*SECTORS_PER_TRACK):.1f}%)")

    if report or total_bad > 0:
        print("\nMissing sectors:")
        any_missing = False
        for t in range(NUM_TRACKS):
            for s in range(NUM_SIDES):
                flat = t * NUM_SIDES + s
                missing = [i for i in range(SECTORS_PER_TRACK)
                           if recovered[flat][i] is None]
                if missing:
                    any_missing = True
                    print(f"  T{t:02d} S{s}: missing={missing}")
        if not any_missing:
            print("  (none — full recovery!)")

    if report:
        print("\n(--report mode: not writing output file)")
        return

    img_path.write_bytes(img)
    print(f"\nWrote {img_path} ({len(img):,} bytes)")
    if total_bad > 0:
        print(f"WARNING: {total_bad} unrecovered sectors left as zero-filled.")

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == '__main__':
    args = sys.argv[1:]
    report_only = '--report' in args
    args = [a for a in args if a != '--report']

    if len(args) < 1 or (not report_only and len(args) < 2):
        print(__doc__)
        sys.exit(1)

    scp_path = Path(args[0])
    img_path = Path(args[1]) if len(args) >= 2 else None

    recover(scp_path, img_path, report_only)
