#!/usr/bin/env python3
"""
Compare AllSequences SysEx payload vs on-disk sequence data.

Usage:
    python3 tools/compare_allseq_sysex_vs_disk.py <sysex_file> <disk_image>

Example:
    python3 tools/compare_allseq_sysex_vs_disk.py \\
        "/Volumes/Aux Brain/Ready for SSD/Music/Music related/Music/SysEx Librarian/sequences/seq-countryseq.syx" \\
        disk_with_everything.img

Purpose:
    The SysEx AllSequences payload (packet 1, type 0x0A) is structured as:
        Bytes 0–239:      "Pointer table" (60 × 4-byte values)
        Bytes 240–24583:  Sequence event data
        Bytes 24584–35864: 60 × 188-byte sequence headers
        Bytes 35864–35885: 21-byte global section

    The on-disk SixtySequences (No Programs) layout is:
        00000–11279:  60 × 188-byte sequence headers
        11280–11300:  Global section (21 bytes)
        11301–11775:  Zeros (475 bytes)
        11776+:       Sequence data

    COUNTRY-* is at disk block 1360, file size 58983 bytes.
    On-disk sequence data starts at byte 44032 within the file.

    This script determines whether SysEx[240:] == disk[44032:] directly,
    and if not, searches for the exact offset alignment.
"""

import sys
import struct

ENSONIQ_MFR = 0x0F
VFX_FAMILY  = 0x05
MSG_ALLSEQ  = 0x0A

DISK_BLOCK       = 1360
DISK_FILE_SIZE   = 58983
DISK_SEQ_OFFSET  = 44032   # within file: where sequence event data starts

SYSEX_PTR_TABLE_SIZE = 240  # 60 × 4 bytes
SYSEX_HEADER_COUNT   = 60
SYSEX_HEADER_SIZE    = 188
SYSEX_GLOBAL_SIZE    = 21


def denybblize(data: bytes) -> bytes:
    """Convert nybble-pairs back to bytes: (hi << 4) | lo."""
    if len(data) % 2 != 0:
        raise ValueError(f"odd nybble count: {len(data)}")
    return bytes((data[i] << 4) | data[i + 1] for i in range(0, len(data), 2))


def parse_sysex_packets(raw: bytes) -> list[tuple[int, int, bytes]]:
    """Return list of (message_type, model, payload) for each F0…F7 packet."""
    packets = []
    i = 0
    while i < len(raw):
        if raw[i] != 0xF0:
            i += 1
            continue
        end = raw.find(0xF7, i)
        if end == -1:
            break
        pkt = raw[i:end + 1]
        if (len(pkt) >= 8
                and pkt[1] == ENSONIQ_MFR
                and pkt[2] == VFX_FAMILY):
            model        = pkt[3]
            message_type = pkt[5]
            nybbles      = pkt[6:-1]
            try:
                payload = denybblize(nybbles)
            except ValueError as e:
                print(f"  [skip packet at {i:#x}: {e}]")
                i = end + 1
                continue
            packets.append((message_type, model, payload))
        i = end + 1
    return packets


def decode_seq_headers(data: bytes, count: int) -> list[dict]:
    """Parse sequence headers from a contiguous block."""
    headers = []
    for slot in range(count):
        off = slot * SYSEX_HEADER_SIZE
        if off + SYSEX_HEADER_SIZE > len(data):
            break
        h = data[off:off + SYSEX_HEADER_SIZE]
        name_raw  = h[2:14]
        data_size = struct.unpack(">I", h[183:187])[0] & 0xFFFFFF  # 3 bytes actually
        ptr       = struct.unpack(">I", h[179:183])[0]
        # data_size is at bytes 183–185 (3 bytes BE) per Giebler
        data_size = struct.unpack(">I", b'\x00' + h[183:186])[0]
        ptr       = struct.unpack(">I", h[179:183])[0]
        try:
            name = name_raw.decode('ascii', errors='replace').rstrip()
        except Exception:
            name = repr(name_raw)
        headers.append({'slot': slot, 'name': name, 'data_size': data_size, 'ptr': ptr})
    return headers


def find_match_offset(needle: bytes, haystack: bytes, search_limit: int = 512) -> int | None:
    """Try offsets 0..search_limit to find where needle[:50] matches haystack."""
    probe = needle[:64]
    for off in range(search_limit):
        if haystack[off:off + len(probe)] == probe:
            return off
    return None


def hexdump(data: bytes, label: str, max_bytes: int = 64) -> None:
    print(f"\n{label} ({len(data)} bytes total, first {min(max_bytes, len(data))}):")
    chunk = data[:max_bytes]
    for i in range(0, len(chunk), 16):
        row = chunk[i:i + 16]
        hex_part = ' '.join(f'{b:02x}' for b in row)
        print(f"  {i:4x}: {hex_part}")


def main() -> None:
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(1)

    sysex_path = sys.argv[1]
    disk_path  = sys.argv[2]

    # ── Load SysEx ──────────────────────────────────────────────────────────
    print(f"Loading SysEx: {sysex_path}")
    with open(sysex_path, 'rb') as f:
        raw_sysex = f.read()
    print(f"  Raw size: {len(raw_sysex)} bytes")

    packets = parse_sysex_packets(raw_sysex)
    print(f"  Packets found: {len(packets)}")
    for i, (mt, model, payload) in enumerate(packets):
        print(f"    Packet {i}: type=0x{mt:02x}, model=0x{model:02x}, payload={len(payload)} bytes")

    allseq_packets = [(mt, model, pl) for mt, model, pl in packets if mt == MSG_ALLSEQ]
    if not allseq_packets:
        print("ERROR: no AllSequences (0x0A) packet found")
        sys.exit(1)

    _, _, payload = allseq_packets[0]
    print(f"\nAllSequences payload: {len(payload)} bytes")

    # ── Decode SysEx structure ───────────────────────────────────────────────
    ptr_table   = payload[:SYSEX_PTR_TABLE_SIZE]
    total_size  = len(payload)
    global_sec  = payload[-SYSEX_GLOBAL_SIZE:]
    headers_sec = payload[-(SYSEX_GLOBAL_SIZE + SYSEX_HEADER_COUNT * SYSEX_HEADER_SIZE):-SYSEX_GLOBAL_SIZE]
    event_data  = payload[SYSEX_PTR_TABLE_SIZE:-(SYSEX_GLOBAL_SIZE + SYSEX_HEADER_COUNT * SYSEX_HEADER_SIZE)]

    print(f"  Pointer table: bytes 0–{SYSEX_PTR_TABLE_SIZE - 1}")
    print(f"  Event data:    bytes {SYSEX_PTR_TABLE_SIZE}–{SYSEX_PTR_TABLE_SIZE + len(event_data) - 1} ({len(event_data)} bytes)")
    print(f"  Headers:       bytes {total_size - SYSEX_GLOBAL_SIZE - len(headers_sec)}–{total_size - SYSEX_GLOBAL_SIZE - 1}")
    print(f"  Global:        bytes {total_size - SYSEX_GLOBAL_SIZE}–{total_size - 1}")

    # Pointer table values
    print("\nPointer table (60 × 4-byte BE values, first 16):")
    for i in range(min(16, 60)):
        val = struct.unpack(">I", ptr_table[i*4:(i+1)*4])[0]
        print(f"  [{i:2d}] 0x{val:08x} = {val}")

    # Sequence headers from SysEx
    headers = decode_seq_headers(headers_sec, SYSEX_HEADER_COUNT)
    defined = [h for h in headers if h['data_size'] > 0 or h['name'].strip()]
    print(f"\nSysEx sequence headers (defined slots):")
    for h in defined[:12]:
        print(f"  Slot {h['slot']:2d}: name={h['name']!r:14s}  data_size={h['data_size']:6d}  ptr=0x{h['ptr']:08x}={h['ptr']}")

    # Global section
    curr_seq  = struct.unpack(">H", global_sec[0:2])[0]
    size_sum  = struct.unpack(">I", global_sec[2:6])[0]
    print(f"\nGlobal: curr_seq={curr_seq}, size_sum=0x{size_sum:08x} ({size_sum})")

    # ── Load on-disk file ────────────────────────────────────────────────────
    print(f"\nLoading disk image: {disk_path}")
    with open(disk_path, 'rb') as f:
        f.seek(DISK_BLOCK * 512)
        disk_file = f.read(DISK_FILE_SIZE)
    print(f"  Read {len(disk_file)} bytes from block {DISK_BLOCK}")

    disk_seq_data = disk_file[DISK_SEQ_OFFSET:]
    print(f"  Sequence data region: bytes {DISK_SEQ_OFFSET}–{DISK_FILE_SIZE - 1} ({len(disk_seq_data)} bytes)")

    hexdump(disk_seq_data, "On-disk sequence data (start)")

    # ── Compare SysEx[240:] with disk sequence data ──────────────────────────
    print("\n" + "=" * 60)
    print("COMPARISON: SysEx event_data vs on-disk sequence data")
    print("=" * 60)

    # Direct comparison (hypothesis B: ptr table is separate, event data starts at 240)
    hexdump(event_data, "SysEx event data (payload[240:])")

    if event_data[:64] == disk_seq_data[:64]:
        print("\n✓ DIRECT MATCH: SysEx[240:] == disk[44032:] (first 64 bytes identical)")
    else:
        print("\n✗ No direct match at offset 0. Searching for alignment...")
        off = find_match_offset(event_data, disk_seq_data, 512)
        if off is not None:
            print(f"  ✓ Match found: disk_seq_data[{off}:] == event_data[:64]")
        else:
            # Try the other direction: event_data starts later in disk_seq_data
            off2 = find_match_offset(disk_seq_data, event_data, 512)
            if off2 is not None:
                print(f"  ✓ Match found: event_data[{off2}:] == disk_seq_data[:64]")
            else:
                print("  ✗ No alignment found in first 512 bytes either direction")

    # Also try full payload from byte 0 (hypothesis: ptr table IS the event data start)
    print("\n-- Also checking: SysEx[0:] vs disk[44032:] --")
    full_from_zero = payload[:64]
    if full_from_zero == disk_seq_data[:64]:
        print("✓ MATCH: payload[0:] == disk_seq_data (ptr table IS the event data)")
    else:
        print("✗ No match for payload[0:] either")

    # ── Pointer table vs on-disk offsets ─────────────────────────────────────
    print("\n" + "=" * 60)
    print("POINTER TABLE INTERPRETATION")
    print("=" * 60)
    print("Testing: does ptr + 240 point to start of a sequence's event data?")
    print("(Hypothesis A: ptrs are relative to event_data section start)")
    print()
    for h in defined[:8]:
        ptr = h['ptr']
        if ptr > len(event_data):
            print(f"  Slot {h['slot']:2d}: ptr={ptr} OUT OF RANGE for event_data ({len(event_data)} bytes)")
            continue
        context = event_data[ptr:ptr + 8]
        print(f"  Slot {h['slot']:2d}: ptr={ptr:6d}  event_data[ptr..ptr+8] = {context.hex(' ')}")

    print()
    print("Testing: does ptr = absolute offset in disk_seq_data?")
    for h in defined[:8]:
        ptr = h['ptr']
        if ptr > len(disk_seq_data):
            print(f"  Slot {h['slot']:2d}: ptr={ptr} OUT OF RANGE for disk_seq_data ({len(disk_seq_data)} bytes)")
            continue
        context = disk_seq_data[ptr:ptr + 8]
        print(f"  Slot {h['slot']:2d}: ptr={ptr:6d}  disk_seq_data[ptr..ptr+8] = {context.hex(' ')}")


if __name__ == '__main__':
    main()
