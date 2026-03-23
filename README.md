# sd1diskutil

Command-line utility for managing Ensoniq SD-1 synthesizer disk images.

Supports reading, writing, extracting, and deleting Programs, Presets, and
Sequences stored on SD-1 floppy disk images. Files are transferred in MIDI
SysEx format, compatible with hardware sysex librarians and DAWs.

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

The binary is placed at `./target/release/sd1cli`.

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

> **Note:** The OS-block free count may read `0` on images written by hardware.
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

The image can be written to a physical floppy disk with a tool such as `dd`:

```sh
sd1cli create /tmp/new_disk.img
dd if=/tmp/new_disk.img of=/dev/rdisk2 bs=512
```

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

---

## Known limitations

These are documented post-v0.1 gaps:

- **Preset names:** Presets have no accessible name field in the on-disk format;
  `list` shows the directory entry name, not an internal patch name.
- **`--dir` capacity check:** if the specified sub-directory is full, the error
  is reported after the FAT allocation step; a pre-flight check is not performed.
- **UniFFI / Swift bindings:** the `sd1disk` library does not yet expose a C or
  Swift API.
- **No `dd` wrapper:** writing images to physical floppy disks requires an
  external tool (`dd`, `balenaEtcher`, etc.).

## LCD character set vs ASCII

File names are stored as raw bytes and displayed using standard ASCII. The SD-1's
hardware display uses a custom LCD character ROM (HD44780-compatible) that does
not match ASCII for all byte values. For example, byte `0x29` renders as `)` in
this utility but appears as a `4.`-like glyph on the hardware display.

This means names that contain non-alphanumeric characters may look different
in `sd1cli list` output than on the synthesizer's own screen. The on-disk bytes
are stored and retrieved faithfully — the discrepancy is display-only.

A future enhancement would be to map the SD-1's LCD character ROM to Unicode so
that `list` can optionally render names as they appear on hardware. This would
require testing file names covering all printable byte values against the hardware
or a cycle-accurate emulator to build the full translation table.
