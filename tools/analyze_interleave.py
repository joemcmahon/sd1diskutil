#!/usr/bin/env python3
"""
Determine the correct interleave mapping for SD-1 sixty-programs disk format.

Usage:
    python3 tools/analyze_interleave.py <sysex_file> <disk_image>

Example:
    python3 tools/analyze_interleave.py \\
        "/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-DB final (all).syx" \\
        ~/Downloads/db_rebuild.img

The script:
1. Extracts 60 programs from the AllPrograms (0x03) SysEx packet
2. Reads the 31800-byte programs section from the disk (block 203, bytes 11776-43576)
3. Determines the actual on-disk arrangement by searching for each program's
   name bytes (at offset 498..509 within each 530-byte program)
4. Detects whether the disk uses simple concatenation or a byte-level interleave
5. Prints the full mapping: sysex_slot -> disk_position

Ground truth reference:
  ~/Downloads/db_rebuild.img  VST3-written disk, FSEP at block 203 (178 blocks, 87469 bytes)
  Programs section: bytes 11776-43576 within FSEP = 31800 bytes = 60 × 530
"""

import sys
import struct

ENSONIQ_MFR   = 0x0F
VFX_FAMILY    = 0x05
MSG_ALLPROG   = 0x03

PROGRAM_SIZE  = 530
PROG_COUNT    = 60
PROG_NAME_OFF = 498   # program name at bytes 498..509 within each 530-byte program
PROG_NAME_LEN = 11

FSEP_BLOCK       = 203
FSEP_FILE_SIZE   = 87469
PROGRAMS_OFFSET  = 11776   # within FSEP: start of programs section
PROGRAMS_END     = 43576   # exclusive: 11776 + 31800
PROGRAMS_SIZE    = PROGRAMS_END - PROGRAMS_OFFSET  # 31800


def denybblize(data: bytes) -> bytes:
    if len(data) % 2 != 0:
        raise ValueError(f"odd nybble count: {len(data)}")
    return bytes((data[i] << 4) | data[i + 1] for i in range(0, len(data), 2))


def parse_sysex_packets(raw: bytes) -> list[tuple[int, int, bytes]]:
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
                i = end + 1
                continue
            packets.append((message_type, model, payload))
        i = end + 1
    return packets


def prog_name(prog_bytes: bytes) -> str:
    raw = prog_bytes[PROG_NAME_OFF:PROG_NAME_OFF + PROG_NAME_LEN]
    return raw.decode('ascii', errors='replace').rstrip()


def b10_to_location(b10: int) -> str:
    """Convert b10 index (0-59) to INT bank/patch label."""
    bank  = b10 // 6
    patch = b10 % 6
    return f"INT{bank+1} patch{patch+1} (b10={b10})"


def try_simple_concat(disk_data: bytes, sysex_progs: list[bytes]) -> list[int] | None:
    """
    Try hypothesis: disk_data = prog[mapping[0]] || prog[mapping[1]] || ...
    Returns mapping list if every 530-byte slot matches a unique SysEx program, else None.
    """
    mapping = []
    for slot in range(PROG_COUNT):
        chunk = disk_data[slot * PROGRAM_SIZE : (slot + 1) * PROGRAM_SIZE]
        found = None
        for p, prog in enumerate(sysex_progs):
            if chunk == prog:
                found = p
                break
        if found is None:
            return None
        mapping.append(found)
    return mapping


def try_byte_interleave(disk_data: bytes, sysex_progs: list[bytes]) -> list[int] | None:
    """
    Try hypothesis: current Rust code's interleave.
    even_data = disk[0,2,4,...,31798]  → 15900 bytes → progs at k*530..(k+1)*530 are sysex[2k]
    odd_data  = disk[1,3,5,...,31799]  → 15900 bytes → progs at k*530..(k+1)*530 are sysex[2k+1]
    Returns mapping[disk_slot] = sysex_slot if all match, else None.
    """
    even_data = bytes(disk_data[i] for i in range(0, len(disk_data), 2))
    odd_data  = bytes(disk_data[i] for i in range(1, len(disk_data), 2))
    mapping = [None] * PROG_COUNT
    for k in range(30):
        even_chunk = even_data[k * PROGRAM_SIZE : (k + 1) * PROGRAM_SIZE]
        odd_chunk  = odd_data[k * PROGRAM_SIZE  : (k + 1) * PROGRAM_SIZE]
        for p, prog in enumerate(sysex_progs):
            if even_chunk == prog:
                mapping[2 * k] = p
            if odd_chunk == prog:
                mapping[2 * k + 1] = p
    if all(m is not None for m in mapping):
        return mapping
    return None


def name_search_mapping(disk_data: bytes, sysex_progs: list[bytes]) -> tuple[str, list]:
    """
    Search for each SysEx program's name bytes in the disk data to find its location.
    Works regardless of interleave scheme.
    Returns (method, results) where results is a list of dicts.
    """
    results = []
    for p, prog in enumerate(sysex_progs):
        name_bytes = prog[PROG_NAME_OFF:PROG_NAME_OFF + PROG_NAME_LEN]
        name_str   = prog_name(prog)

        # Search in raw disk data
        hits = []
        pos = 0
        while True:
            idx = disk_data.find(name_bytes, pos)
            if idx == -1:
                break
            hits.append(idx)
            pos = idx + 1

        results.append({
            'sysex_slot': p,
            'name':       name_str,
            'disk_hits':  hits,
        })
    return results


def main() -> None:
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(1)

    sysex_path = sys.argv[1]
    disk_path  = sys.argv[2]

    # ── Load SysEx ────────────────────────────────────────────────────────────
    print(f"Loading SysEx: {sysex_path}")
    with open(sysex_path, 'rb') as f:
        raw_sysex = f.read()
    print(f"  Raw size: {len(raw_sysex)} bytes")

    packets = parse_sysex_packets(raw_sysex)
    allprog = [(mt, model, pl) for mt, model, pl in packets if mt == MSG_ALLPROG]
    if not allprog:
        print("ERROR: no AllPrograms (0x03) packet found")
        sys.exit(1)

    _, _, payload = allprog[0]
    print(f"  AllPrograms payload: {len(payload)} bytes (expected {PROG_COUNT * PROGRAM_SIZE})")
    if len(payload) != PROG_COUNT * PROGRAM_SIZE:
        print(f"ERROR: payload size mismatch")
        sys.exit(1)

    sysex_progs = [payload[p * PROGRAM_SIZE : (p + 1) * PROGRAM_SIZE]
                   for p in range(PROG_COUNT)]

    print(f"\nSysEx programs (first 10):")
    for p in range(10):
        print(f"  [{p:2d}] {prog_name(sysex_progs[p])!r}")

    # ── Load disk programs section ────────────────────────────────────────────
    print(f"\nLoading disk: {disk_path}")
    with open(disk_path, 'rb') as f:
        f.seek(FSEP_BLOCK * 512)
        fsep_raw = f.read(FSEP_FILE_SIZE)
    print(f"  Read {len(fsep_raw)} bytes from block {FSEP_BLOCK}")

    disk_data = fsep_raw[PROGRAMS_OFFSET:PROGRAMS_END]
    print(f"  Programs section: bytes {PROGRAMS_OFFSET}–{PROGRAMS_END-1} ({len(disk_data)} bytes)")
    assert len(disk_data) == PROGRAMS_SIZE

    # ── Hypothesis 1: simple concatenation ───────────────────────────────────
    print("\n" + "=" * 60)
    print("HYPOTHESIS 1: Simple concatenation (disk[k*530:(k+1)*530] = sysex[mapping[k]])")
    print("=" * 60)
    mapping1 = try_simple_concat(disk_data, sysex_progs)
    if mapping1:
        print("✓ MATCH: All 60 programs found as contiguous 530-byte chunks")
        print("\nMapping (disk_slot → sysex_slot):")
        for disk_slot, sysex_slot in enumerate(mapping1):
            print(f"  disk[{disk_slot:2d}] ({b10_to_location(disk_slot):<24}) "
                  f"← sysex[{sysex_slot:2d}] {prog_name(sysex_progs[sysex_slot])!r}")
    else:
        print("✗ No: programs do not appear as contiguous 530-byte chunks")

    # ── Hypothesis 2: byte interleave (current Rust code) ────────────────────
    print("\n" + "=" * 60)
    print("HYPOTHESIS 2: Byte-interleave (current Rust interleave_sixty_programs)")
    print("  even_data = disk[0,2,4,...], odd_data = disk[1,3,5,...]")
    print("  even_data[k*530:(k+1)*530] = sysex[2k], odd_data[k*530:(k+1)*530] = sysex[2k+1]")
    print("=" * 60)
    mapping2 = try_byte_interleave(disk_data, sysex_progs)
    if mapping2:
        print("✓ MATCH: All 60 programs found via byte-interleave de-interleave")
        print("\nMapping (disk_slot → sysex_slot):")
        for disk_slot, sysex_slot in enumerate(mapping2):
            print(f"  disk[{disk_slot:2d}] ({b10_to_location(disk_slot):<24}) "
                  f"← sysex[{sysex_slot:2d}] {prog_name(sysex_progs[sysex_slot])!r}")
    else:
        print("✗ No: byte-interleave de-interleave does not recover all programs")
        # Show partial matches
        even_data = bytes(disk_data[i] for i in range(0, len(disk_data), 2))
        odd_data  = bytes(disk_data[i] for i in range(1, len(disk_data), 2))
        print("\nPartial even-data matches (first 5 mismatches):")
        misses = 0
        for k in range(30):
            chunk = even_data[k * PROGRAM_SIZE : (k + 1) * PROGRAM_SIZE]
            found = next((p for p, prog in enumerate(sysex_progs) if chunk == prog), None)
            if found is None:
                name_in_chunk = chunk[PROG_NAME_OFF:PROG_NAME_OFF+PROG_NAME_LEN]
                print(f"  even_data[{k}] (disk_slot {2*k}) name={name_in_chunk.decode('ascii','replace').rstrip()!r} → NO MATCH")
                misses += 1
                if misses >= 5:
                    break
        print("\nPartial odd-data matches (first 5 mismatches):")
        misses = 0
        for k in range(30):
            chunk = odd_data[k * PROGRAM_SIZE : (k + 1) * PROGRAM_SIZE]
            found = next((p for p, prog in enumerate(sysex_progs) if chunk == prog), None)
            if found is None:
                name_in_chunk = chunk[PROG_NAME_OFF:PROG_NAME_OFF+PROG_NAME_LEN]
                print(f"  odd_data[{k}] (disk_slot {2*k+1}) name={name_in_chunk.decode('ascii','replace').rstrip()!r} → NO MATCH")
                misses += 1
                if misses >= 5:
                    break

    # ── Name-based search (format-agnostic) ───────────────────────────────────
    print("\n" + "=" * 60)
    print("NAME SEARCH: Locate each SysEx program's name in raw disk data")
    print("=" * 60)
    results = name_search_mapping(disk_data, sysex_progs)

    found_count = sum(1 for r in results if r['disk_hits'])
    print(f"Programs found in disk data: {found_count}/{PROG_COUNT}")

    # Show known anchor: KOTO-DREAMS should be at b10=32 (INT6 patch3)
    print("\nAnchor check — KOTO-DREAMS should be at b10=32 on VST3 disk:")
    for r in results:
        if 'KOTO' in r['name']:
            for hit in r['disk_hits']:
                # Infer disk slot from hit offset
                if hit % PROGRAM_SIZE == PROG_NAME_OFF:
                    disk_slot = hit // PROGRAM_SIZE
                    print(f"  sysex[{r['sysex_slot']}] {r['name']!r} → disk byte {hit} "
                          f"= slot {disk_slot} ({b10_to_location(disk_slot)})")
                else:
                    print(f"  sysex[{r['sysex_slot']}] {r['name']!r} → disk byte {hit} "
                          f"(offset within slot: {hit % PROGRAM_SIZE} — not at name offset, check interleave)")

    print("\nAll programs with disk hits (first 20):")
    for r in results[:20]:
        if r['disk_hits']:
            hits_str = ', '.join(str(h) for h in r['disk_hits'][:3])
            print(f"  sysex[{r['sysex_slot']:2d}] {r['name']!r:<14} → disk byte(s) {hits_str}")
        else:
            print(f"  sysex[{r['sysex_slot']:2d}] {r['name']!r:<14} → NOT FOUND")

    # ── What our current Rust code produces vs ground truth ──────────────────
    print("\n" + "=" * 60)
    print("CURRENT RUST INTERLEAVE vs GROUND TRUTH (first 3 mismatched programs)")
    print("=" * 60)

    # Simulate interleave_sixty_programs from Rust
    even_payload = b''.join(payload[k * 2 * PROGRAM_SIZE : (k * 2 + 1) * PROGRAM_SIZE]
                            for k in range(30))
    odd_payload  = b''.join(payload[(k * 2 + 1) * PROGRAM_SIZE : (k * 2 + 2) * PROGRAM_SIZE]
                            for k in range(30))
    rust_interleaved = bytearray(PROGRAMS_SIZE)
    for i in range(15900):
        rust_interleaved[2 * i]     = even_payload[i]
        rust_interleaved[2 * i + 1] = odd_payload[i]
    rust_interleaved = bytes(rust_interleaved)

    if rust_interleaved == disk_data:
        print("✓ Rust output MATCHES ground truth exactly — interleave is correct!")
    else:
        diff_count = sum(a != b for a, b in zip(rust_interleaved, disk_data))
        print(f"✗ Rust output differs from ground truth: {diff_count}/{PROGRAMS_SIZE} bytes differ")

        # Find first differing byte
        first_diff = next(i for i, (a, b) in enumerate(zip(rust_interleaved, disk_data)) if a != b)
        print(f"  First difference at byte {first_diff} (slot {first_diff // PROGRAM_SIZE}, "
              f"offset within slot {first_diff % PROGRAM_SIZE})")

        # Show first 3 program slots where Rust and disk differ
        mismatches = 0
        for slot in range(PROG_COUNT):
            if mapping1:
                rust_prog_idx = slot  # Rust puts sysex[slot] at disk_slot[slot] ... via the interleave
                # Actually for the byte interleave, Rust's disk[slot] comes from sysex[slot]
                # Let's just compare what Rust produces at each 530-byte block after de-interleaving
                pass

        # De-interleave both and compare program-by-program
        def deinterleave(data: bytes) -> list[bytes]:
            even = bytes(data[i] for i in range(0, len(data), 2))
            odd  = bytes(data[i] for i in range(1, len(data), 2))
            progs = []
            for k in range(30):
                progs.append(even[k * PROGRAM_SIZE:(k + 1) * PROGRAM_SIZE])
                progs.append(odd[k * PROGRAM_SIZE:(k + 1) * PROGRAM_SIZE])
            return progs

        rust_deint = deinterleave(rust_interleaved)
        disk_deint = deinterleave(disk_data)

        print("\nDe-interleaved comparison (first 10 slots):")
        print(f"  {'slot':>4}  {'Rust program name':<16}  {'Disk program name':<16}  match?")
        for slot in range(10):
            rust_name = rust_deint[slot][PROG_NAME_OFF:PROG_NAME_OFF+PROG_NAME_LEN]
            disk_name = disk_deint[slot][PROG_NAME_OFF:PROG_NAME_OFF+PROG_NAME_LEN]
            rust_str  = rust_name.decode('ascii', 'replace').rstrip()
            disk_str  = disk_name.decode('ascii', 'replace').rstrip()
            match = "✓" if rust_deint[slot] == disk_deint[slot] else "✗"
            print(f"  [{slot:2d}]  {rust_str!r:<16}  {disk_str!r:<16}  {match}")


if __name__ == '__main__':
    main()
