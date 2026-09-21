#!/usr/bin/env python3
"""
extract_intact_sequences.py — Extract fully-intact sequences from a damaged
                               Ensoniq SD-1 SixtySequences on-disk file and
                               save each as a standalone OneSequence SysEx (.syx).

Usage:
    python3 tools/extract_intact_sequences.py <image.img> <file_name>
            [--channel N] [--outdir DIR]

    file_name   SD-1 filename as shown by 'sd1cli list', e.g. 'ALEX 96'
    --channel   MIDI channel (0-15, default 0)
    --outdir    Output directory (default: current directory)

Limitation: the SixtySequences event pool is a sequential blob — each slot's
data immediately follows the previous slot's (block-padded to 512 bytes).  If
any *defined* slot earlier in the file has an unreadable header (we can't know
its ds), we lose track of all subsequent slots' offsets.  Only slots up to
that point can be extracted.

SixtySequences on-disk layout (from types.rs):
    [0..11280)     60 × 188-byte slot headers
    [11280..11301) 21-byte global section
    [11301..11776) zeros / padding
    [11776..)      event data pool; defined slots in slot-number order,
                   each padded to the next 512-byte boundary.

OneSequence SysEx wire format:
    F0 0F 05 00 [channel] 09 [nybblized event bytes] F7
"""

import struct
import sys
from pathlib import Path

BLOCK_SIZE     = 512
HEADER_SIZE    = 188
HEADER_COUNT   = 60
SEQ_DATA_START = 11776

# SysEx constants
SYSEX_START        = 0xF0
ENSONIQ_MANF       = 0x0F
VFX_FAMILY         = 0x05
MODEL_DEFAULT      = 0x00
MSG_SINGLE_SEQ     = 0x09
SYSEX_END          = 0xF7

# Directory entry file types
FTYPE_SIXTY_SEQ = 0x13
FTYPE_ONE_SEQ   = 0x11

# ---------------------------------------------------------------------------
# Image helpers
# ---------------------------------------------------------------------------

def load_img(path: Path) -> bytes:
    data = path.read_bytes()
    assert len(data) == 819_200, f"Expected 819200 bytes, got {len(data)}"
    return data

def block_is_zero(img: bytes, disk_block: int) -> bool:
    off = disk_block * BLOCK_SIZE
    return img[off:off + BLOCK_SIZE] == b'\x00' * BLOCK_SIZE

def file_bytes_available(img: bytes, first_disk_block: int,
                         file_offset: int, count: int) -> bool:
    """True iff every disk block covering file[file_offset..file_offset+count) is non-zero."""
    start_blk = file_offset // BLOCK_SIZE
    end_blk   = (file_offset + count - 1) // BLOCK_SIZE
    return all(not block_is_zero(img, first_disk_block + b)
               for b in range(start_blk, end_blk + 1))

def read_file_slice(img: bytes, first_disk_block: int,
                    file_offset: int, count: int) -> bytes:
    abs_off = first_disk_block * BLOCK_SIZE + file_offset
    return img[abs_off:abs_off + count]

# ---------------------------------------------------------------------------
# Directory
# ---------------------------------------------------------------------------

def parse_dir_entry(img: bytes, entry_offset: int):
    d = img[entry_offset:entry_offset + 26]
    if len(d) < 26 or d[1] == 0:
        return None
    name        = d[2:13].decode('ascii', 'replace').rstrip()
    ftype       = d[1]
    contig      = struct.unpack_from('>H', d, 16)[0]
    first_block = struct.unpack_from('>I', d, 18)[0]
    size_bytes  = (d[23] << 16) | (d[24] << 8) | d[25]
    return dict(name=name, ftype=ftype, contig=contig,
                first_block=first_block, size_bytes=size_bytes)

def find_file(img: bytes, name: str):
    for blk in [15, 16]:
        base = blk * BLOCK_SIZE
        for i in range(BLOCK_SIZE // 26):
            e = parse_dir_entry(img, base + i * 26)
            if e and e['name'].strip() == name.strip():
                return e
    return None

# ---------------------------------------------------------------------------
# SysEx encoding
# ---------------------------------------------------------------------------

def nybblize(data: bytes) -> bytes:
    out = bytearray(len(data) * 2)
    for i, b in enumerate(data):
        out[2*i]     = (b >> 4) & 0x0F
        out[2*i + 1] =  b       & 0x0F
    return bytes(out)

def wrap_single_sequence(event_bytes: bytes, channel: int) -> bytes:
    payload = nybblize(event_bytes)
    return bytes([SYSEX_START, ENSONIQ_MANF, VFX_FAMILY,
                  MODEL_DEFAULT, channel & 0x0F, MSG_SINGLE_SEQ]
                 ) + payload + bytes([SYSEX_END])

# ---------------------------------------------------------------------------
# Main extraction logic
# ---------------------------------------------------------------------------

def extract(img_path: Path, file_name: str, channel: int, outdir: Path) -> None:
    img   = load_img(img_path)
    entry = find_file(img, file_name)
    if not entry:
        print(f"ERROR: '{file_name}' not found in directory.")
        sys.exit(1)

    if entry['ftype'] not in (FTYPE_SIXTY_SEQ,):
        print(f"ERROR: '{file_name}' is not a SixtySequences file "
              f"(type=0x{entry['ftype']:02X}).")
        sys.exit(1)

    first_block = entry['first_block']
    print(f"File '{file_name}': first_block={first_block} "
          f"contig={entry['contig']} bytes={entry['size_bytes']}")

    outdir.mkdir(parents=True, exist_ok=True)

    event_pool_off = SEQ_DATA_START
    cursor_lost_at = None

    stats = dict(extracted=0, no_header=0, data_incomplete=0, cursor_lost=0)

    for slot in range(HEADER_COUNT):
        hdr_off = slot * HEADER_SIZE
        hdr_ok  = file_bytes_available(img, first_block, hdr_off, HEADER_SIZE)

        if not hdr_ok:
            stats['no_header'] += 1
            if cursor_lost_at is None:
                # Only matters if this slot turns out to be defined — we'll
                # mark cursor lost now conservatively.
                cursor_lost_at = slot
            continue

        hdr = read_file_slice(img, first_block, hdr_off, HEADER_SIZE)

        if hdr[0] == 0xFF:
            # Undefined slot — contributes nothing to the event pool.
            continue

        ds = (hdr[183] << 16) | (hdr[184] << 8) | hdr[185]
        if ds == 0:
            continue

        # This slot is defined and has event data.
        if cursor_lost_at is not None:
            stats['cursor_lost'] += 1
            print(f"  Slot {slot:2d}: defined ds={ds}, "
                  f"offset UNKNOWN (cursor lost at slot {cursor_lost_at}) — skipped")
            continue

        data_ok = file_bytes_available(img, first_block, event_pool_off, ds)
        if not data_ok:
            stats['data_incomplete'] += 1
            print(f"  Slot {slot:2d}: defined ds={ds}, "
                  f"event @ file[{event_pool_off}..{event_pool_off+ds}] — data incomplete, skipped")
            padded = ((ds + BLOCK_SIZE - 1) // BLOCK_SIZE) * BLOCK_SIZE
            event_pool_off += padded
            continue

        # Fully intact: extract.
        event_bytes = read_file_slice(img, first_block, event_pool_off, ds)
        syx         = wrap_single_sequence(event_bytes, channel)
        safe_name   = file_name.strip().replace(' ', '_')
        out_file    = outdir / f"{safe_name}_slot{slot:02d}.syx"
        out_file.write_bytes(syx)
        stats['extracted'] += 1
        print(f"  Slot {slot:2d}: ds={ds} bytes → {out_file}")

        padded = ((ds + BLOCK_SIZE - 1) // BLOCK_SIZE) * BLOCK_SIZE
        event_pool_off += padded

    print(f"\nSummary: {stats['extracted']} extracted, "
          f"{stats['no_header']} unreadable header, "
          f"{stats['data_incomplete']} data incomplete, "
          f"{stats['cursor_lost']} offset unknown.")

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == '__main__':
    import argparse
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('image',     help='Disk image (.img)')
    ap.add_argument('file_name', help="SD-1 filename, e.g. 'ALEX 96'")
    ap.add_argument('--channel', type=int, default=0, help='MIDI channel 0-15 (default 0)')
    ap.add_argument('--outdir',  default='.', help='Output directory (default: .)')
    args = ap.parse_args()

    extract(Path(args.image), args.file_name, args.channel, Path(args.outdir))
