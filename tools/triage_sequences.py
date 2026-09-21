#!/usr/bin/env python3
"""
triage_sequences.py — Analyse damage to SixtySequences and OneSequence files
                      on a (possibly partial) Ensoniq SD-1 disk image and
                      report which sequences, if any, are fully intact.

Usage:
    python3 tools/triage_sequences.py <image.img>

SixtySequences on-disk layout (per types.rs):
    [0..11280)   60 × 188-byte sequence headers
    [11280..11301) 21-byte global section
    [11301..11776) zeros / padding
    [11776..)    event data pool: sequences stored in slot order, each
                 block-padded to 512 bytes.  Only defined (non-0xFF) slots
                 contribute.

OneSequence on-disk layout: raw event bytes, block-padded.
"""

import struct
import sys
from pathlib import Path

BLOCK_SIZE      = 512
SECTOR_SIZE     = 512
HEADER_SIZE     = 188        # each slot header on disk
HEADER_COUNT    = 60
HEADERS_TOTAL   = HEADER_SIZE * HEADER_COUNT   # 11280
GLOBAL_START    = HEADERS_TOTAL                # 11280
GLOBAL_SIZE     = 21
SEQ_DATA_START  = 11776

NUM_TRACKS   = 80
NUM_SIDES    = 2

# ---------------------------------------------------------------------------
# Disk-image helpers
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
    """Return True iff all bytes in [file_offset, file_offset+count) are
    in non-zero (recovered) disk blocks."""
    end = file_offset + count
    start_block = file_offset // BLOCK_SIZE
    end_block   = (end - 1) // BLOCK_SIZE
    for rel_blk in range(start_block, end_block + 1):
        disk_blk = first_disk_block + rel_blk
        if block_is_zero(img, disk_blk):
            return False
    return True

def read_file_bytes(img: bytes, first_disk_block: int,
                    file_offset: int, count: int) -> bytes | None:
    """Read `count` bytes at `file_offset` within the file.
    Returns None if any required disk block is zero/unrecovered."""
    if not file_bytes_available(img, first_disk_block, file_offset, count):
        return None
    off = first_disk_block * BLOCK_SIZE + file_offset
    return img[off:off + count]

# ---------------------------------------------------------------------------
# Directory parsing (matches directory.rs parse_entry layout)
# byte 0:     type_info
# byte 1:     file_type
# bytes 2-12: name (11 bytes)
# byte 13:    reserved
# bytes 14-15: size_blocks (BE u16)
# bytes 16-17: contiguous_blocks (BE u16)
# bytes 18-21: first_block (BE u32)
# byte 22:    file_number
# bytes 23-25: size_bytes (BE 24-bit)
# ---------------------------------------------------------------------------

def parse_dir_entry(img: bytes, entry_offset: int):
    d = img[entry_offset:entry_offset + 26]
    if len(d) < 26 or d[1] == 0:
        return None
    name        = d[2:13].decode('ascii', 'replace').rstrip()
    ftype       = d[1]
    size_blocks = struct.unpack_from('>H', d, 14)[0]
    contig      = struct.unpack_from('>H', d, 16)[0]
    first_block = struct.unpack_from('>I', d, 18)[0]
    size_bytes  = (d[23] << 16) | (d[24] << 8) | d[25]
    return dict(name=name, ftype=ftype, size_blocks=size_blocks,
                contig=contig, first_block=first_block, size_bytes=size_bytes)

def list_files(img: bytes):
    """Return list of directory entries from subdirectory 0 (blocks 15-16)."""
    files = []
    # Subdirectory 0 occupies blocks 15 and 16
    for blk in [15, 16]:
        base = blk * BLOCK_SIZE
        for i in range(512 // 26):
            e = parse_dir_entry(img, base + i * 26)
            if e:
                files.append(e)
    return files

# ---------------------------------------------------------------------------
# SixtySequences triage
# ---------------------------------------------------------------------------

SLOT_UNDEFINED_MARKER = 0xFF

def read_slot_header(img: bytes, first_disk_block: int, slot: int):
    """Read the 188-byte on-disk header for slot `slot`.
    Returns None if any required block is unrecovered."""
    off = slot * HEADER_SIZE
    return read_file_bytes(img, first_disk_block, off, HEADER_SIZE)

def slot_ds(header: bytes) -> int:
    """Extract declared size (ds) from a 188-byte slot header."""
    return (header[183] << 16) | (header[184] << 8) | header[185]

def is_slot_defined(header: bytes) -> bool:
    return header[0] != SLOT_UNDEFINED_MARKER

def triage_sixty_sequences(img: bytes, entry: dict):
    name        = entry['name']
    first_block = entry['first_block']
    num_blocks  = entry['contig']
    size_bytes  = entry['size_bytes']

    print(f"\n{'='*60}")
    print(f"SixtySequences: '{name}'")
    print(f"  first_block={first_block} num_blocks={num_blocks} size_bytes={size_bytes}")

    # Show which file-area blocks are zero
    zero_file_blocks = [rel for rel in range(num_blocks)
                        if block_is_zero(img, first_block + rel)]
    ok_file_blocks   = [rel for rel in range(num_blocks)
                        if rel not in set(zero_file_blocks)]
    print(f"  Readable file blocks: {len(ok_file_blocks)}/{num_blocks}")
    print(f"  Missing file blocks:  {zero_file_blocks}")

    # Walk headers in slot order, accumulating event pool offset
    event_pool_offset = SEQ_DATA_START  # byte offset within the file
    salvageable = []
    blocked_at  = None   # first slot where we lost offset tracking

    print(f"\n  Scanning {HEADER_COUNT} slot headers …")
    for slot in range(HEADER_COUNT):
        hdr = read_slot_header(img, first_block, slot)

        if hdr is None:
            if blocked_at is None:
                blocked_at = slot
            # Can't read this header: ds unknown → can't advance event pool cursor
            continue

        if not is_slot_defined(hdr):
            # Undefined slot contributes no event data
            continue

        ds = slot_ds(hdr)
        if ds == 0:
            continue

        # We know where this slot's event data lives — but only if we haven't
        # lost track of the pool cursor (i.e. no missing defined headers before this).
        if blocked_at is not None:
            # A prior defined header was unreadable; we don't know how much event
            # data it had, so the cursor is unknown.
            print(f"    Slot {slot:2d}: defined, ds={ds}, "
                  f"event offset UNKNOWN (cursor lost at slot {blocked_at})")
            continue

        # Check whether the event data is fully in recovered blocks.
        data_ok = file_bytes_available(img, first_block, event_pool_offset, ds)
        status  = "✓ INTACT" if data_ok else "✗ PARTIAL/MISSING"
        print(f"    Slot {slot:2d}: defined, ds={ds}, "
              f"event @ file[{event_pool_offset}..{event_pool_offset+ds}]  {status}")

        if data_ok:
            salvageable.append(dict(slot=slot, ds=ds, offset=event_pool_offset))

        # Advance pool cursor (block-padded)
        padded = ((ds + BLOCK_SIZE - 1) // BLOCK_SIZE) * BLOCK_SIZE
        event_pool_offset += padded

    print(f"\n  Salvageable sequences: {len(salvageable)}")
    for s in salvageable:
        print(f"    Slot {s['slot']:2d}: {s['ds']} bytes at file offset {s['offset']}")

    return salvageable

# ---------------------------------------------------------------------------
# OneSequence triage
# ---------------------------------------------------------------------------

def triage_one_sequence(img: bytes, entry: dict):
    name        = entry['name']
    first_block = entry['first_block']
    num_blocks  = entry['contig']
    size_bytes  = entry['size_bytes']

    print(f"\n{'='*60}")
    print(f"OneSequence: '{name}'")
    print(f"  first_block={first_block} num_blocks={num_blocks} size_bytes={size_bytes}")

    zero_file_blocks = [rel for rel in range(num_blocks)
                        if block_is_zero(img, first_block + rel)]
    ok_file_blocks   = [rel for rel in range(num_blocks)
                        if rel not in set(zero_file_blocks)]
    print(f"  Readable file blocks: {len(ok_file_blocks)}/{num_blocks}")
    print(f"  Missing file blocks (relative):  {zero_file_blocks}")

    if not zero_file_blocks:
        print("  ✓ Sequence is fully intact — extract with sd1cli.")
        return True

    # Show readable contiguous runs (may be useful as partial MIDI data)
    print(f"\n  Readable byte ranges:")
    runs = []
    in_run = False
    for rel in range(num_blocks):
        byte_start = rel * BLOCK_SIZE
        byte_end   = min(byte_start + BLOCK_SIZE, size_bytes) - 1
        if byte_end < byte_start:
            break
        available = not block_is_zero(img, first_block + rel)
        if available and not in_run:
            run_start = byte_start
            in_run = True
        elif not available and in_run:
            runs.append((run_start, byte_start - 1))
            in_run = False
    if in_run:
        runs.append((run_start, size_bytes - 1))

    for (a, b) in runs:
        print(f"    bytes [{a}..{b}]  ({b-a+1} bytes)")

    print(f"\n  File is INCOMPLETE — {len(zero_file_blocks)} block(s) missing "
          f"in the middle of the sequence data.")
    print(f"  Partial data cannot be loaded by the SD-1 hardware.")
    return False

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    img_path = Path(sys.argv[1])
    img = load_img(img_path)

    files = list_files(img)
    if not files:
        print("No files found in directory.")
        sys.exit(0)

    print(f"Found {len(files)} file(s):")
    for e in files:
        print(f"  '{e['name']}' type=0x{e['ftype']:02X} "
              f"blocks={e['contig']} bytes={e['size_bytes']} first={e['first_block']}")

    SIXTY_SEQ = 0x13
    ONE_SEQ   = 0x11

    for entry in files:
        ft = entry['ftype']
        if ft == SIXTY_SEQ:
            triage_sixty_sequences(img, entry)
        elif ft == ONE_SEQ:
            triage_one_sequence(img, entry)
        else:
            print(f"\n  '{entry['name']}': file type 0x{ft:02X} not analysed here.")

if __name__ == '__main__':
    main()
