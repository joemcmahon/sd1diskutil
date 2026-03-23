# sd1diskutil Design Spec
**Date:** 2026-03-22
**Status:** Approved

## Overview

A Rust library (`sd1disk`) and thin CLI binary (`sd1cli`) for manipulating Ensoniq SD-1 floppy disk images. Converts MIDI SysEx dumps to/from the SD-1 binary disk format, and provides full disk management (list, inspect, write, extract, delete, create).

The library is designed for future Swift bridging via Mozilla UniFFI.

---

## Background & Format Summary

### SD-1 Disk Format (from Giebler / Ensoniq docs)
- **Geometry:** 80 tracks × 2 heads × 10 sectors × 512 bytes = 1,600 blocks = 819,200 bytes
- **Block formula:** `Block = ((Track × 2) + Head) × 10 + Sector`
- **Byte order:** Big-endian (68000-family CPU). All multi-byte fields — OS block free count (4 bytes), `size_blocks` (2 bytes), `contiguous_blocks` (2 bytes), `first_block` (4 bytes), `size_bytes` (3 bytes) — are stored big-endian. Use `u16::from_be_bytes()`, `u32::from_be_bytes()`, etc.
- **Reserved blocks:**
  - Block 0: Unused (repeating `6D B6` pattern)
  - Block 1: Device ID block
  - Block 2: OS block — bytes 0–3 = free block count (big-endian u32); bytes 28–29 = `4F 53` ("OS")
  - Blocks 3–4: Main Directory (points to 4 sub-directories)
  - Blocks 5–14: File Allocation Table (10 blocks, 170 three-byte entries each)
  - Blocks 15–16: Sub-Directory 1
  - Blocks 17–18: Sub-Directory 2
  - Blocks 19–20: Sub-Directory 3
  - Blocks 21–22: Sub-Directory 4
  - Blocks 23–1599: File data

### File Allocation Table
Each FAT entry is 3 bytes. The raw 24-bit value (interpreted as big-endian) determines meaning regardless of the entry's index in the FAT:
- `0x000000` = Free block
- `0x000001` = End of file (last block in chain)
- `0x000002` = Bad block
- Any other value = next block number in chain

Entries 0–22 in the FAT (covering reserved blocks) are always set to `0x000001` on a freshly formatted disk and must never be allocated. `allocate()` must skip block numbers 0–22.

### Directory Structure
- 4 sub-directories × 39 entries = 156 files maximum
- Each directory entry: 26 bytes

| Bytes | Field | Notes |
|-------|-------|-------|
| 1 | `type_info` | Type-dependent (bank/slot info) |
| 2 | `file_type` | See file type table |
| 3–13 | `name` | 11 bytes, null-padded, raw bytes (not guaranteed UTF-8) |
| 14 | `_reserved` | Always zero; must be written as zero, must not be used |
| 15–16 | `size_blocks` | Big-endian u16 |
| 17–18 | `contiguous_blocks` | Big-endian u16 |
| 19–22 | `first_block` | Big-endian u32 |
| 23 | `file_number` | 0–59 per file type |
| 24–26 | `size_bytes` | 24-bit big-endian (3 bytes) |

### SD-1 File Types
| Decimal | Hex  | Description             |
|---------|------|-------------------------|
| 10      | 0x0A | 1 Program               |
| 11      | 0x0B | 6 Programs              |
| 12      | 0x0C | 30 Programs             |
| 13      | 0x0D | 60 Programs             |
| 14      | 0x0E | 1 Preset                |
| 15      | 0x0F | 10 Presets              |
| 16      | 0x10 | 20 Presets              |
| 17      | 0x11 | 1 Sequence/Song         |
| 18      | 0x12 | 30 Sequences/Songs      |
| 19      | 0x13 | 60 Sequences/Songs      |
| 20      | 0x14 | System Exclusive        |
| 21      | 0x15 | System Setup            |
| 22      | 0x16 | Sequencer OS            |

An unknown `file_type` byte in a directory entry returns `Error::InvalidFileType(u8)`.

### SysEx Format (from SD-1 MIDI SysEx Spec v3.11)
- **Header:** `F0 0F 05 00 [midi_chan] [msg_type]`
- **Encoding:** Each 8-bit byte nybblized → two 4-bit MIDI bytes (`0000HHHH`, `0000LLLL`)
- **Tail:** `F7`
- **Internal data sizes:**
  - One Program: 530 bytes (1060 MIDI bytes)
  - All Programs (60): 31,800 bytes
  - One Preset: 48 bytes (96 MIDI bytes)
  - All Presets (20): 960 bytes
  - Sequences: variable

---

## Architecture

### Cargo Workspace
```
sd1diskutil/
  Cargo.toml                      ← workspace manifest
  blank_image.img                 ← embedded as template
  disk_with_everything.img        ← reference/test image
  SD1-SYSEX.pdf                   ← reference docs
  crates/
    sd1disk/                      ← library crate
      src/
        lib.rs
        error.rs
        image.rs
        fat.rs
        directory.rs
        sysex.rs
        types.rs
    sd1cli/                       ← binary crate
      src/
        main.rs
```

---

## Domain Types

### `DiskImage` (image.rs)
Owns the raw 819,200-byte image in memory. All mutations go through `save()` for atomic writes.

```rust
pub struct DiskImage { data: Vec<u8> }

impl DiskImage {
    pub fn open(path: &Path) -> Result<Self>;
    pub fn create() -> Self;                        // clones embedded blank_image.img
    pub fn save(&self, path: &Path) -> Result<()>;  // write to temp file, then rename
    pub fn block(&self, n: u16) -> Result<&[u8]>;         // Err if n >= 1600
    pub fn block_mut(&mut self, n: u16) -> Result<&mut [u8]>; // Err if n >= 1600
    pub fn free_blocks(&self) -> u32;               // OS block bytes 0–3, big-endian
    pub fn set_free_blocks(&mut self, count: u32);  // updates OS block bytes 0–3
}
```

`DiskImage::create()` uses `include_bytes!("../../../blank_image.img")` — embedding a known-good formatted image eliminates the risk of mis-implementing the header format.

`block()` / `block_mut()` return `Err(Error::BlockOutOfRange(n))` for `n >= 1600`. Callers must not assume infallibility.

### `FileAllocationTable` (fat.rs)
Operations on the FAT require a `&mut DiskImage` to write back changes. `FileAllocationTable` is constructed as an owned helper, not a borrowing view, to avoid conflicts with simultaneous `SubDirectory` mutations.

```rust
pub enum FatEntry { Free, EndOfFile, BadBlock, Next(u16) }

pub struct FileAllocationTable;  // stateless; all methods take &mut DiskImage

impl FileAllocationTable {
    pub fn entry(image: &DiskImage, block: u16) -> FatEntry;
    pub fn chain(image: &DiskImage, start: u16) -> Result<Vec<u16>>;
    // Returns Err(Error::CorruptFat) if:
    //   - a cycle is detected (block number already seen in traversal), or
    //   - a reserved block number (0–22) appears mid-chain.
    // Returns Err(Error::BadBlockInChain(n)) if:
    //   - block n is marked 0x000002 (bad) during traversal.

    pub fn allocate(image: &mut DiskImage, n: u16) -> Result<Vec<u16>>;
    // Scans blocks 23–1599 for a contiguous run of n free blocks first.
    // Falls back to a scattered list if no run is found.
    // Does NOT update the OS block free count (caller's responsibility).
    // Err(Error::DiskFull) if fewer than n free blocks exist.

    pub fn free_chain(image: &mut DiskImage, start: u16);
    // Marks all blocks in the chain as Free.
    // Does NOT update the OS block free count (caller's responsibility).

    pub fn set_chain(image: &mut DiskImage, blocks: &[u16]);
    // Links blocks[0]→blocks[1]→…→blocks[n-1], marking the last as EndOfFile.
    // Replaces calling set_next() in a loop.

    pub fn set_next(image: &mut DiskImage, block: u16, next: FatEntry);
}
```

**OS block free count:** `allocate()` and `free_chain()` do **not** update the free block count in the OS block. The caller (i.e., the WRITE and DELETE operations) is responsible for calling `image.set_free_blocks()` after mutating the FAT. This is documented as a required call-site obligation. Keeping the concern at the operation level (not the FAT level) simplifies the FAT API and makes the obligation visible at the point of use.

### `SubDirectory` / `DirectoryEntry` (directory.rs)

```rust
pub enum FileType {
    OneProgram, SixPrograms, ThirtyPrograms, SixtyPrograms,
    OnePreset, TenPresets, TwentyPresets,
    OneSequence, ThirtySequences, SixtySequences,
    SystemExclusive, SystemSetup, SequencerOs,
}

pub struct DirectoryEntry {
    pub type_info:         u8,
    pub file_type:         FileType,
    pub name:              [u8; 11],    // raw bytes, null-padded; not UTF-8 guaranteed
    pub _reserved:         u8,         // always zero
    pub size_blocks:       u16,
    pub contiguous_blocks: u16,
    pub first_block:       u32,
    pub file_number:       u8,
    pub size_bytes:        u32,        // stored as 24-bit big-endian on disk
}

impl DirectoryEntry {
    pub fn name_str(&self) -> Cow<'_, str>;  // uses from_utf8_lossy(); never panics
}

// SubDirectory operates directly on DiskImage to avoid borrow conflicts with FAT ops.
pub struct SubDirectory { index: u8 }  // index 0..3

impl SubDirectory {
    pub fn new(index: u8) -> Self;
    pub fn entries(&self, image: &DiskImage) -> Vec<DirectoryEntry>;
    pub fn find(&self, image: &DiskImage, name: &str) -> Option<DirectoryEntry>;
    // Name matching is case-sensitive exact match on the raw 11-byte name field.
    pub fn add(&self, image: &mut DiskImage, entry: DirectoryEntry) -> Result<()>;
    // Validates that entry.name is ≤ 11 bytes; returns Err(Error::InvalidName)
    // if not. Name validation is the library's responsibility, not the CLI's,
    // so it is enforced here regardless of call site (CLI, UniFFI, or direct).
    pub fn remove(&self, image: &mut DiskImage, name: &str) -> Result<()>;
    pub fn free_slots(&self, image: &DiskImage) -> usize;
}
```

### `SysExPacket` (sysex.rs)
The only place nybble-decoding and encoding occur.

```rust
pub enum MessageType {
    OneProgram, AllPrograms,
    OnePreset, AllPresets,
    SingleSequence, AllSequences,
    TrackParameters,
    // error/command types for completeness
}

pub struct SysExPacket {
    pub message_type: MessageType,
    pub midi_channel: u8,
    pub payload: Vec<u8>,   // de-nybblized internal data
}

impl SysExPacket {
    pub fn parse(bytes: &[u8]) -> Result<Self>;         // validate + de-nybblize
    pub fn to_bytes(&self, channel: u8) -> Vec<u8>;     // re-nybblize + frame
}
```

### `Program`, `Preset`, `Sequence` (types.rs)

```rust
pub struct Program([u8; 530]);
pub struct Preset([u8; 48]);
pub struct Sequence(Vec<u8>);

impl Program {
    pub fn from_sysex(packet: &SysExPacket) -> Result<Self>;
    pub fn to_bytes(&self) -> &[u8];
    pub fn name(&self) -> Cow<'_, str>;   // bytes 498–508 via from_utf8_lossy()
    pub fn to_sysex(&self, channel: u8) -> Vec<u8>;
    pub fn file_type(&self) -> FileType;  // always FileType::OneProgram
}
// Preset and Sequence follow the same pattern.
// Preset::name() reads from the appropriate offset in the preset structure.
```

---

## Operations (Type Transformations)

### LIST
```
DiskImage::open(path)
  → SubDirectory::new(0..3).entries(image)
  → Vec<DirectoryEntry>   (printed as table: name, type, size, blocks, slot)
```

### INSPECT
```
DiskImage::open(path)
  → image.free_blocks()                  (OS block bytes 0–3)
  → FileAllocationTable::entry(image, 0..1599)   (count free/used/bad)
  → image.block(1)                       (Device ID — disk label if present)
```

### WRITE
```
read .syx → &[u8]
  → SysExPacket::parse()                    de-nybblize, validate header
  → Program | Preset | Sequence             validate payload size matches type
  → determine FileType                      from MessageType
  → resolve name: --name flag ?? Program::name()  (CLI --name takes precedence)
  → resolve dir:  --dir flag ?? first SubDirectory where free_slots() > 0
  → if file exists and --overwrite:
      let blocks = FileAllocationTable::chain(image, entry.first_block)?
      FileAllocationTable::free_chain(image, entry.first_block)
      SubDirectory::remove(image, name)
      image.set_free_blocks(image.free_blocks() + blocks.len() as u32)
  → else if file exists: Err(Error::FileExists(name))
  → FileAllocationTable::allocate(image, n_blocks)   prefer contiguous
  → write data into allocated blocks        via image.block_mut()
  → FileAllocationTable::set_chain(image, &blocks)   link + mark end
  → build DirectoryEntry
  → SubDirectory::add(image, entry)
  → image.set_free_blocks(image.free_blocks() - n_blocks)
  → DiskImage::save(path)                   atomic write
```

### EXTRACT
```
DiskImage::open(path)
  → search SubDirectory::new(0..3) for name  (case-sensitive)
  → FileAllocationTable::chain(image, entry.first_block)  → Result<Vec<u16>>
  → read blocks via image.block()            → Vec<u8>
  → Program | Preset | Sequence              constructed from raw bytes
  → .to_sysex(channel)                      re-nybblize + frame
  → write to --out path, or default: "<name>.syx" in current working directory
```

### DELETE
```
DiskImage::open(path)
  → search SubDirectory::new(0..3) for name
  → Err(Error::FileNotFound) if not found
  → freed = chain length
  → FileAllocationTable::free_chain(image, entry.first_block)
  → SubDirectory::remove(image, name)
  → image.set_free_blocks(image.free_blocks() + freed)
  → DiskImage::save(path)
```

### CREATE
```
DiskImage::create()   // copies embedded blank_image.img via include_bytes!
  → DiskImage::save(path)
```

---

## Error Handling

```rust
#[derive(Debug)]
pub enum Error {
    InvalidImage(&'static str),
    InvalidSysEx(&'static str),
    WrongMessageType { expected: MessageType, got: MessageType },
    FileNotFound(String),
    FileExists(String),          // use --overwrite to replace
    DiskFull { needed: u16, available: u16 },
    DirectoryFull,               // all 4 × 39 slots occupied
    BlockOutOfRange(u16),        // block number >= 1600
    InvalidFileType(u8),         // unknown file type byte in directory entry
    CorruptFat,                  // FAT chain contains a cycle or hits a reserved block
    BadBlockInChain(u16),        // block n is marked bad mid-chain during read
    InvalidName(String),         // name > 11 bytes or otherwise unrepresentable
    Io(std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`Error` implements `std::error::Error` and `std::fmt::Display`. This is required for UniFFI error bridging to Swift.

---

## CLI Interface

Built with `clap`. Exits non-zero on any error (scriptable).

```
sd1disk list    <image.img>
sd1disk inspect <image.img>
sd1disk write   <image.img> <file.syx> [--name NAME] [--dir 1-4] [--overwrite]
sd1disk extract <image.img> <name>     [--out file.syx] [--channel N]
sd1disk delete  <image.img> <name>
sd1disk create  <image.img>
```

**Flag semantics:**
- `--name NAME`: overrides the file name derived from the SysEx payload or input filename. Must be ≤ 11 bytes; `Error::InvalidName` otherwise.
- `--dir 1-4`: places the file in the specified sub-directory rather than the first with a free slot.
- `--overwrite`: if a file with the same name exists, delete it and replace it.
- `--out file.syx`: output path for `extract`. Defaults to `<name>.syx` in the current working directory.
- `--channel N`: MIDI channel (0–15) to embed in the re-nybblized SysEx header. Defaults to 0.

---

## Key Invariants

1. **Nybble boundary:** `SysExPacket::parse` is the only decode site. `SysExPacket::to_bytes` / `to_sysex` is the only encode site. All intermediate code works with plain bytes.
2. **Atomic writes:** `DiskImage::save` writes to a temp file and renames. Partial writes cannot corrupt a disk image.
3. **Contiguous preference:** `FAT::allocate` always prefers a contiguous block run, matching SD-1 hardware write behaviour and improving playback performance.
4. **Known-good blank:** `DiskImage::create()` copies the embedded `blank_image.img` rather than constructing a blank from spec. Correctness is verified by hardware, not by our implementation.
5. **No leaky abstractions:** The CLI has no disk logic. The library has no CLI concerns. Types enforce the transformation pipeline at compile time.
6. **No dual borrows:** `FileAllocationTable` and `SubDirectory` are stateless handles that take `&DiskImage` or `&mut DiskImage` as parameters. This avoids Rust borrow-checker conflicts when FAT mutations and directory mutations are interleaved within one operation.
7. **Big-endian throughout:** All multi-byte disk fields are big-endian. All serialization/deserialization uses explicit `from_be_bytes()` / `to_be_bytes()`.
8. **OS block consistency:** `allocate()` and `free_chain()` do not update the OS block free count. Each operation (WRITE, DELETE) is responsible for calling `image.set_free_blocks()` after FAT mutations. This is a documented call-site obligation.
9. **Name matching is case-sensitive:** `SubDirectory::find()` performs exact case-sensitive matching on the raw 11-byte name field.

---

## Future: Swift / UniFFI Bridge

`sd1disk` is designed as a clean UniFFI surface. `DiskImage`, `Program`, `Preset`, `Sequence`, and `Error` are lifetime-free and map naturally to Swift types. `FileAllocationTable` and `SubDirectory` are internal stateless handles and are not part of the UniFFI surface.

`Error` implements `std::error::Error` + `Display` as required for UniFFI error bridging. The `Io` variant wraps `std::io::Error` and should be flattened to a string in the UniFFI layer.
