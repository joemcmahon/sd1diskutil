# sd1diskutil

Command-line utility for managing Ensoniq SD-1 and VFXsd synthesizer disk images.

Supports reading, writing, extracting, and deleting Programs, Presets, and
Sequences stored on floppy disk images. Files are transferred in MIDI
SysEx format, compatible with hardware sysex librarians and DAWs.

Also supports HFE v1 flux image format, used by the HxC floppy emulator and
the Sojus Records SD-1 VST3 plugin. See [HFE format and pre-0.9.8 disk write bug](#hfe-format-and-the-mame-bug) for version compatibility notes.

---

## Contents

- [Installation](#installation)
- [Quick start](#quick-start)
- [Commands](#commands)
  - [list](#list)
  - [inspect](#inspect)
  - [write](#write)
  - [extract](#extract)
  - [delete](#delete)
  - [create](#create)
  - [hfe-to-img](#hfe-to-img)
  - [img-to-hfe](#img-to-hfe)
  - [inspect-sysex](#inspect-sysex)
  - [dump-programs](#dump-programs)
- [HFE format and pre-0.9.8 disk write bug](#hfe-format-and-pre-0.9.8-disk-write-bug)
- [SD-1 disk format overview](#sd-1-disk-format-overview)
- [Supported file types](#supported-file-types)
- [Known limitations](#known-limitations)

---

## Installation

Requires [Rust](https://rustup.rs/) 1.75 or later.

```sh
git clone https://github.com/yourname/sd1diskutil
cd sd1diskutil
cargo build --release
```

The binary is created at `./target/release/sd1cli`.

Copy it to a location on your `PATH`:

```sh
cp target/release/sd1cli /usr/local/bin/
```

---

## Quick start

```sh
# List all files on a disk image
sd1cli list my_sounds.img

# Inspect free space and FAT health
sd1cli inspect my_sounds.img

# Write a SysEx file to a disk image
sd1cli write my_sounds.img patch.syx

# Extract a file as SysEx
sd1cli extract my_sounds.img MYPROG --out myprog.syx

# Delete a file
sd1cli delete my_sounds.img MYPROG

# Create a new blank disk image
sd1cli create new_disk.img

# Convert a flat .img to HFE (for use with HxC emulator or Sojus VST3)
sd1cli img-to-hfe my_sounds.img my_sounds.hfe

# Convert an HFE back to a flat .img
sd1cli hfe-to-img my_sounds.hfe my_sounds.img

# Inspect the contents of a SysEx file
sd1cli inspect-sysex dump.syx

# Show the 60 programs embedded in a SixtySequences file
sd1cli dump-programs my_sequences.img --file COUNTRY
```

---

## Commands

Run `sd1cli --help` for a summary, or `sd1cli <COMMAND> --help` for full details.

### list

```
sd1cli list <IMAGE>
```

Lists all files on a disk image. Prints a table with each file's name, type,
size in blocks and bytes, and directory slot number. Ends with a summary of
file count and free blocks.

**Example:**

```
$ sd1cli list disk_with_everything.img
NAME         TYPE                    BLOCKS  BYTES SLOT
--------------------------------------------------------
MYPROG       OneProgram                   2    530    0
MYPRESET     OnePreset                    2    530    1
MYSEQ        OneSequence                  4   1560    2
...

49 file(s), 5 free blocks
```

---

### inspect

```
sd1cli inspect <IMAGE>
```

Shows disk metadata without modifying the image: path, free block counts, and
a File Allocation Table (FAT) summary (free / used / bad blocks).

> **Note:** The OS-block free count may read `0` on images written by hardware or the SD-1 emulator.
> The FAT-derived count is always accurate.

**Example:**

```
$ sd1cli inspect blank_image.img
Disk image: blank_image.img
Free blocks: 0
Total blocks: 1600 (23 reserved, 1577 usable)
FAT: 1569 free, 8 used, 0 bad
```

---

### write

```
sd1cli write <IMAGE> <SYSEX> [--name <NAME>] [--dir <1-4>] [--overwrite]
```

Writes a SysEx file into a disk image. The SysEx message must be one of:
`OneProgram`, `OnePreset`, `SingleSequence`, or `AllSequences`.

| Flag | Default | Description |
|------|---------|-------------|
| `--name NAME` | SysEx filename stem | Override the name stored on disk (max 11 characters, A–Z 0–9 space) |
| `--dir 1-4` | First directory with free space | Target sub-directory (1–4) |
| `--overwrite` | off | Replace existing file with the same name |

**Examples:**

```sh
# Write using the SysEx filename as the disk name
sd1cli write sounds.img mypatch.syx

# Override the stored name
sd1cli write sounds.img mypatch.syx --name "BASS DRV"

# Write to sub-directory 2
sd1cli write sounds.img mypatch.syx --dir 2

# Overwrite an existing file
sd1cli write sounds.img mypatch.syx --overwrite
```

---

### extract

```
sd1cli extract <IMAGE> <NAME> [--out <PATH>] [--channel <0-15>]
```

Extracts a named file from a disk image and saves it as a SysEx `.syx` file.

| Flag | Default | Description |
|------|---------|-------------|
| `--out PATH` | `<NAME>.syx` | Output file path |
| `--channel N` | `0` (MIDI channel 1) | MIDI channel embedded in the SysEx header |

**Examples:**

```sh
# Extract to MYPROG.syx in the current directory
sd1cli extract sounds.img MYPROG

# Extract with a custom output path
sd1cli extract sounds.img MYPROG --out ~/Desktop/myprog_backup.syx

# Extract targeting MIDI channel 3
sd1cli extract sounds.img MYPROG --channel 2
```

---

### delete

```
sd1cli delete <IMAGE> <NAME>
```

Deletes a named file from the disk image. The file's blocks are freed in the
FAT and the directory entry is removed. The image is saved immediately.

> **Warning:** This cannot be undone. Make a backup of the image first if needed.

**Example:**

```sh
sd1cli delete sounds.img MYPROG
# Deleted: MYPROG (2 block(s) freed)
```

---

### create

```
sd1cli create <IMAGE>
```

Creates a new blank SD-1 disk image (819,200 bytes — 1600 × 512-byte blocks)
pre-formatted with SD-1 OS structures. Blocks 0–22 are reserved; blocks 23–1599
are available for files.


---

### hfe-to-img

```
sd1cli hfe-to-img <HFE> <IMG>
```

Converts an HFE v1 flux image to a flat SD-1 `.img` disk image. Decodes the MFM
bitstream for all 80 tracks × 2 sides × 10 sectors and verifies CRC16-CCITT for
every sector. Fails with a descriptive error on bad signature, unsupported HFE
revision, CRC mismatch, or missing sector.

**Example:**

```sh
sd1cli hfe-to-img my_sounds.hfe my_sounds.img
```

---

### img-to-hfe

```
sd1cli img-to-hfe <IMG> <HFE>
```

Converts a flat SD-1 `.img` disk image to HFE v1 flux format. Encodes all 80
tracks × 2 sides × 10 sectors as MFM bitstream with correct CRC16-CCITT
checksums. The output is compatible with the HxC floppy emulator and the Sojus
VST3 plugin. Uses atomic write (writes to `.hfe.tmp` then renames).

**Example:**

```sh
sd1cli img-to-hfe my_sounds.img my_sounds.hfe
```

---

### inspect-sysex

```
sd1cli inspect-sysex <SYSEX>
```

Parses a SysEx `.syx` file and displays its contents without touching any disk image.

| Message type | Output |
|---|---|
| `AllPrograms` | Lists all 60 program slot names |
| `OneProgram` | Shows program name and payload size |
| `AllPresets` | Shows preset count and size |
| `AllSequences` / `SingleSequence` | Shows message type and payload size |
| Other | Shows message type byte and payload size |

**Example:**

```sh
sd1cli inspect-sysex my_sequences.syx
```

---

### dump-programs

```
sd1cli dump-programs <IMAGE> [--file <PREFIX>] [--sysex <SYSEX>]
```

Shows all 60 programs embedded in a `SixtySequences+Programs` disk file, and
decodes the track-program assignments for the first three defined sequences.
Program names are resolved: RAM slot indices are shown with their actual program
name from the file, and ROM references are decoded to bank and patch name.

| Flag | Default | Description |
|------|---------|-------------|
| `--file PREFIX` | First SixtySequences+Programs file found | File name prefix to search for |
| `--sysex PATH` | (none) | AllPrograms SysEx file to compare slot-by-slot against the disk |

**Examples:**

```sh
# Dump programs from the first SixtySequences+Programs file on a disk
sd1cli dump-programs my_sequences.img

# Target a specific file by name prefix
sd1cli dump-programs my_sequences.img --file COUNTRY

# Compare on-disk programs against a SysEx AllPrograms dump
sd1cli dump-programs my_sequences.img --file COUNTRY --sysex my_programs.syx
```

---

## HFE format and pre-0.9.8 disk write bug

[HFE](https://hxc2001.com/download/floppy_drive_emulator/SDCard_HxC_Floppy_Emulator_HFE_file_format.pdf)
is a raw MFM flux image format used by the HxC floppy emulator family and the
Sojus VST3 plugin. Instead of cooked sectors, it stores the actual bitstream the
read head would encounter — flux transitions encoded as ones and zeros, with full
MFM encoding and CRC fields intact.

**Version 0.9.8 and later of the Sojus SD-1 VST3 plugin handle `.img` files
correctly.** You can use flat `.img` files directly with the plugin — no HFE
conversion needed. If you are already using HFE files and prefer to continue
doing so, that is equally safe, though less convenient.

**Version 0.9.7 and earlier** had a sector-numbering mismatch in the MAME floppy
backend. When saving a `.img` file, the emulator routed writes through MAME's
`get_track_data_mfm_pc`, which expects PC-standard sector numbering (sectors 1–10),
while the Ensoniq SD-1 uses sectors 0–9. MAME silently discarded sector 0 of
every track, shifted the remaining sectors down by one, and zeroed the last
sector per track.

On a 160-track SD-1 disk this corrupted every tenth block (blocks 0, 10, 20, …).

**Data in those sectors is unrecoverable.**

HFE files written by the 0.9.7
emulator were *not* affected — the HFE raw bitstream bypasses MAME's sector
extraction entirely.

### Recommended workflow with the SD-1 emulator VST3

#### Version 0.9.8 and later

`.img` files work correctly with the plugin. Use `sd1cli` directly:

1. Use `sd1cli` write/delete/create commands to manage your `.img` file.
2. Load the `.img` directly into the SD-1 plugin.
3. Do any disk operations you please in the SD-1 emulator.
4. After saving files from the plugin, use `sd1cli list` to verify.

#### Version 0.9.7 and earlier

Do **not** use `.img` files as the live working copy in the plugin. Always use an HFE *copy* as the interchange format:

1. Prepare your disk as a flat `.img` using `sd1cli` write/delete/create commands.
2. Convert to HFE: `sd1cli img-to-hfe my_sounds.img my_sounds.hfe`
3. Load `my_sounds.hfe` into the SD-1 plugin.
4. After saving files from the plugin, convert back: `sd1cli hfe-to-img my_sounds.hfe my_sounds.img`
5. Use `sd1cli list` to verify the saved files appear correctly.

---

## SD-1 disk format overview

SD-1 disks are 800 KB double-density floppy images (1600 blocks × 512 bytes).

| Block range | Purpose |
|-------------|---------|
| 0–4 | OS / boot blocks |
| 5–14 | File Allocation Table (FAT) — 170 entries/block × 3 bytes/entry |
| 15–22 | Sub-directory blocks (2 blocks per directory × 4 directories) |
| 23–1599 | File data |

**File Allocation Table:** Each 3-byte FAT entry encodes a next-block pointer or
a sentinel value (Free, EndOfFile, BadBlock). Files are stored as chains of
blocks linked through the FAT.

**Sub-directories:** Four sub-directories, each holding up to 39 entries
(156 files total). Each directory entry is 26 bytes and records the file name
(space-padded, not null-terminated), file type, first block, size in blocks,
size in bytes, and a file number.

**Names:** Up to 11 characters, uppercase A–Z, digits 0–9, and space.
Names are compared case-insensitively by the utility.

**Multi-byte fields:** All multi-byte integers on disk are big-endian.

---

## Supported file types

| SysEx type | Disk file type | Description |
|------------|---------------|-------------|
| OneProgram | `OneProgram` | Single Program (530 bytes payload) |
| OnePreset | `OnePreset` | Single Preset (530 bytes payload) |
| SingleSequence | `OneSequence` | Single Sequence |
| AllSequences | `ThirtySequences` / `SixtySequences` | Full sequence bank |

We do not currently support copying the sequencer OS, but it would be easy to add. Please file an issue if you want it.

---

## Known limitations

- **Preset names:** Presets have no accessible name field in the on-disk format;
  `list` shows the directory entry name, not an internal patch name.
- **`--dir` capacity check:** if the specified sub-directory is full, the error
  is reported after the FAT allocation step; a pre-flight check is not performed.
- **UniFFI / Swift bindings:** the `sd1disk` library does not yet expose a C or
  Swift API.
- **No `dd` wrapper:** writing images to physical floppy disks requires an
  external tool (`dd`, `balenaEtcher`, etc.).
- **HFE v1 only:** HFE v2 and v3 use a different container format and are not
  supported. All SD-1 HFE files produced by Sojus and HxC emulators are v1.
- **Sojus-corrupted `.img` repair:** `.img` files already damaged by the 0.9.7
  MAME sector-numbering bug cannot be repaired — sector 0 data is unrecoverable.
  Upgrade to 0.9.8 or later and use a fresh image going forward.

## LCD character set vs ASCII

File names are stored as raw bytes and displayed using standard ASCII. The SD-1's
hardware display uses a custom LCD character ROM (HD44780-compatible) that does
not match ASCII for all byte values. For example, byte `0x29` renders as `)` in
this utility but appears as a `4.`-like glyph on the hardware display.

This means names that contain non-alphanumeric characters may look different
in `sd1cli list` output than on the synthesizer's own screen. The on-disk bytes
are stored and retrieved faithfully — the discrepancy is display-only.

### Complete character map (tested against Sojus SD-1 emulator v0.9.8)

Every byte value from `0x00` through `0xFF` has been tested by writing filenames
containing each byte and observing the emulator display.

**Bytes that render as blank (display nothing):**

| Range | Notes |
|-------|-------|
| `0x00`–`0x1F` | Control characters |
| `0x26` `&` | |
| `0x2C` `,` | |
| `0x3A` `:` | |
| `0x3F` `?` | |
| `0x60` `` ` `` | |
| `0x61`–`0x7A` | Lowercase a–z |
| `0x7B`–`0x7F` | `{`, `\|`, `}`, `~`, DEL |
| `0x80`–`0xFF` | Entire upper half of byte range |

**Bytes that render as digit-dot glyphs:**

| Byte | ASCII char | Displays as |
|------|-----------|------------|
| `0x21` | `!` | `0.` |
| `0x23` | `#` | `1.` |
| `0x25` | `%` | `2.` |
| `0x28` | `(` | `3.` |
| `0x29` | `)` | `4.` |
| `0x3B` | `;` | `6.` |
| `0x5C` | `\` | `8.` |

The glyphs `5.`, `7.`, and `9.` couldn't be rendered at any byte value; it's unknown as to whether they are renderable at all, but I was unable to find them by sweeping through all the possible 1-byte values.

**All other bytes `0x20`–`0x5F` render as their standard ASCII equivalents.**
This includes space, `!`–`/` (except those listed above), `0`–`9`, `A`–`Z`,
and `[`, `]`, `^`, `_`.

Non-alphanumerics other than space, /, ., -, +, and *, which can all be entered from the SD-1's interface, may look somewhat odd due to the limitations of the display (e.g., = renders as an underlined dash).
