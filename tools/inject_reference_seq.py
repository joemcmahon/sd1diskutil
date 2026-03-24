#!/usr/bin/env python3
"""
inject_reference_seq.py — Copy a file from a known-good disk image into a fresh SD-1 disk.

Usage:
    python3 tools/inject_reference_seq.py <source_disk> <target_disk> [name_prefix]

Arguments:
    source_disk  — disk image to copy the file FROM (e.g. disk_with_everything.img)
    target_disk  — disk image to inject INTO (must already be initialized with sd1cli create)
    name_prefix  — name prefix to match in SubDir0 (default: COUNTRY, wildcards ok e.g. COUNTRY-*)

Example:
    cargo run -p sd1cli -- create /tmp/test-ref3.img
    python3 tools/inject_reference_seq.py disk_with_everything.img /tmp/test-ref3.img COUNTRY-*
    cargo run -p sd1cli -- list /tmp/test-ref3.img

Purpose:
    Bypass SysEx conversion to test raw disk structure. If the SD-1 emulator loads the
    injected file successfully, the disk layout (FAT, directory, block layout) is correct.

Disk layout constants (from crates/sd1disk/src/):
    FAT starts at block 5, 3-byte big-endian entries, 170 entries/block
    EOF marker = 0x000001   (NOT 0x01FFFF)
    Free block  = 0x000000
    SubDir0 starts at block 15, each SubDir is 2 blocks wide
    Directory entry = 26 bytes, 39 slots per SubDirectory
    First data block = 23, total blocks = 1600
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
SUBDIR_BLOCKS_EACH   = 2   # each SubDirectory occupies 2 consecutive blocks

FAT_FREE             = 0x000000
FAT_EOF              = 0x000001


# ── FAT helpers ──────────────────────────────────────────────────────────────

def fat_byte_offset(block: int) -> int:
    """Byte offset of the 3-byte FAT entry for 'block'."""
    fat_blk = FAT_START_BLOCK + block // ENTRIES_PER_FAT_BLK
    return fat_blk * BLOCK_SIZE + (block % ENTRIES_PER_FAT_BLK) * 3


def fat_read(img: bytearray, block: int) -> int:
    off = fat_byte_offset(block)
    return struct.unpack(">I", b'\x00' + img[off:off + 3])[0]


def fat_write(img: bytearray, block: int, value: int) -> None:
    off = fat_byte_offset(block)
    img[off:off + 3] = value.to_bytes(3, 'big')


def fat_chain(img: bytearray, start: int) -> list[int]:
    """Follow FAT chain from start; return block list. Raises on cycle or free mid-chain."""
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


def fat_set_chain(img: bytearray, blocks: list[int]) -> None:
    """Write FAT entries for a chain; last entry = EOF."""
    for i, blk in enumerate(blocks):
        fat_write(img, blk, blocks[i + 1] if i + 1 < len(blocks) else FAT_EOF)


def fat_alloc(img: bytearray, n: int) -> list[int]:
    """Find n free blocks (contiguous if possible). Does NOT write FAT."""
    free = [b for b in range(FIRST_DATA_BLOCK, TOTAL_BLOCKS) if fat_read(img, b) == FAT_FREE]
    if len(free) < n:
        raise ValueError(f"Disk full: need {n} blocks, only {len(free)} free")
    # Prefer a contiguous run
    run_start, run_len = 0, 1
    for i in range(1, len(free)):
        if free[i] == free[i - 1] + 1:
            run_len += 1
            if run_len >= n:
                return free[run_start:run_start + n]
        else:
            run_start, run_len = i, 1
    return free[:n]


# ── Directory helpers ─────────────────────────────────────────────────────────

def dir_entry_offset(subdir_idx: int, slot: int) -> int:
    base_block = SUBDIR_START_BLOCK + subdir_idx * SUBDIR_BLOCKS_EACH
    return base_block * BLOCK_SIZE + slot * SUBDIR_ENTRY_SIZE


def dir_read_entry(img: bytearray, subdir_idx: int, slot: int) -> dict | None:
    off = dir_entry_offset(subdir_idx, slot)
    d = img[off:off + SUBDIR_ENTRY_SIZE]
    if d[1] == 0:
        return None
    name_bytes = bytes(d[2:13])
    try:
        name_str = name_bytes.decode('ascii').rstrip()
    except Exception:
        name_str = repr(name_bytes)
    return {
        'type_info':         d[0],
        'file_type':         d[1],
        'name':              name_bytes,
        'name_str':          name_str,
        'size_blocks':       struct.unpack(">H", d[14:16])[0],
        'contiguous_blocks': struct.unpack(">H", d[16:18])[0],
        'first_block':       struct.unpack(">I", d[18:22])[0],
        'file_number':       d[22],
        'size_bytes':        struct.unpack(">I", b'\x00' + d[23:26])[0],
    }


def dir_write_entry(img: bytearray, subdir_idx: int, slot: int, entry: dict) -> None:
    off = dir_entry_offset(subdir_idx, slot)
    d = bytearray(SUBDIR_ENTRY_SIZE)
    d[0]    = entry['type_info']
    d[1]    = entry['file_type']
    d[2:13] = entry['name']
    d[13]   = 0
    d[14:16] = struct.pack(">H", entry['size_blocks'])
    d[16:18] = struct.pack(">H", entry['contiguous_blocks'])
    d[18:22] = struct.pack(">I", entry['first_block'])
    d[22]   = entry['file_number']
    sb = struct.pack(">I", entry['size_bytes'])
    d[23:26] = sb[1:]
    img[off:off + SUBDIR_ENTRY_SIZE] = d


def dir_find(img: bytearray, subdir_idx: int, prefix: str) -> tuple[int, dict] | None:
    for slot in range(SUBDIR_CAPACITY):
        e = dir_read_entry(img, subdir_idx, slot)
        if e and e['name_str'].startswith(prefix):
            return slot, e
    return None


def dir_first_free(img: bytearray, subdir_idx: int) -> int | None:
    for slot in range(SUBDIR_CAPACITY):
        if dir_read_entry(img, subdir_idx, slot) is None:
            return slot
    return None


def dir_list(img: bytearray, subdir_idx: int) -> None:
    for slot in range(SUBDIR_CAPACITY):
        e = dir_read_entry(img, subdir_idx, slot)
        if e:
            print(f"  slot {slot:2d}: {e['name_str']!r:16s}  type=0x{e['file_type']:02x}"
                  f"  first_blk={e['first_block']:5d}  size={e['size_bytes']:7d}")


# ── Main ──────────────────────────────────────────────────────────────────────

def main() -> None:
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)

    source_path  = sys.argv[1]
    target_path  = sys.argv[2]
    name_arg     = sys.argv[3] if len(sys.argv) > 3 else "COUNTRY"
    name_prefix  = name_arg.rstrip('*')   # "COUNTRY-*" → "COUNTRY-"

    print(f"Source: {source_path}")
    print(f"Target: {target_path}")
    print(f"Search prefix: {name_prefix!r}")

    with open(source_path, 'rb') as f:
        source = bytearray(f.read())
    print(f"Source: {len(source)} bytes ({len(source) // BLOCK_SIZE} blocks)")

    # ── Find file in source ──────────────────────────────────────────────────
    found = dir_find(source, 0, name_prefix)
    if found is None:
        print(f"\nERROR: no entry matching {name_prefix!r}* in source SubDir0")
        print("Available entries in source SubDir0:")
        dir_list(source, 0)
        sys.exit(1)

    src_slot, src_entry = found
    print(f"\nFound in source SubDir0 slot {src_slot}:")
    print(f"  name:        {src_entry['name_str']!r}")
    print(f"  file_type:   0x{src_entry['file_type']:02x}")
    print(f"  first_block: {src_entry['first_block']}")
    print(f"  size_bytes:  {src_entry['size_bytes']}")
    print(f"  size_blocks: {src_entry['size_blocks']}")

    # ── Extract file data from source ────────────────────────────────────────
    chain = fat_chain(source, src_entry['first_block'])
    print(f"\nSource FAT chain: {len(chain)} blocks, {chain[0]}–{chain[-1]}")
    raw = bytearray()
    for blk in chain:
        raw.extend(source[blk * BLOCK_SIZE:(blk + 1) * BLOCK_SIZE])
    file_data = bytes(raw[:src_entry['size_bytes']])
    print(f"Extracted {len(file_data)} bytes")

    # ── Load target ───────────────────────────────────────────────────────────
    with open(target_path, 'rb') as f:
        target = bytearray(f.read())
    print(f"\nTarget: {len(target)} bytes")

    # ── Allocate blocks on target ─────────────────────────────────────────────
    n_blocks = (len(file_data) + BLOCK_SIZE - 1) // BLOCK_SIZE
    blocks = fat_alloc(target, n_blocks)
    contiguous = (blocks[-1] - blocks[0] == n_blocks - 1)
    print(f"Allocated {n_blocks} blocks: {blocks[0]}–{blocks[-1]} "
          f"({'contiguous' if contiguous else 'scattered'})")

    # ── Write file data to target blocks ──────────────────────────────────────
    padded = file_data + b'\x00' * (n_blocks * BLOCK_SIZE - len(file_data))
    for i, blk in enumerate(blocks):
        off = blk * BLOCK_SIZE
        target[off:off + BLOCK_SIZE] = padded[i * BLOCK_SIZE:(i + 1) * BLOCK_SIZE]

    # ── Write FAT chain ───────────────────────────────────────────────────────
    fat_set_chain(target, blocks)
    print(f"FAT chain written (last entry = 0x{FAT_EOF:06x} = EOF)")

    # Verify block 2 OS marker is intact
    os_marker = bytes(target[2 * BLOCK_SIZE + 28:2 * BLOCK_SIZE + 32])
    if os_marker[:2] == b'OS':
        print(f"OS marker intact: {os_marker.hex()}")
    else:
        print(f"WARNING: OS marker looks wrong: {os_marker.hex()} (expected 4f 53 ...)")

    # ── Write directory entry ─────────────────────────────────────────────────
    free_slot = dir_first_free(target, 0)
    if free_slot is None:
        print("ERROR: SubDir0 is full on target disk")
        sys.exit(1)

    new_entry = dict(src_entry)
    new_entry['first_block']       = blocks[0]
    new_entry['size_blocks']       = n_blocks
    new_entry['contiguous_blocks'] = n_blocks
    # file_number, size_bytes, file_type, name, type_info kept from source

    dir_write_entry(target, 0, free_slot, new_entry)
    print(f"\nDirectory entry written to SubDir0 slot {free_slot}")

    # ── Save target ───────────────────────────────────────────────────────────
    with open(target_path, 'wb') as f:
        f.write(target)
    print(f"\n✓ Saved {target_path}")
    print(f"\nVerify with:")
    print(f"  cargo run -p sd1cli -- list {target_path}")


if __name__ == '__main__':
    main()
