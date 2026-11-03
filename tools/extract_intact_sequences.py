#!/usr/bin/env python3
"""
extract_intact_sequences.py — Extract intact sequences from damaged
                               SixtySequences or ThirtySequences files.

Usage:
    python3 tools/extract_intact_sequences.py <image.img> <file_name> [--channel N] [--outdir DIR]

Examples:
    python3 tools/extract_intact_sequences.py damaged.img "ALEX 96"
    python3 tools/extract_intact_sequences.py damaged.img "ALEX 96" --channel 2 --outdir recovered

Extracts one or more intact sequences from a damaged Ensoniq SD-1
SixtySequences or ThirtySequences on-disk file and saves each as a
standalone OneSequence SysEx (.syx) file.

Each extracted sequence is wrapped as an Ensoniq SysEx message:
    F0 0F 05 00 [channel] 09 [nybblized event bytes] F7

Where:
    - F0 = SysEx start
    - 0F 05 = Ensoniq manufacturer ID
    - 00 = SD-1 model
    - channel = MIDI channel (0-15)
    - 09 = SingleSequence message type
    - nybblized event bytes = each byte split into two nibbles
    - F7 = SysEx end

SixtySequences on-disk layout:
    [0..11280)      60 × 188-byte slot headers
    [11280..11301)  21-byte global section
    [11301..11776)  zeros/padding
    [11776..)       event data pool: defined slots in slot order,
                    each padded to 512-byte multiple
    With embedded programs (directory type_info bit 0x20), 60 programs
    occupy [11776..43576) and the event pool starts at 44032 instead.

ThirtySequences on-disk layout:
    [0..5640)       30 × 188-byte slot headers
    [5640..5661)    21-byte global section
    [5661..6144)    zeros/padding
    [6144..)        event data pool: defined slots in slot order,
                    each padded to 512-byte multiple
    Embedded programs, if any, come AFTER the event pool, so the pool
    offset never moves.

File blocks: the first `contiguous_blocks` blocks run from first_block;
the rest are found by following the FAT chain (block 5 onward, 3-byte BE
entries). If the chain can't be followed (unrecovered FAT block, corrupt
entry), the remaining blocks are treated as unrecoverable, never guessed.

A slot is defined when header byte 0 != 0xFF.
Declared size (ds) = (header[183] << 16) | (header[184] << 8) | header[185]

LIMITATION: If a defined slot early in the file has an unreadable header,
all subsequent slots' event data offsets become unknown and cannot be
extracted, even if those headers are themselves readable. This is because
we need the ds value to advance the event pool cursor.

Directory entry layout (26 bytes):
    byte 0:       type_info
    byte 1:       file_type
    bytes 2-12:   name (11 bytes)
    byte 13:      reserved
    bytes 14-15:  size_blocks (BE u16)
    bytes 16-17:  contiguous_blocks (BE u16)
    bytes 18-21:  first_block (BE u32)
    byte 22:      file_number
    bytes 23-25:  size_bytes (BE 24-bit)

Directories: four subdirectories, subdirectory n at disk block 15 + 2n,
each 39 contiguous 26-byte entries; if subdirectory 0 is empty, the VST3
plugin's flat directory at block 1 offset 0x1E is used instead.
"""

import argparse
import struct
import sys
from pathlib import Path

BLOCK_SIZE      = 512
HEADER_SIZE     = 188
HEADER_COUNT_60 = 60
HEADER_COUNT_30 = 30
GLOBAL_SIZE     = 21
SEQ_DATA_START_60 = 11776
SEQ_DATA_START_30 = 6144
SEQ_DATA_START_60_WITH_PROGRAMS = 44032
PROGRAMS_EMBEDDED_FLAG = 0x20     # directory type_info bit

SLOT_UNDEFINED_MARKER = 0xFF

# File type constants (from directory.rs)
FTYPE_THIRTY_SEQUENCES = 0x12
FTYPE_SIXTY_SEQUENCES  = 0x13

# SysEx constants
SYSEX_START      = 0xF0
SYSEX_END        = 0xF7
ENSONIQ_MFR_ID_1 = 0x0F
ENSONIQ_MFR_ID_2 = 0x05
SD1_MODEL        = 0x00
MSG_SINGLE_SEQ   = 0x09

# ---------------------------------------------------------------------------
# Disk-image helpers
# ---------------------------------------------------------------------------

def load_img(path: Path) -> bytes:
    data = path.read_bytes()
    assert len(data) == 819_200, f"Expected 819200 bytes, got {len(data)}"
    return data

def block_is_zero(img: bytes, disk_block: int) -> bool:
    """Return True if the given disk block is all zeros (unrecovered)."""
    off = disk_block * BLOCK_SIZE
    return img[off:off + BLOCK_SIZE] == b'\x00' * BLOCK_SIZE

FAT_START_BLOCK       = 5     # fat.rs: 3-byte BE entries, 170 per block
FAT_ENTRIES_PER_BLOCK = 170
FAT_FIRST_DATA_BLOCK  = 23
FAT_FREE, FAT_EOF, FAT_BAD = 0, 1, 2

def _fat_next(img: bytes, block: int):
    """Next block in the chain, FAT_EOF, or None if the FAT entry can't be
    trusted (its FAT block is unrecovered, or the entry is free/bad/invalid)."""
    fat_block = FAT_START_BLOCK + block // FAT_ENTRIES_PER_BLOCK
    if block_is_zero(img, fat_block):
        return None
    off = fat_block * BLOCK_SIZE + (block % FAT_ENTRIES_PER_BLOCK) * 3
    v = (img[off] << 16) | (img[off + 1] << 8) | img[off + 2]
    if v == FAT_EOF:
        return FAT_EOF
    if v in (FAT_FREE, FAT_BAD) or v < FAT_FIRST_DATA_BLOCK or v >= len(img) // BLOCK_SIZE:
        return None
    return v

def file_block_map(img: bytes, entry: dict) -> list:
    """Disk block for each block of the file (None where unknown).

    The first `contig` blocks are contiguous by definition (directory entry);
    the rest follow the FAT chain, as sd1disk's FileAllocationTable::chain
    does. If the chain can't be followed (unrecovered FAT block, corrupt
    entry, early EOF, loop), the remaining blocks are unknown rather than
    guessed, so their bytes count as unavailable."""
    n = entry['size_blocks']
    contig = min(entry['contig'], n) or 1
    blocks = [entry['first_block'] + i for i in range(contig)]
    seen = set(blocks)
    cur = blocks[-1]
    while len(blocks) < n:
        nxt = _fat_next(img, cur)
        if nxt is None or nxt == FAT_EOF or nxt in seen:
            break
        blocks.append(nxt); seen.add(nxt); cur = nxt
    return blocks + [None] * (n - len(blocks))

def file_bytes_available(img: bytes, blockmap: list,
                         file_offset: int, count: int) -> bool:
    """Return True iff all bytes in [file_offset, file_offset+count) lie in
    known, non-zero (recovered) disk blocks of the file."""
    end = file_offset + count
    start_block = file_offset // BLOCK_SIZE
    end_block   = (end - 1) // BLOCK_SIZE
    for rel_blk in range(start_block, end_block + 1):
        if rel_blk >= len(blockmap) or blockmap[rel_blk] is None:
            return False
        if block_is_zero(img, blockmap[rel_blk]):
            return False
    return True

def read_file_bytes(img: bytes, blockmap: list,
                    file_offset: int, count: int) -> bytes | None:
    """Read `count` bytes at `file_offset` within the file, following the
    block map. Returns None if any required block is unknown or unrecovered."""
    if not file_bytes_available(img, blockmap, file_offset, count):
        return None
    out = bytearray()
    pos, remaining = file_offset, count
    while remaining:
        blk, within = divmod(pos, BLOCK_SIZE)
        take = min(remaining, BLOCK_SIZE - within)
        off = blockmap[blk] * BLOCK_SIZE + within
        out += img[off:off + take]
        pos += take; remaining -= take
    return bytes(out)

# ---------------------------------------------------------------------------
# Directory parsing
# ---------------------------------------------------------------------------

def parse_dir_entry(img: bytes, entry_offset: int):
    d = img[entry_offset:entry_offset + 26]
    # Valid SD-1 file types are 0x0A..0x16 (FileType::from_byte in directory.rs);
    # anything else is an empty slot or non-directory data.
    if len(d) < 26 or not (0x0A <= d[1] <= 0x16):
        return None
    name        = d[2:13].decode('ascii', 'replace').rstrip()
    ftype       = d[1]
    size_blocks = struct.unpack_from('>H', d, 14)[0]
    contig      = struct.unpack_from('>H', d, 16)[0]
    first_block = struct.unpack_from('>I', d, 18)[0]
    size_bytes  = (d[23] << 16) | (d[24] << 8) | d[25]
    return dict(name=name, ftype=ftype, type_info=d[0], size_blocks=size_blocks,
                contig=contig, first_block=first_block, size_bytes=size_bytes)

SUBDIR_START_BLOCK = 15     # SubDir n starts at block 15 + 2n (directory.rs)
SUBDIR_COUNT       = 4
SUBDIR_CAPACITY    = 39     # 39 contiguous 26-byte entries, spanning the 2 blocks
DIR_ENTRY_SIZE     = 26
BLOCK1_DIR_OFFSET  = BLOCK_SIZE + 0x1E   # VST3 plugin's flat directory

def _subdir_entries(img: bytes, base: int):
    files = []
    for slot in range(SUBDIR_CAPACITY):
        e = parse_dir_entry(img, base + slot * DIR_ENTRY_SIZE)
        if e:
            files.append(e)
    return files

def list_files(img: bytes):
    """Return all directory entries, mirroring sd1cli's all_entries():
    all four subdirectories (hardware format); if subdirectory 0 is empty,
    fall back to the VST3 plugin's flat directory in block 1."""
    if not _subdir_entries(img, SUBDIR_START_BLOCK * BLOCK_SIZE):
        b1 = _subdir_entries(img, BLOCK1_DIR_OFFSET)
        if b1:
            return b1
    files = []
    for n in range(SUBDIR_COUNT):
        files += _subdir_entries(img, (SUBDIR_START_BLOCK + 2 * n) * BLOCK_SIZE)
    return files

def find_file_by_name(img: bytes, name: str):
    """Find directory entry matching the given name (case-insensitive)."""
    files = list_files(img)
    name_upper = name.upper()
    for f in files:
        if f['name'].upper() == name_upper:
            return f
    return None

# ---------------------------------------------------------------------------
# SysEx formatting
# ---------------------------------------------------------------------------

def nybblize(data: bytes) -> bytes:
    """Convert each byte to two nibbles (high, low) per Ensoniq SysEx format."""
    result = bytearray(len(data) * 2)
    for i, b in enumerate(data):
        result[i * 2]     = (b >> 4) & 0x0F
        result[i * 2 + 1] = b & 0x0F
    return bytes(result)

def make_single_sequence_sysex(event_data: bytes, channel: int) -> bytes:
    """Wrap event data as an Ensoniq SD-1 SingleSequence SysEx message."""
    nybblized = nybblize(event_data)
    msg = bytearray([
        SYSEX_START,
        ENSONIQ_MFR_ID_1,
        ENSONIQ_MFR_ID_2,
        SD1_MODEL,
        channel & 0x0F,
        MSG_SINGLE_SEQ,
    ])
    msg.extend(nybblized)
    msg.append(SYSEX_END)
    return bytes(msg)

# ---------------------------------------------------------------------------
# Slot header parsing
# ---------------------------------------------------------------------------

def read_slot_header(img: bytes, blockmap: list, slot: int) -> bytes | None:
    """Read the 188-byte on-disk header for slot `slot`.
    Returns None if any required block is unknown or unrecovered."""
    off = slot * HEADER_SIZE
    return read_file_bytes(img, blockmap, off, HEADER_SIZE)

def slot_ds(header: bytes) -> int:
    """Extract declared size (ds) from a 188-byte slot header."""
    return (header[183] << 16) | (header[184] << 8) | header[185]

def is_slot_defined(header: bytes) -> bool:
    return header[0] != SLOT_UNDEFINED_MARKER

# ---------------------------------------------------------------------------
# Sequence extraction
# ---------------------------------------------------------------------------

def extract_intact_sequences(img: bytes, entry: dict, channel: int, outdir: Path):
    """Extract all intact sequences from a SixtySequences or ThirtySequences file."""
    name        = entry['name']
    ftype       = entry['ftype']
    first_block = entry['first_block']
    num_blocks  = entry['contig']
    size_bytes  = entry['size_bytes']

    if ftype == FTYPE_SIXTY_SEQUENCES:
        header_count = HEADER_COUNT_60
        # type_info bit 0x20: 60 programs are embedded BEFORE the sequence
        # data, which then starts at 44032 (types.rs disk_to_allsequences).
        # (ThirtySequences store embedded programs AFTER the data, so their
        # sequence offset never moves.)
        if entry['type_info'] & PROGRAMS_EMBEDDED_FLAG:
            seq_data_start = SEQ_DATA_START_60_WITH_PROGRAMS
        else:
            seq_data_start = SEQ_DATA_START_60
        type_name = "SixtySequences"
    elif ftype == FTYPE_THIRTY_SEQUENCES:
        header_count = HEADER_COUNT_30
        seq_data_start = SEQ_DATA_START_30
        type_name = "ThirtySequences"
    else:
        print(f"Error: '{name}' is not a SixtySequences or ThirtySequences file (type=0x{ftype:02X})")
        return

    blockmap = file_block_map(img, entry)
    print(f"Processing {type_name}: '{name}'"
          + (" (programs embedded)" if seq_data_start == SEQ_DATA_START_60_WITH_PROGRAMS else ""))
    if entry['contig'] < entry['size_blocks']:
        known = sum(b is not None for b in blockmap)
        print(f"  fragmented: {entry['contig']} of {entry['size_blocks']} blocks contiguous; "
              f"{known} of {entry['size_blocks']} located via the FAT chain")
    print(f"  first_block={first_block}, num_blocks={num_blocks}, size_bytes={size_bytes}")
    print(f"  channel={channel}, outdir={outdir}")
    print()

    # Track statistics
    intact_count = 0
    header_unreadable_count = 0
    data_incomplete_count = 0
    cursor_lost_count = 0
    
    event_pool_offset = seq_data_start
    cursor_lost_at = None

    for slot in range(header_count):
        hdr = read_slot_header(img, blockmap, slot)

        if hdr is None:
            # Header block is unreadable
            header_unreadable_count += 1
            if cursor_lost_at is None:
                cursor_lost_at = slot
            continue

        if not is_slot_defined(hdr):
            # Undefined slot contributes no event data
            continue

        ds = slot_ds(hdr)
        if ds == 0:
            continue

        # Check if cursor is still valid
        if cursor_lost_at is not None:
            # We lost track of the event pool offset at an earlier slot
            cursor_lost_count += 1
            continue

        # Check if event data is fully available
        data_ok = file_bytes_available(img, blockmap, event_pool_offset, ds)

        if data_ok:
            # Extract the event data
            event_data = read_file_bytes(img, blockmap, event_pool_offset, ds)
            if event_data is None:
                # Shouldn't happen since we just checked, but be defensive
                data_incomplete_count += 1
            else:
                # Wrap as SysEx and write
                sysex = make_single_sequence_sysex(event_data, channel)
                
                # Sanitize filename: replace spaces with underscores, remove special chars
                safe_name = name.replace(' ', '_').replace('/', '_').replace('\\', '_')
                output_file = outdir / f"{safe_name}_slot{slot:02d}.syx"
                output_file.write_bytes(sysex)
                
                print(f"Extracted slot {slot:2d} (ds={ds:5d} bytes) → {output_file.name}")
                intact_count += 1
        else:
            # Header readable but event data incomplete
            data_incomplete_count += 1

        # Advance pool cursor (block-padded)
        padded = ((ds + BLOCK_SIZE - 1) // BLOCK_SIZE) * BLOCK_SIZE
        event_pool_offset += padded

    # Print summary
    print()
    print("=" * 60)
    print(f"Extraction complete for '{name}':")
    print(f"  {intact_count} intact sequences extracted")
    print(f"  {header_unreadable_count} skipped (header unreadable)")
    print(f"  {data_incomplete_count} skipped (data incomplete)")
    if cursor_lost_at is not None:
        print(f"  {cursor_lost_count} unknown (cursor lost at slot {cursor_lost_at})")
    print("=" * 60)

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description='Extract intact sequences from damaged SixtySequences or ThirtySequences files',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__
    )
    parser.add_argument('image', type=Path, help='Disk image file (.img)')
    parser.add_argument('file_name', help='SD-1 filename to extract from (e.g. "ALEX 96")')
    parser.add_argument('--channel', type=int, default=0, metavar='N',
                        help='MIDI channel for SysEx output (0-15, default: 0)')
    parser.add_argument('--outdir', type=Path, default=Path('.'), metavar='DIR',
                        help='Output directory for .syx files (default: current directory)')
    
    args = parser.parse_args()

    # Validate arguments
    if not args.image.exists():
        print(f"Error: Image file '{args.image}' not found")
        sys.exit(1)

    if not (0 <= args.channel <= 15):
        print(f"Error: Channel must be 0-15, got {args.channel}")
        sys.exit(1)

    # Create output directory if needed
    args.outdir.mkdir(parents=True, exist_ok=True)

    # Load image
    img = load_img(args.image)

    # Find the file
    entry = find_file_by_name(img, args.file_name)
    if entry is None:
        print(f"Error: File '{args.file_name}' not found in disk image")
        print("\nAvailable files:")
        for f in list_files(img):
            print(f"  '{f['name']}' (type=0x{f['ftype']:02X})")
        sys.exit(1)

    # Extract sequences
    extract_intact_sequences(img, entry, args.channel, args.outdir)

if __name__ == '__main__':
    main()
