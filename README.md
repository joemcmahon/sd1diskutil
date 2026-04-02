# sd1diskutil

Command-line utility for managing Ensoniq SD-1 and VFXsd synthesizer disk images.

Supports reading, writing, extracting, and deleting Programs, Presets, and
Sequences stored on SD-1 floppy disk images. Files are transferred in MIDI
SysEx format, compatible with hardware sysex librarians and DAWs.

Also supports HFE v1 flux image format, used by the HxC floppy emulator and
the Sojus VST3 plugin. See [HFE format and the Sojus MAME bug, now being worked on for version 0.9.8](#hfe-format-and-the-sojus-mame-bug).

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
- [HFE format and the Sojus MAME bug](#hfe-format-and-the-sojus-mame-bug)
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

# Convert a flat .img to HFE (for use with HxC emulator or Sojus VST3)
sd1cli img-to-hfe my_sounds.img my_sounds.hfe

# Convert an HFE back to a flat .img
sd1cli hfe-to-img my_sounds.hfe my_sounds.img
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

## HFE format and the Sojus MAME bug

[HFE](https://hxc2001.com/download/floppy_drive_emulator/SDCard_HxC_Floppy_Emulator_HFE_file_format.pdf)
is a raw MFM flux image format used by the HxC floppy emulator family and the
Sojus VST3 plugin. Instead of cooked sectors, it stores the actual bitstream the
read head would encounter — flux transitions encoded as ones and zeros, with full
MFM encoding and CRC fields intact.

### Why HFE matters: the 0.9.7 MAME bug

The 0.9.7 version of the SD-1 VST3 plugin emulates the SD-1's floppy drive via MAME. When saving a
`.img` file, the emulator routes writes through MAME's `get_track_data_mfm_pc`, which
expects PC-standard sector numbering (sectors 1–10). The Ensoniq SD-1 uses
sectors 0–9. Because of this mismatch, MAME silently discards sector 0 of every track, shifts the
remaining sectors down by one position, and zeros the last sector per track.

On a 160-track SD-1 disk, this corrupts every tenth block (blocks 0, 10, 20,
…). **The data in those sectors is gone and cannot be recovered.** A disk image
written by MAME that appears to load correctly may nonetheless have silent data corruption
in its first sector of every track.

**HFE files written by the 0.9.7 version of the emulator are not affected** — the HFE raw bitstream bypasses
MAME's sector extraction entirely; HFE images can be mounted, read, and written safely. 

Sojus Records is working on a workaround for MAME's mangling of the sectors and expects to
have it ready for version 0.9.8; when the issue is fixed, we'll remove this section (but
keep HFE support).

### Recommended workflow with the SD-1 emulator VST3

1. Prepare your disk as a flat `.img` using `sd1cli` write/delete/create commands.
2. Convert to HFE: `sd1cli img-to-hfe my_sounds.img my_sounds.hfe`
3. Load `my_sounds.hfe` into the SD-1 plugin.
4. After saving files from the plugin, convert back: `sd1cli hfe-to-img my_sounds.hfe my_sounds.img`
5. Use `sd1cli list` to verify the saved files appear correctly.

Do not use `.img` files as the live working copy inside the 0.9.7 or earlier SD-1 VST. Always use HFE
as the interchange format between `sd1diskutil` and the plugin for that version or earlier!

Once the plugin's MAME disk handling is corrected, we'll remove the HFE wrapping process in these instructions as it will no longer
be necessary to use HFE format to safely save files in the emulator.

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
- **Sojus-corrupted `.img` repair:** `.img` files already damaged by the Sojus
  MAME bug cannot be repaired — sector 0 data is unrecoverable. Use HFE going
  forward.

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
