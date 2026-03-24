#!/usr/bin/env python3
"""
hybrid_programs_test.py — Create a hybrid SixtySequences file for error-192 debugging.

Takes the programs section (bytes 11776–43575) from a known-good reference
SixtySequences file (e.g. COUNTRY-* from disk_with_everything.img) and splices
it into our written NC12NORTSEQ, producing a hybrid test image.

If the hybrid loads, the problem is our interleaved programs data.
If it still gives error 192, the problem is structural (layout/detection).

Usage:
    python3 tools/hybrid_programs_test.py <ref_disk> <test_disk> <output_disk>
                                          [ref_name] [test_name]

    ref_disk   — disk with known-good SixtySequences (e.g. disk_with_everything.img)
    test_disk  — disk written by sd1cli (e.g. /tmp/nc12test.img)
    output_disk — where to write the hybrid result
    ref_name   — file prefix to find in ref_disk (default: COUNTRY)
    test_name  — file prefix to find in test_disk (default: NC12NORTSEQ)

Example:
    python3 tools/hybrid_programs_test.py disk_with_everything.img /tmp/nc12test.img /tmp/hybrid_test.img
"""

import sys
import struct

BLOCK_SIZE           = 512
FAT_START_BLOCK      = 5
ENTRIES_PER_FAT_BLK  = 170
FIRST_DATA_BLOCK     = 23
TOTAL_BLOCKS         = 1600

SUBDIR_START_BLOCK   = 15
SUBDIR_ENTRY_SIZE    = 26
SUBDIR_CAPACITY      = 39
SUBDIR_BLOCKS_EACH   = 2

FAT_FREE             = 0x000000
FAT_EOF              = 0x000001

PROGRAMS_OFFSET      = 11776
PROGRAMS_END         = 43576  # exclusive (31800 bytes)


def fat_byte_offset(block: int) -> int:
    fat_blk = FAT_START_BLOCK + block // ENTRIES_PER_FAT_BLK
    return fat_blk * BLOCK_SIZE + (block % ENTRIES_PER_FAT_BLK) * 3


def fat_read(img: bytearray, block: int) -> int:
    off = fat_byte_offset(block)
    return struct.unpack(">I", b'\x00' + img[off:off + 3])[0]


def fat_chain(img: bytearray, start: int) -> list:
    chain, seen, current = [], set(), start
    while True:
        if current in seen:
            raise ValueError(f"FAT cycle at block {current}")
        seen.add(current)
        chain.append(current)
        val = fat_read(img, current)
        if val == FAT_EOF:
            break
        elif val == FAT_FREE:
            raise ValueError(f"FAT chain hit free block at {current}")
        else:
            current = val
    return chain


def dir_entry_offset(subdir_idx: int, slot: int) -> int:
    base_block = SUBDIR_START_BLOCK + subdir_idx * SUBDIR_BLOCKS_EACH
    return base_block * BLOCK_SIZE + slot * SUBDIR_ENTRY_SIZE


def dir_read_entry(img: bytearray, subdir_idx: int, slot: int) -> dict | None:
    off = dir_entry_offset(subdir_idx, slot)
    d = img[off:off + SUBDIR_ENTRY_SIZE]
    if d[1] == 0:
        return None
    try:
        name_str = bytes(d[2:13]).decode('ascii').rstrip()
    except Exception:
        name_str = repr(bytes(d[2:13]))
    return {
        'name_str':    name_str,
        'file_type':   d[1],
        'first_block': struct.unpack(">I", d[18:22])[0],
        'size_bytes':  struct.unpack(">I", b'\x00' + d[23:26])[0],
    }


def dir_find(img: bytearray, prefix: str) -> dict | None:
    for slot in range(SUBDIR_CAPACITY):
        e = dir_read_entry(img, 0, slot)
        if e and e['name_str'].startswith(prefix.rstrip('*')):
            return e
    return None


def extract_file(img: bytearray, entry: dict) -> bytes:
    chain = fat_chain(img, entry['first_block'])
    raw = bytearray()
    for blk in chain:
        raw.extend(img[blk * BLOCK_SIZE:(blk + 1) * BLOCK_SIZE])
    return bytes(raw[:entry['size_bytes']])


def write_file_data(img: bytearray, entry: dict, data: bytes) -> None:
    """Write modified file data back to disk blocks (must fit within existing allocation)."""
    chain = fat_chain(img, entry['first_block'])
    padded = data + b'\x00' * (len(chain) * BLOCK_SIZE - len(data))
    for i, blk in enumerate(chain):
        off = blk * BLOCK_SIZE
        img[off:off + BLOCK_SIZE] = padded[i * BLOCK_SIZE:(i + 1) * BLOCK_SIZE]


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)

    ref_path    = sys.argv[1]
    test_path   = sys.argv[2]
    output_path = sys.argv[3]
    ref_prefix  = sys.argv[4] if len(sys.argv) > 4 else "COUNTRY"
    test_prefix = sys.argv[5] if len(sys.argv) > 5 else "NC12NORTSEQ"

    with open(ref_path, 'rb') as f:
        ref_img = bytearray(f.read())
    with open(test_path, 'rb') as f:
        test_img = bytearray(f.read())

    # Find reference file
    ref_entry = dir_find(ref_img, ref_prefix)
    if ref_entry is None:
        print(f"ERROR: '{ref_prefix}' not found in {ref_path}")
        sys.exit(1)
    print(f"Reference: {ref_entry['name_str']!r}  first_block={ref_entry['first_block']}  size={ref_entry['size_bytes']}")

    # Find test file
    test_entry = dir_find(test_img, test_prefix)
    if test_entry is None:
        print(f"ERROR: '{test_prefix}' not found in {test_path}")
        sys.exit(1)
    print(f"Test file: {test_entry['name_str']!r}  first_block={test_entry['first_block']}  size={test_entry['size_bytes']}")

    # Extract full file bytes
    ref_data  = bytearray(extract_file(ref_img, ref_entry))
    test_data = bytearray(extract_file(test_img, test_entry))

    print(f"\nReference file size: {len(ref_data)} bytes")
    print(f"Test file size:      {len(test_data)} bytes")

    if len(ref_data) < PROGRAMS_END:
        print(f"ERROR: reference file too short ({len(ref_data)} < {PROGRAMS_END})")
        sys.exit(1)
    if len(test_data) < PROGRAMS_END:
        print(f"ERROR: test file too short ({len(test_data)} < {PROGRAMS_END})")
        sys.exit(1)

    # Extract programs sections for comparison
    ref_progs  = ref_data[PROGRAMS_OFFSET:PROGRAMS_END]
    test_progs = test_data[PROGRAMS_OFFSET:PROGRAMS_END]

    print(f"\nPrograms section (11776–43575, {PROGRAMS_END - PROGRAMS_OFFSET} bytes):")
    print(f"  Reference first 16 bytes: {ref_progs[:16].hex()}")
    print(f"  Test file first 16 bytes: {test_progs[:16].hex()}")
    print(f"  Identical: {ref_progs == test_progs}")

    # Check zero padding (43576–44031)
    ref_pad  = ref_data[PROGRAMS_END:44032]
    test_pad = test_data[PROGRAMS_END:44032]
    print(f"\nZero padding (43576–44031):")
    print(f"  Reference all-zero: {all(b == 0 for b in ref_pad)}")
    print(f"  Test file all-zero: {all(b == 0 for b in test_pad)}")

    # Check sequence data start
    print(f"\nSequence data at 44032 (first 16 bytes):")
    print(f"  Reference: {ref_data[44032:44048].hex()}")
    print(f"  Test file: {test_data[44032:44048].hex()}")

    # Splice reference programs into test file
    print(f"\nSplicing reference programs into test file...")
    test_data[PROGRAMS_OFFSET:PROGRAMS_END] = ref_progs

    # Write result to output disk (copy of test_img with modified file)
    import shutil
    shutil.copy2(test_path, output_path)
    with open(output_path, 'rb') as f:
        out_img = bytearray(f.read())

    write_file_data(out_img, test_entry, bytes(test_data))

    with open(output_path, 'wb') as f:
        f.write(out_img)

    print(f"\nHybrid written to: {output_path}")
    print(f"  Programs section: from {ref_entry['name_str']!r} (reference)")
    print(f"  Sequence data:    from {test_entry['name_str']!r} (our write)")
    print()
    print("If this loads → our programs data is wrong (interleaving issue)")
    print("If error 192  → structural layout problem (not the programs bytes)")


if __name__ == '__main__':
    main()
