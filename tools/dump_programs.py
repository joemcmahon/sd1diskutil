#!/usr/bin/env python3
"""
dump_programs.py — Verify program slot mapping in a SixtySequences+60Programs disk file.

Extracts the programs section from a disk image, de-interleaves it using the same
algorithm as the Rust code (interleave_sixty_programs), and prints the name of each
program slot. Optionally also parses an AllPrograms SysEx file for comparison.

Also shows the program numbers referenced by each track in the first defined sequence.

Usage:
    python3 tools/dump_programs.py <disk_image> [file_prefix] [sysex_file]

    disk_image   — SD-1 disk image file
    file_prefix  — file name prefix to find in directory (default: NC12NORTSEQ)
    sysex_file   — optional: .syx file containing AllPrograms packet

Example:
    # Dump our test file programs vs SysEx
    python3 tools/dump_programs.py /tmp/nc12_fixed.img NC12NORTSEQ \\
        "/Volumes/Aux Brain/Music, canonical/Ableton projects/SysEx Librarian/Shatterday/seq-NC 12 North final (all).syx"

    # Dump reference COUNTRY-* to verify our de-interleave algorithm is correct
    python3 tools/dump_programs.py disk_with_everything.img COUNTRY
"""

import sys
import struct

BLOCK_SIZE           = 512
FAT_START_BLOCK      = 5
ENTRIES_PER_FAT_BLK  = 170
FIRST_DATA_BLOCK     = 23

SUBDIR_START_BLOCK   = 15
SUBDIR_ENTRY_SIZE    = 26
SUBDIR_CAPACITY      = 39
SUBDIR_BLOCKS_EACH   = 2

FAT_EOF              = 0x000001

PROGRAMS_OFFSET      = 11776
PROGRAMS_END         = 43576   # exclusive (31800 bytes)
PROGRAM_SIZE         = 530
SIXTY_PROGRAMS_COUNT = 60
PROGRAM_NAME_OFFSET  = 498
PROGRAM_NAME_LEN     = 11

# Sequence header constants
HEADER_SIZE          = 188
HEADER_COUNT         = 60
TRACK_PARAMS_START   = 28      # byte offset within header
TRACK_PARAM_SIZE     = 11
TRACK_COUNT          = 12
# Within each 11-byte track param block: program number byte
TRACK_PARAM_PROG_BYTE = 10     # last byte (b10); empirically confirmed 2026-03-24

# INT0 user bank: 10 banks × 6 patches = 60 programs; index = bank*6 + patch
INT0_PROGRAMS = [
    "ARTIC-ELATE", "OLYMPIANO",   "ALTO-SAX",    "MERLIN",       "WAY-FAT",     "GROOVE-KIT",   # bank 0
    "ALLS-FAIR",   "IN-CONCERT",  "SOLOTRUMPET", "INSPIRED",     "AMEN-CHOIR",  "PASSION",      # bank 1
    "SYMPHONY",    "MY-DESIRE",   "MUTED-HORNS", "STACK-BASS",   "DRAWBARS-1",  "SONOTAR",      # bank 2
    "STRINGS",     "BRASS-STAB",  "MANDOLIN",    "CROWN-CHOIR",  "TUBULAR HIT", "JAZZ-KIT",     # bank 3
    "STRUM-ME",    "LUNAR",       "BLUES-HARP",  "WIDEPUNCH",    "BRIGHT-PNO",  "PIPE-ORGAN1",  # bank 4
    "MALLETS",     "SWEEPER",     "KOTO-DREAMS", "SWELL-SAW",    "WILBUR",      "MEATY-KIT",    # bank 5
    "FIDDLE",      "PEDAL-STEEL", "BANJO-BANJO", "CLOCK-BELLS",  "THE-QUEEN",   "ROCK-KIT-2",   # bank 6
    "SMOOTH-STRG", "DARK-HALL",   "GUITAR-PADS", "FANFARE",      "MINI-LEAD",   "NORM-1-KIT",   # bank 7
    "STRATOS-VOX", "FUNKY-CLAV2", "COOL-FLUTES", "OH-BE-EX",     "DANCEBASS-2", "WOODY-PERC",   # bank 8
    "ANNABELL",    "FUNK-GUITAR", "ELEC-BASS2",  "CLEAR-GUITAR", "STUDIO-CITY", "MEAN-KIT-1",   # bank 9
]

# ROM program lookup: enc = (b10 & 0x7F), rom_index = enc + 8
# Formula: enc = bank*6 + patch - 8  (verified: REEL-STEEL enc=40, ELEC-BASS enc=24)
# ROM 0 indices 0-59 then ROM 1 indices 60-119; enc 0 maps to rom_index 8 (ROM0 bank1 patch2).
# ROM 0 bank 0 (indices 0-5) and bank 1 patches 0-1 (indices 6-7) are unreachable (enc < 0).
ROM_ALL_PROGRAMS = [
    # ROM 0 — 60 programs (indices 0-59)
    "ITS-A-SYNTH", "ZIRCONIUM",    " FAT-BRASS",   "STAR-DRIVE ", " WONDERS ",   "SAW-O-LIFE",   # bank 0
    "DIGIPIANO-1", "NEW-PLANET",   " DANGEROUS ",  " FUNKYCLAV ", "WARM-TINES",  "METAL-TINES",  # bank 1
    " BIG-PIANO ", "BRIGHT-PNO2",  " SYN-PIANO ",  "TRANS-PIANO", "CLASSIC-PNO", "HARPSICHORD",  # bank 2
    "DOUBLE-REED", " TENOR-SAX ",  "WOODFLUTE",    " CHIFFLUTE ", "MALLET+FLTS", "FLUTE-VIL",    # bank 3
    " STARBRASS ", " FRENCHORN ",  " TOP-BRASS ",  "FLUGEL-STRG", "  BRASSY  ",  "SYNTH-HORNS",  # bank 4
    "SMAK-BASS",   "BEBOP-BASS",   "ELEC-BASS",    "SYNTHBASS",   "DANCE-BASS",  "BUZZ-BASS",    # bank 5
    " ORGANIZER",  "NASTY-ORGAN",  "CATHEDRAL-1",  "TIMBRE-ORG",  "ANGELBREATH", " VERYBREATH",  # bank 6
    "SWELLSTRNGS", " PIZZICATO ",  "LUSH-STRNGS",  "GOLDEN-HARP", "REZ-STRINGS", " ORCH+SOLO ",  # bank 7
    "REEL-STEEL",  "SUN-N-MOON",   "FLANG-CLEAN",  " FUZZ-LEAD",  "SPANISH-GTR", " 12-STRING",   # bank 8
    "KITCHN-SINK", "PERCUSSION",   "FUSION-KIT",   " BALLAD-KIT", "SYNTH-KIT",   "ROCKIN-KIT",   # bank 9
    # ROM 1 — 60 programs (indices 60-119)
    "OMNIVERSE",   "FLASH-BACK",   " SD1-PAD",     "SQUARE-PAD",  "NU-MEANING",  "ASCENSION",    # bank 0
    "IN-DEMAND",   " FM-PIANO",    "MANY-ROADS",   "DEEP-TINES",  "PURE-TINE",   "INNOCENCE",    # bank 1
    "STUDIO-GRND", " POP-GRND",    "JAZZ-GRAND",   "CHURCH-GRND", "CLASSIC-GND", "BOWS+GRAND",   # bank 2
    "SOPRANO-SAX", " ALTO-SAX",    "BARI+HORNS",   "HARMONICA",   "SHAKUHACHI",  " PICCOLO +",   # bank 3
    " ODYSSEY",    "MANY-LEADS",   " FUNK-LEAD",   "FUNKY-STABS", " CHICAGO",    "MUTED-HORN",   # bank 4
    "MOOG-MUTE",   "  ANAREZO",    "PERKY-MOOG",   "CROSS-BASS",  "SLICK-ELEC",  "BLEACHBASS",   # bank 5
    "JAZZ-ORGAN",  "DIRTY-ORGAN",  "NU-CHOIR",     "DIGITALIAN",  " CHORALE-2",  "90-S-VOX",     # bank 6
    "DRAMA-STGS",  "NU-STRINGS",   "LUSH-STRG-2",  "  VIOLIN",    "   CELLO",    "  QUARTET",    # bank 7
    "DREAM-GTR",   "JAZZ-GUITAR",  "ELEC-GUITAR",  "DIST-GTR",    "   NU-BEL",   " MULTI-BELL",  # bank 8
    "DRUMS-MAP-R", "808-MAP-R",    "SLAM-MAP-R",   "MULTI-PERCS", "ORCH-PERKS",  " INDO-AFRO",   # bank 9
]

ENSONIQ_MFR  = 0x0F
MSG_ALLPROGS = 0x03

# ─── Disk helpers ────────────────────────────────────────────────────────────

def fat_read(img: bytearray, block: int) -> int:
    fat_blk = FAT_START_BLOCK + block // ENTRIES_PER_FAT_BLK
    off = fat_blk * BLOCK_SIZE + (block % ENTRIES_PER_FAT_BLK) * 3
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
        elif val == 0x000000:
            raise ValueError(f"FAT chain hit free block at {current}")
        else:
            current = val
    return chain


def dir_find(img: bytearray, prefix: str) -> dict | None:
    p = prefix.rstrip('*')
    for subdir in range(3):
        for slot in range(SUBDIR_CAPACITY):
            base_block = SUBDIR_START_BLOCK + subdir * SUBDIR_BLOCKS_EACH
            off = base_block * BLOCK_SIZE + slot * SUBDIR_ENTRY_SIZE
            d = img[off:off + SUBDIR_ENTRY_SIZE]
            if d[1] == 0:
                continue
            try:
                name_str = bytes(d[2:13]).decode('ascii').rstrip()
            except Exception:
                continue
            if name_str.startswith(p):
                return {
                    'name_str':    name_str,
                    'type_info':   d[0],
                    'file_type':   d[1],
                    'first_block': struct.unpack(">I", d[18:22])[0],
                    'size_bytes':  struct.unpack(">I", b'\x00' + d[23:26])[0],
                }
    return None


def extract_file(img: bytearray, entry: dict) -> bytes:
    chain = fat_chain(img, entry['first_block'])
    raw = bytearray()
    for blk in chain:
        raw.extend(img[blk * BLOCK_SIZE:(blk + 1) * BLOCK_SIZE])
    return bytes(raw[:entry['size_bytes']])


# ─── Program helpers ──────────────────────────────────────────────────────────

def deinterleave_sixty_programs(data: bytes) -> bytes:
    """Mirror of Rust deinterleave_sixty_programs."""
    expected = SIXTY_PROGRAMS_COUNT * PROGRAM_SIZE
    assert len(data) == expected, f"expected {expected} bytes, got {len(data)}"
    even_data = bytes(data[i] for i in range(0, expected, 2))   # bytes at even positions
    odd_data  = bytes(data[i] for i in range(1, expected, 2))   # bytes at odd positions
    result = bytearray(expected)
    for k in range(30):
        dst_even = k * 2 * PROGRAM_SIZE
        dst_odd  = (k * 2 + 1) * PROGRAM_SIZE
        result[dst_even:dst_even + PROGRAM_SIZE] = even_data[k * PROGRAM_SIZE:(k + 1) * PROGRAM_SIZE]
        result[dst_odd:dst_odd + PROGRAM_SIZE]   = odd_data[k * PROGRAM_SIZE:(k + 1) * PROGRAM_SIZE]
    return bytes(result)


def program_name(slot_data: bytes) -> str:
    raw = slot_data[PROGRAM_NAME_OFFSET:PROGRAM_NAME_OFFSET + PROGRAM_NAME_LEN]
    # Strip high bits (MSB can be set for mute flags on Ensoniq), mask to 7-bit ASCII
    cleaned = bytes(b & 0x7F for b in raw)
    name = cleaned.rstrip(b'\x00').rstrip(b' ')
    try:
        return name.decode('ascii')
    except Exception:
        return repr(name)


def dump_disk_programs(file_data: bytes, label: str) -> list[str]:
    """De-interleave and return list of 60 program names."""
    if len(file_data) < PROGRAMS_END:
        print(f"  ERROR: file too short ({len(file_data)} bytes) — no programs section")
        return []
    raw = file_data[PROGRAMS_OFFSET:PROGRAMS_END]
    deint = deinterleave_sixty_programs(raw)
    names = []
    for slot in range(SIXTY_PROGRAMS_COUNT):
        slot_data = deint[slot * PROGRAM_SIZE:(slot + 1) * PROGRAM_SIZE]
        names.append(program_name(slot_data))
    print(f"\n{'─'*60}")
    print(f"Programs from disk ({label}):")
    for i, n in enumerate(names):
        print(f"  slot {i:2d}: {n!r}")
    return names


# ─── SysEx helpers ────────────────────────────────────────────────────────────

def denybblize(data: bytes) -> bytes:
    if len(data) % 2 != 0:
        raise ValueError(f"odd nybble count: {len(data)}")
    return bytes((data[i] << 4) | data[i + 1] for i in range(0, len(data), 2))


def parse_sysex_packets(raw: bytes) -> list[tuple[int, bytes]]:
    packets = []
    i = 0
    while i < len(raw):
        if raw[i] != 0xF0:
            i += 1
            continue
        end = raw.index(0xF7, i)
        body = raw[i + 1:end]
        i = end + 1
        if len(body) < 6 or body[0] != ENSONIQ_MFR:
            continue
        msg_type = body[4]   # F0 0F 05 <model> <channel> <msg_type> <nybbles> F7
        nybbles = body[5:]
        try:
            payload = denybblize(nybbles)
        except Exception:
            continue
        packets.append((msg_type, payload))
    return packets


def dump_sysex_programs(syx_path: str) -> list[str]:
    with open(syx_path, 'rb') as f:
        raw = f.read()
    packets = parse_sysex_packets(raw)
    allprogs = [(t, p) for t, p in packets if t == MSG_ALLPROGS]
    if not allprogs:
        print(f"  No AllPrograms packet (type 0x{MSG_ALLPROGS:02X}) found in {syx_path}")
        return []
    _, payload = allprogs[0]
    expected = SIXTY_PROGRAMS_COUNT * PROGRAM_SIZE
    if len(payload) != expected:
        print(f"  AllPrograms payload size {len(payload)}, expected {expected}")
        return []
    names = []
    for slot in range(SIXTY_PROGRAMS_COUNT):
        slot_data = payload[slot * PROGRAM_SIZE:(slot + 1) * PROGRAM_SIZE]
        names.append(program_name(slot_data))
    print(f"\n{'─'*60}")
    print(f"Programs from SysEx ({syx_path}):")
    for i, n in enumerate(names):
        print(f"  slot {i:2d}: {n!r}")
    return names


# ─── Track params ─────────────────────────────────────────────────────────────

def decode_b10(b10: int, disk_programs: list[str] | None = None) -> str:
    """Decode track param b10 (program byte) to a human-readable program name.

    Encoding:
      0x00–0x3B (0–59):  RAM bank (INT0) slot index; resolved against disk_programs when the
                         file embeds a custom program set, otherwise falls back to the default
                         INT0 init programs (the fixed set the SD-1 loads on power-up)
      0x80–0xFE:         ROM program; enc = b10 & 0x7F, rom_index = enc + 8
                         ROM 0 banks 0-9 occupy rom_index 0-59; ROM 1 banks 0-9 occupy 60-119
                         (enc 0 = ROM0 bank1 patch2; ROM0 bank0 and bank1 patches 0-1 unreachable)
      0x7F:              no program change on sequence recall (use current)
      0xFF:              track inactive / no program defined
    """
    if b10 == 0xFF:
        return "(inactive)"
    if b10 == 0x7F:
        return "(no prog change)"
    if b10 <= 0x3B:  # 0–59: RAM (INT0) user bank, addressed as bank*6+patch
        # The de-interleave separates banks 0-4 (even slots) from banks 5-9 (odd slots):
        #   banks 0-4: de-interleaved slot = b10 * 2  (b10 < 30)
        #   banks 5-9: de-interleaved slot = (b10 - 30) * 2 + 1  (b10 >= 30)
        if disk_programs:
            deint_slot = b10 * 2 if b10 < 30 else (b10 - 30) * 2 + 1
            if deint_slot < len(disk_programs):
                return f"RAM[{b10}]={disk_programs[deint_slot]}"
        if b10 < len(INT0_PROGRAMS):
            return f"RAM[{b10}]={INT0_PROGRAMS[b10]}"
        return f"RAM[{b10}]=?"
    if b10 & 0x80:  # ROM program
        enc = b10 & 0x7F
        rom_index = enc + 8
        if rom_index < len(ROM_ALL_PROGRAMS):
            bank_label = "ROM0" if rom_index < 60 else "ROM1"
            return f"{bank_label}[enc={enc}]={ROM_ALL_PROGRAMS[rom_index]}"
        return f"ROM[enc={enc}]=?"
    return f"b10=0x{b10:02X}(?)"


def dump_track_params(file_data: bytes, disk_programs: list[str] | None = None, max_seqs: int = 3) -> None:
    """Print track program numbers from first max_seqs defined sequences.

    disk_programs: list of 60 program names from the file's embedded programs section;
    used to resolve user-bank b10 indices. Falls back to stock INT0 ROM names if absent.
    """
    label = "embedded disk programs" if disk_programs else "INT0 default init programs (fallback)"
    print(f"\n{'─'*60}")
    print(f"Track program numbers from sequence headers (b10 = program byte; resolving via {label}):")
    shown = 0
    for seq in range(HEADER_COUNT):
        hdr = file_data[seq * HEADER_SIZE:(seq + 1) * HEADER_SIZE]
        if hdr[0] == 0xFF:
            continue  # undefined slot
        seq_name_raw = bytes(b & 0x7F for b in hdr[2:13]).rstrip(b'\x00').rstrip(b' ')
        try:
            seq_name = seq_name_raw.decode('ascii')
        except Exception:
            seq_name = repr(seq_name_raw)
        print(f"\n  Seq slot {seq} ({seq_name!r}):")
        for track in range(TRACK_COUNT):
            tp_off = TRACK_PARAMS_START + track * TRACK_PARAM_SIZE
            tp = hdr[tp_off:tp_off + TRACK_PARAM_SIZE]
            b10 = tp[TRACK_PARAM_PROG_BYTE]
            prog_str = decode_b10(b10, disk_programs)
            print(f"    Track {track+1:2d}: {tp.hex()}  b10=0x{b10:02X}  {prog_str}")
        shown += 1
        if shown >= max_seqs:
            break


# ─── Main ─────────────────────────────────────────────────────────────────────

def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    disk_path   = sys.argv[1]
    file_prefix = sys.argv[2] if len(sys.argv) > 2 else "NC12NORTSEQ"
    syx_path    = sys.argv[3] if len(sys.argv) > 3 else None

    with open(disk_path, 'rb') as f:
        img = bytearray(f.read())

    entry = dir_find(img, file_prefix)
    if entry is None:
        print(f"ERROR: '{file_prefix}' not found in {disk_path}")
        sys.exit(1)

    print(f"Found: {entry['name_str']!r}  type_info=0x{entry['type_info']:02X}  "
          f"first_block={entry['first_block']}  size={entry['size_bytes']}")

    file_data = extract_file(img, entry)
    print(f"File size: {len(file_data)} bytes")

    disk_names = dump_disk_programs(file_data, entry['name_str'])
    dump_track_params(file_data, disk_names or None)

    if syx_path:
        syx_names = dump_sysex_programs(syx_path)
        if disk_names and syx_names:
            print(f"\n{'─'*60}")
            print("Slot comparison (disk de-interleaved vs SysEx):")
            mismatches = 0
            for i in range(SIXTY_PROGRAMS_COUNT):
                d = disk_names[i] if i < len(disk_names) else "?"
                s = syx_names[i]  if i < len(syx_names)  else "?"
                match = "OK" if d == s else "MISMATCH"
                if d != s:
                    mismatches += 1
                    print(f"  slot {i:2d}: disk={d!r:20s}  syx={s!r}  ← {match}")
            if mismatches == 0:
                print("  All 60 slots match!")
            else:
                print(f"\n  {mismatches} mismatches out of 60 slots")


if __name__ == '__main__':
    main()
