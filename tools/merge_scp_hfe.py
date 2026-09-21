#!/usr/bin/env python3
"""
merge_scp_hfe.py — Merge sectors from a Greaseweazle SCP and HFE flux image
                   into a single Ensoniq SD-1 raw disk image (.img).

Both files typically come from the same Greaseweazle read session.  The HFE
is a single-pass MFM capture; the SCP has 3 revolutions of raw flux data.
In practice the SCP recovers more sectors, but occasionally the HFE decodes a
sector the SCP decoder misses.  This tool takes valid (CRC-checked) sectors
from both and merges them, preferring SCP data but filling gaps from the HFE.

Usage:
    python3 tools/merge_scp_hfe.py <input.scp> <input.hfe> <output.img> [--report]

    --report   Print per-sector source summary; do not write output file.

SCP parsing: see tools/scp_to_img.py for full format documentation.

HFE v1 format (subset used here):
  - 512-byte header: "HXCPICFE" + metadata
  - 512-byte track table at block 1: 80 × (2-byte block offset LE, 2-byte length LE)
  - Track data: side-0 and side-1 interleaved in 256-byte chunks
  - Each side is a raw MFM bitstream (LSB-first bit storage, 2 bytes per MFM bit)
  - A1* sync: bytes [0x22, 0x91]
  - Data bits at odd positions within each 16-bit MFM word
"""

import struct
import sys
from pathlib import Path

# ---------------------------------------------------------------------------
# Shared constants
# ---------------------------------------------------------------------------

SECTORS_PER_TRACK = 10
SECTOR_SIZE       = 512
NUM_TRACKS        = 80
NUM_SIDES         = 2
IMG_SIZE          = NUM_TRACKS * NUM_SIDES * SECTORS_PER_TRACK * SECTOR_SIZE

TICKS_PER_CELL    = 80   # 25ns × 80 = 2µs (one MFM half-cell at 250 kbps)
A1_MFM_WORD       = 0x4489  # A1* sync marker word in raw MFM

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
# SCP: flux → MFM bits → sectors
# (see scp_to_img.py for detailed comments)
# ---------------------------------------------------------------------------

def scp_flux_to_bits(flux_values: list) -> bytearray:
    bits = bytearray()
    for val in flux_values:
        if val == 0:
            bits.extend(b'\x00' * 8)
            continue
        cells = max(1, min(8, round(val / TICKS_PER_CELL)))
        bits.extend(b'\x00' * (cells - 1))
        bits.append(1)
    return bits

def scp_find_a1(bits: bytearray, start: int = 0):
    end = len(bits) - 16
    for i in range(start, end, 2):
        word = 0
        for k in range(16):
            word = (word << 1) | bits[i + k]
        if word == A1_MFM_WORD:
            return i
    return None

def scp_read_bytes(bits: bytearray, bit_off: int, count: int):
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

def scp_decode_sectors(bits: bytearray, track: int, side: int) -> dict:
    found = {}
    pos   = 0
    while pos < len(bits):
        sync = scp_find_a1(bits, pos)
        if sync is None:
            break
        after = sync
        while True:
            nxt = scp_find_a1(bits, after)
            if nxt == after:
                after += 16
            else:
                break
        if after + 16 > len(bits):
            break
        marker_b = scp_read_bytes(bits, after, 1)
        if marker_b is None:
            break
        if marker_b[0] == 0xFE:
            fields = scp_read_bytes(bits, after + 16, 6)
            if fields is None:
                pos = after + 16; continue
            trk_f, side_f, sec_f, size_f, crc_hi, crc_lo = fields
            stored_crc = (crc_hi << 8) | crc_lo
            if crc16(bytes([0xA1,0xA1,0xA1,0xFE,trk_f,side_f,sec_f,size_f])) != stored_crc \
               or sec_f >= SECTORS_PER_TRACK:
                pos = after + 16; continue
            dam_sync = scp_find_a1(bits, after + 16 + 6*16)
            if dam_sync is None:
                pos = after + 16; continue
            dam_after = dam_sync
            while True:
                nxt = scp_find_a1(bits, dam_after)
                if nxt == dam_after: dam_after += 16
                else: break
            dam_m = scp_read_bytes(bits, dam_after, 1)
            if dam_m is None or dam_m[0] != 0xFB:
                pos = after + 16; continue
            payload = scp_read_bytes(bits, dam_after + 16, SECTOR_SIZE + 2)
            if payload is None:
                pos = after + 16; continue
            sec_data   = payload[:SECTOR_SIZE]
            dam_stored = (payload[SECTOR_SIZE] << 8) | payload[SECTOR_SIZE + 1]
            if crc16(bytes([0xA1,0xA1,0xA1,0xFB]) + sec_data) == dam_stored:
                if sec_f not in found:
                    found[sec_f] = sec_data
            pos = dam_after + 16 + (SECTOR_SIZE + 2) * 16
        else:
            pos = after + 16
    return found

def read_scp(path: Path) -> dict:
    """Parse SCP file; return per-(track,side) dict of {sector: bytes}."""
    data     = path.read_bytes()
    assert data[0:3] == b'SCP'
    num_revs = data[5]
    end_trk  = data[7]
    start_trk= data[6]
    entries  = end_trk - start_trk + 1

    result = {}
    for entry_idx in range(entries):
        tbl_off = 16 + entry_idx * 4
        trk_off = struct.unpack_from('<I', data, tbl_off)[0]
        if trk_off == 0:
            continue
        t = entry_idx // NUM_SIDES
        s = entry_idx  % NUM_SIDES
        if t >= NUM_TRACKS:
            continue

        all_found = {}
        for rev in range(num_revs):
            base  = trk_off + 4 + rev * 12
            cnt   = struct.unpack_from('<I', data, base + 4)[0]
            doff  = struct.unpack_from('<I', data, base + 8)[0]
            flux  = [struct.unpack_from('>H', data, trk_off + doff + i*2)[0]
                     for i in range(cnt)]
            bits  = scp_flux_to_bits(flux)
            secs  = scp_decode_sectors(bits, t, s)
            for sec_num, sec_data in secs.items():
                if sec_num not in all_found:
                    all_found[sec_num] = sec_data

        result[(t, s)] = all_found
    return result

# ---------------------------------------------------------------------------
# HFE: MFM bitstream → sectors
# HFE stores bits LSB-first in each byte; A1* = [0x22, 0x91].
# Data bits are at odd positions (1,3,5,...,15) of each 16-bit MFM word.
# ---------------------------------------------------------------------------

def hfe_to_bits_16(b0: int, b1: int) -> int:
    """Pack two HFE bytes into a 16-bit MFM word (time-order: bit 0 = oldest)."""
    word = 0
    for i in range(8):
        word |= ((b0 >> i) & 1) << i
        word |= ((b1 >> i) & 1) << (i + 8)
    return word

def hfe_decode_byte(b0: int, b1: int) -> int:
    """Extract 8 data bits from two HFE MFM bytes."""
    val = 0
    bits0 = [(b0 >> i) & 1 for i in range(8)]
    bits1 = [(b1 >> i) & 1 for i in range(8)]
    all16 = bits0 + bits1
    for i in range(8):
        val |= all16[2*i + 1] << (7 - i)
    return val

A1_BITS = [0,1,0,0,0,1,0,0, 1,0,0,0,1,0,0,1]  # 0x22, 0x91 LSB-first

def hfe_find_sync(stream: bytes, start: int):
    for i in range(start, len(stream) - 1):
        b0 = [(stream[i]   >> k) & 1 for k in range(8)]
        b1 = [(stream[i+1] >> k) & 1 for k in range(8)]
        if b0 + b1 == A1_BITS:
            return i
    return None

def hfe_extract_side(raw: bytes, side: int, track_len: int) -> bytes:
    chunk = 256
    out   = bytearray()
    pos   = side * chunk
    while pos < len(raw) and len(out) < track_len:
        take = min(chunk, len(raw) - pos, track_len - len(out))
        if take == 0: break
        out.extend(raw[pos:pos + take])
        pos += chunk * 2
    return bytes(out[:track_len])

def hfe_decode_sectors(stream: bytes, track: int, side: int) -> dict:
    found = {}
    pos   = 0
    while pos < len(stream):
        sync = hfe_find_sync(stream, pos)
        if sync is None: break
        after = sync
        while after + 1 < len(stream) and hfe_find_sync(stream, after) == after:
            after += 2
        if after + 2 > len(stream): break
        marker = hfe_decode_byte(stream[after], stream[after + 1])
        if marker == 0xFE:
            fs = after + 2
            if fs + 12 > len(stream):
                pos = after + 2; continue
            fields = [hfe_decode_byte(stream[fs + i*2], stream[fs + i*2 + 1])
                      for i in range(6)]
            trk_f, side_f, sec_f, size_f, ch, cl = fields
            stored_crc = (ch << 8) | cl
            if crc16(bytes([0xA1,0xA1,0xA1,0xFE,trk_f,side_f,sec_f,size_f])) != stored_crc \
               or sec_f >= SECTORS_PER_TRACK:
                pos = after + 2; continue
            ds = hfe_find_sync(stream, fs + 12)
            if ds is None:
                pos = after + 2; continue
            dam_after = ds
            while dam_after + 1 < len(stream) and hfe_find_sync(stream, dam_after) == dam_after:
                dam_after += 2
            if dam_after + 2 > len(stream):
                pos = after + 2; continue
            dm = hfe_decode_byte(stream[dam_after], stream[dam_after + 1])
            if dm != 0xFB:
                pos = after + 2; continue
            dd = dam_after + 2
            if dd + (SECTOR_SIZE + 2) * 2 > len(stream):
                pos = after + 2; continue
            sec_data = bytes(hfe_decode_byte(stream[dd + i*2], stream[dd + i*2 + 1])
                             for i in range(SECTOR_SIZE))
            co = dd + SECTOR_SIZE * 2
            dam_ch = hfe_decode_byte(stream[co],     stream[co + 1])
            dam_cl = hfe_decode_byte(stream[co + 2], stream[co + 3])
            dam_stored = (dam_ch << 8) | dam_cl
            if crc16(bytes([0xA1,0xA1,0xA1,0xFB]) + sec_data) == dam_stored:
                if sec_f not in found:
                    found[sec_f] = sec_data
            pos = co + 4
        else:
            pos = after + 2
    return found

def read_hfe(path: Path) -> dict:
    """Parse HFE v1 file; return per-(track,side) dict of {sector: bytes}."""
    data = path.read_bytes()
    assert data[0:8] == b'HXCPICFE', "Not an HFE file"
    assert data[8] == 0,             "Unsupported HFE revision"

    num_tracks      = data[9]
    tl_block        = struct.unpack_from('<H', data, 18)[0]
    tl_offset       = tl_block * 512

    result = {}
    for t in range(min(num_tracks, NUM_TRACKS)):
        off    = tl_offset + t * 4
        blk    = struct.unpack_from('<H', data, off)[0]
        length = struct.unpack_from('<H', data, off + 2)[0]
        raw    = data[blk * 512 : blk * 512 + length]
        for s in range(NUM_SIDES):
            side_stream = hfe_extract_side(raw, s, length // 2)
            secs = hfe_decode_sectors(side_stream, t, s)
            result[(t, s)] = secs

    return result

# ---------------------------------------------------------------------------
# Merge and write
# ---------------------------------------------------------------------------

def merge_and_recover(scp_path: Path, hfe_path: Path,
                      img_path: Path | None, report: bool) -> None:
    print(f"Reading SCP: {scp_path} …")
    scp_data = read_scp(scp_path)
    print(f"Reading HFE: {hfe_path} …")
    hfe_data = read_hfe(hfe_path)

    img = bytearray(IMG_SIZE)
    source  = {}   # (t,s,sec) → 'scp' | 'hfe'
    missing = []

    scp_only = hfe_only = both_ok = total_missing = 0

    for t in range(NUM_TRACKS):
        for s in range(NUM_SIDES):
            scp_secs = scp_data.get((t, s), {})
            hfe_secs = hfe_data.get((t, s), {})
            for sec in range(SECTORS_PER_TRACK):
                block = t * 20 + s * 10 + sec
                if sec in scp_secs:
                    img[block*SECTOR_SIZE:(block+1)*SECTOR_SIZE] = scp_secs[sec]
                    if sec in hfe_secs:
                        both_ok += 1
                        source[(t,s,sec)] = 'both'
                    else:
                        scp_only += 1
                        source[(t,s,sec)] = 'scp'
                elif sec in hfe_secs:
                    img[block*SECTOR_SIZE:(block+1)*SECTOR_SIZE] = hfe_secs[sec]
                    hfe_only += 1
                    source[(t,s,sec)] = 'hfe'
                else:
                    total_missing += 1
                    missing.append((t, s, sec))
                    source[(t,s,sec)] = 'none'

    total_ok = scp_only + hfe_only + both_ok
    print(f"\nResults:")
    print(f"  SCP-only sectors:    {scp_only}")
    print(f"  HFE-only sectors:    {hfe_only}  ← additional recovery from HFE")
    print(f"  Both sources agree:  {both_ok}")
    print(f"  Total recovered:     {total_ok}/{NUM_TRACKS*NUM_SIDES*SECTORS_PER_TRACK} "
          f"({100*total_ok/(NUM_TRACKS*NUM_SIDES*SECTORS_PER_TRACK):.1f}%)")
    print(f"  Still missing:       {total_missing}")

    if missing:
        print(f"\nMissing sectors ({len(missing)}):")
        # Group by track/side for readability
        from itertools import groupby
        for (t,s), grp in groupby(missing, key=lambda x: (x[0],x[1])):
            secs = [x[2] for x in grp]
            print(f"  T{t:02d} S{s}: {secs}")

    if hfe_only > 0:
        print(f"\nHFE-only sector locations:")
        for (t,s,sec), src in sorted(source.items()):
            if src == 'hfe':
                print(f"  T{t:02d} S{s} sec {sec}")

    if report:
        print("\n(--report mode: not writing output file)")
        return

    img_path.write_bytes(img)
    print(f"\nWrote {img_path} ({len(img):,} bytes)")
    if total_missing > 0:
        print(f"WARNING: {total_missing} unrecovered sectors are zero-filled.")

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == '__main__':
    args = sys.argv[1:]
    report_only = '--report' in args
    args = [a for a in args if a != '--report']

    if len(args) < 2 or (not report_only and len(args) < 3):
        print(__doc__)
        sys.exit(1)

    scp_path = Path(args[0])
    hfe_path = Path(args[1])
    img_path = Path(args[2]) if len(args) >= 3 else None

    merge_and_recover(scp_path, hfe_path, img_path, report_only)
