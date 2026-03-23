# sd1diskutil Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust library (`sd1disk`) and CLI (`sd1cli`) that can inspect, read, write, extract, and delete files on Ensoniq SD-1 floppy disk images, converting to/from MIDI SysEx format.

**Architecture:** Cargo workspace with two crates. `sd1disk` is a pure library with no I/O dependencies beyond `std`; all disk mutation goes through `DiskImage::save()` for atomic writes. `sd1cli` is a thin `clap`-based binary that calls into the library. `FileAllocationTable` and `SubDirectory` are stateless handles that take `&DiskImage` or `&mut DiskImage` to avoid Rust borrow conflicts.

**Tech Stack:** Rust (stable, 2021 edition), `clap` v4 (derive feature) for CLI only, no other external dependencies.

---

## Reference: Key Constants

```
Disk geometry:   80 tracks × 2 heads × 10 sectors × 512 bytes = 1,600 blocks
Block formula:   Block = ((Track × 2) + Head) × 10 + Sector
Byte order:      Big-endian for all multi-byte disk fields

Reserved blocks:
  Block 0:   Unused (6D B6 repeating)
  Block 1:   Device ID
  Block 2:   OS block — bytes 0–3: free block count (big-endian u32); bytes 28–29: "OS"
  Blocks 3–4:  Main Directory
  Blocks 5–14: FAT (10 blocks × 170 entries × 3 bytes = 1700 entries)
  Blocks 15–16: SubDir 0   (bytes 7680–8191, 8192–8703)
  Blocks 17–18: SubDir 1
  Blocks 19–20: SubDir 2
  Blocks 21–22: SubDir 3
  Blocks 23–1599: File data

FAT entry encoding (raw 24-bit big-endian value):
  0x000000 = Free
  0x000001 = End of file
  0x000002 = Bad block
  other    = Next block number

FAT byte offset for block N:
  fat_block_index = N / 170          (0–9; maps to disk blocks 5–14)
  offset_in_block = (N % 170) * 3
  byte_in_image   = (5 + fat_block_index) * 512 + offset_in_block

Directory entry (26 bytes, 0-indexed):
  [0]     type_info
  [1]     file_type
  [2–12]  name (11 bytes, null-padded, raw bytes)
  [13]    _reserved (always zero)
  [14–15] size_blocks (big-endian u16)
  [16–17] contiguous_blocks (big-endian u16)
  [18–21] first_block (big-endian u32)
  [22]    file_number
  [23–25] size_bytes (24-bit big-endian)

SubDir N byte offset in image: (15 + N * 2) * 512
Each SubDir holds 39 entries (39 × 26 = 1014 bytes, fits in 1024)

SysEx header:  F0 0F 05 00 [chan] [msg_type]
SysEx tail:    F7
Nybblization:  each 8-bit byte → 0000HHHH 0000LLLL (hi nybble first)
Message types: 02=OneProgram 03=AllPrograms 04=OnePreset 05=AllPresets
               09=SingleSequence 0A=AllSequences 0B=TrackParameters

Internal data sizes (de-nybblized):
  One Program:  530 bytes
  All Programs: 31,800 bytes (60 × 530)
  One Preset:   48 bytes
  All Presets:  960 bytes (20 × 48)
  Sequences:    variable
```

---

## File Map

```
sd1diskutil/
  Cargo.toml                                   ← workspace manifest
  crates/
    sd1disk/
      Cargo.toml
      src/
        lib.rs                                 ← re-exports; pub use all public types
        error.rs                               ← Error enum, Result<T>
        image.rs                               ← DiskImage struct + open/create/save/block
        fat.rs                                 ← FatEntry enum, FileAllocationTable
        directory.rs                           ← FileType, DirectoryEntry, SubDirectory
        sysex.rs                               ← MessageType, SysExPacket
        types.rs                               ← Program, Preset, Sequence
      tests/
        image_tests.rs                         ← DiskImage integration tests
        fat_tests.rs                           ← FAT round-trip tests
        directory_tests.rs                     ← directory parse/write tests
        sysex_tests.rs                         ← nybblize/denybblize tests
        types_tests.rs                         ← Program/Preset from real SysEx
        operations_tests.rs                    ← end-to-end list/write/extract/delete
    sd1cli/
      Cargo.toml
      src/
        main.rs                                ← clap App + subcommand dispatch
```

---

## Task 1: Workspace Bootstrap

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/sd1disk/Cargo.toml`
- Create: `crates/sd1disk/src/lib.rs`
- Create: `crates/sd1cli/Cargo.toml`
- Create: `crates/sd1cli/src/main.rs`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
# Cargo.toml (workspace root)
[workspace]
members = ["crates/sd1disk", "crates/sd1cli"]
resolver = "2"
```

- [ ] **Step 2: Create sd1disk crate**

```toml
# crates/sd1disk/Cargo.toml
[package]
name = "sd1disk"
version = "0.1.0"
edition = "2021"

[dependencies]
```

```rust
// crates/sd1disk/src/lib.rs
// (empty for now — modules added as we go)
```

- [ ] **Step 3: Create sd1cli crate**

```toml
# crates/sd1cli/Cargo.toml
[package]
name = "sd1cli"
version = "0.1.0"
edition = "2021"

[dependencies]
sd1disk = { path = "../sd1disk" }
clap = { version = "4", features = ["derive"] }
```

```rust
// crates/sd1cli/src/main.rs
fn main() {
    println!("sd1disk");
}
```

- [ ] **Step 4: Verify workspace compiles**

```bash
cargo build
```

Expected: compiles cleanly with no warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/
git commit -m "feat: scaffold Cargo workspace with sd1disk and sd1cli crates"
```

---

## Task 2: Error Types

**Files:**
- Create: `crates/sd1disk/src/error.rs`
- Modify: `crates/sd1disk/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/sd1disk/src/error.rs (add at bottom, after type definitions)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_disk_full() {
        let e = Error::DiskFull { needed: 10, available: 3 };
        let s = format!("{}", e);
        assert!(s.contains("10"), "should mention needed blocks");
        assert!(s.contains("3"), "should mention available blocks");
    }

    #[test]
    fn error_is_std_error() {
        // Verifies Error implements std::error::Error (required for UniFFI)
        fn assert_std_error<E: std::error::Error>() {}
        assert_std_error::<Error>();
    }

    #[test]
    fn error_file_not_found_contains_name() {
        let e = Error::FileNotFound("MY_PATCH".to_string());
        let s = format!("{}", e);
        assert!(s.contains("MY_PATCH"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p sd1disk 2>&1 | head -20
```

Expected: compile error — `Error` not defined.

- [ ] **Step 3: Implement error.rs**

```rust
// crates/sd1disk/src/error.rs
use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Disk image is not a valid SD-1 image or is truncated
    InvalidImage(&'static str),
    /// SysEx packet has wrong header, truncated data, or bad structure
    InvalidSysEx(&'static str),
    /// SysEx message type was not what was expected
    WrongMessageType { expected: String, got: String },
    /// No file with this name exists in any sub-directory
    FileNotFound(String),
    /// A file with this name already exists; use --overwrite to replace
    FileExists(String),
    /// Disk does not have enough free blocks
    DiskFull { needed: u16, available: u16 },
    /// All 4 sub-directories × 39 slots are full
    DirectoryFull,
    /// Block number must be 0–1599
    BlockOutOfRange(u16),
    /// Unknown file type byte found in directory entry
    InvalidFileType(u8),
    /// FAT chain contains a cycle or visits a reserved block number
    CorruptFat,
    /// A bad-block marker (0x000002) was found mid-chain at this block
    BadBlockInChain(u16),
    /// Name exceeds 11 bytes or contains unrepresentable characters
    InvalidName(String),
    /// I/O error from std
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidImage(msg) => write!(f, "Invalid disk image: {}", msg),
            Error::InvalidSysEx(msg) => write!(f, "Invalid SysEx data: {}", msg),
            Error::WrongMessageType { expected, got } => {
                write!(f, "Wrong SysEx message type: expected {}, got {}", expected, got)
            }
            Error::FileNotFound(name) => write!(f, "File not found: {}", name),
            Error::FileExists(name) => {
                write!(f, "File already exists: {} (use --overwrite to replace)", name)
            }
            Error::DiskFull { needed, available } => {
                write!(f, "Disk full: need {} blocks, {} available", needed, available)
            }
            Error::DirectoryFull => write!(f, "Directory full: all 156 file slots are used"),
            Error::BlockOutOfRange(n) => write!(f, "Block {} is out of range (max 1599)", n),
            Error::InvalidFileType(b) => write!(f, "Unknown file type byte: 0x{:02X}", b),
            Error::CorruptFat => write!(f, "FAT is corrupt: cycle or illegal block reference detected"),
            Error::BadBlockInChain(n) => write!(f, "Bad block {} encountered in file chain", n),
            Error::InvalidName(name) => {
                write!(f, "Invalid file name '{}': must be 1–11 ASCII bytes", name)
            }
            Error::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_disk_full() {
        let e = Error::DiskFull { needed: 10, available: 3 };
        let s = format!("{}", e);
        assert!(s.contains("10"), "should mention needed blocks");
        assert!(s.contains("3"), "should mention available blocks");
    }

    #[test]
    fn error_is_std_error() {
        fn assert_std_error<E: std::error::Error>() {}
        assert_std_error::<Error>();
    }

    #[test]
    fn error_file_not_found_contains_name() {
        let e = Error::FileNotFound("MY_PATCH".to_string());
        let s = format!("{}", e);
        assert!(s.contains("MY_PATCH"));
    }
}
```

- [ ] **Step 4: Wire into lib.rs**

```rust
// crates/sd1disk/src/lib.rs
pub mod error;
pub use error::{Error, Result};
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sd1disk
```

Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sd1disk/src/error.rs crates/sd1disk/src/lib.rs
git commit -m "feat: add Error enum with Display and std::error::Error impls"
```

---

## Task 3: DiskImage

**Files:**
- Create: `crates/sd1disk/src/image.rs`
- Modify: `crates/sd1disk/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

```rust
// crates/sd1disk/src/image.rs — tests block at bottom
#[cfg(test)]
mod tests {
    use super::*;

    fn test_image() -> DiskImage {
        // Use the blank image embedded in the library
        DiskImage::create()
    }

    #[test]
    fn blank_image_is_correct_size() {
        let img = test_image();
        assert_eq!(img.data.len(), 1600 * 512);
    }

    #[test]
    fn block_zero_returns_512_bytes() {
        let img = test_image();
        let block = img.block(0).unwrap();
        assert_eq!(block.len(), 512);
    }

    #[test]
    fn block_out_of_range_returns_error() {
        let img = test_image();
        assert!(img.block(1600).is_err());
        assert!(img.block(u16::MAX).is_err());
    }

    #[test]
    fn free_blocks_is_reasonable_for_blank_disk() {
        let img = test_image();
        // Blank disk: 1600 total - 23 reserved = 1577 usable
        let free = img.free_blocks();
        assert!(free > 0 && free <= 1577, "free blocks should be 0–1577, got {}", free);
    }

    #[test]
    fn set_free_blocks_round_trips() {
        let mut img = test_image();
        img.set_free_blocks(42);
        assert_eq!(img.free_blocks(), 42);
    }

    #[test]
    fn save_and_reload_round_trips() {
        let mut img = test_image();
        img.set_free_blocks(999);
        let path = std::env::temp_dir().join("sd1_test_roundtrip.img");
        img.save(&path).unwrap();
        let loaded = DiskImage::open(&path).unwrap();
        assert_eq!(loaded.free_blocks(), 999);
        std::fs::remove_file(&path).ok();
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p sd1disk image 2>&1 | head -10
```

Expected: compile error — `DiskImage` not defined.

- [ ] **Step 3: Implement image.rs**

```rust
// crates/sd1disk/src/image.rs
use std::path::Path;
use crate::{Error, Result};

// The blank image is embedded at compile time.
// Path is relative to this source file: 3 levels up to workspace root.
static BLANK_IMAGE: &[u8] = include_bytes!("../../../blank_image.img");

const BLOCK_SIZE: usize = 512;
const BLOCK_COUNT: usize = 1600;
const TOTAL_SIZE: usize = BLOCK_SIZE * BLOCK_COUNT;  // 819,200

// OS block (block 2) byte offsets
const OS_BLOCK_START: usize = 2 * BLOCK_SIZE;        // byte 1024
const OS_FREE_COUNT_OFFSET: usize = OS_BLOCK_START;  // bytes 1024–1027

pub struct DiskImage {
    pub(crate) data: Vec<u8>,
}

impl DiskImage {
    /// Load an existing disk image from a file.
    pub fn open(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)?;
        if data.len() != TOTAL_SIZE {
            return Err(Error::InvalidImage("image must be exactly 819,200 bytes"));
        }
        Ok(Self { data })
    }

    /// Create a blank formatted disk image from the embedded template.
    pub fn create() -> Self {
        assert_eq!(
            BLANK_IMAGE.len(), TOTAL_SIZE,
            "blank_image.img must be 819,200 bytes; found {}",
            BLANK_IMAGE.len()
        );
        Self { data: BLANK_IMAGE.to_vec() }
    }

    /// Save the disk image atomically (write to temp file, then rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("img.tmp");
        std::fs::write(&tmp, &self.data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Return a 512-byte slice for block n. Error if n >= 1600.
    pub fn block(&self, n: u16) -> Result<&[u8]> {
        if n as usize >= BLOCK_COUNT {
            return Err(Error::BlockOutOfRange(n));
        }
        let start = n as usize * BLOCK_SIZE;
        Ok(&self.data[start..start + BLOCK_SIZE])
    }

    /// Return a mutable 512-byte slice for block n. Error if n >= 1600.
    pub fn block_mut(&mut self, n: u16) -> Result<&mut [u8]> {
        if n as usize >= BLOCK_COUNT {
            return Err(Error::BlockOutOfRange(n));
        }
        let start = n as usize * BLOCK_SIZE;
        Ok(&mut self.data[start..start + BLOCK_SIZE])
    }

    /// Read the free block count from the OS block (big-endian u32).
    pub fn free_blocks(&self) -> u32 {
        let bytes = &self.data[OS_FREE_COUNT_OFFSET..OS_FREE_COUNT_OFFSET + 4];
        u32::from_be_bytes(bytes.try_into().unwrap())
    }

    /// Write the free block count to the OS block.
    pub fn set_free_blocks(&mut self, count: u32) {
        let bytes = count.to_be_bytes();
        self.data[OS_FREE_COUNT_OFFSET..OS_FREE_COUNT_OFFSET + 4].copy_from_slice(&bytes);
    }
}
```

- [ ] **Step 4: Add module to lib.rs**

```rust
// crates/sd1disk/src/lib.rs
pub mod error;
pub use error::{Error, Result};

pub mod image;
pub use image::DiskImage;
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sd1disk image
```

Expected: all 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sd1disk/src/image.rs crates/sd1disk/src/lib.rs
git commit -m "feat: add DiskImage with open/create/save/block access and OS block free count"
```

---

## Task 4: File Allocation Table

**Files:**
- Create: `crates/sd1disk/src/fat.rs`
- Modify: `crates/sd1disk/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

```rust
// In crates/sd1disk/src/fat.rs, at bottom:
#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiskImage;

    fn blank() -> DiskImage { DiskImage::create() }

    #[test]
    fn reserved_blocks_are_end_of_file() {
        // Blocks 0–22 on a blank SD-1 disk are marked EndOfFile (0x000001)
        let img = blank();
        for n in 0u16..23 {
            assert_eq!(
                FileAllocationTable::entry(&img, n),
                FatEntry::EndOfFile,
                "block {} should be EndOfFile on blank disk", n
            );
        }
    }

    #[test]
    fn data_blocks_are_free_on_blank_disk() {
        let img = blank();
        // Spot-check a few data blocks
        for n in [23u16, 50, 100, 500, 1599] {
            assert_eq!(
                FileAllocationTable::entry(&img, n),
                FatEntry::Free,
                "block {} should be Free on blank disk", n
            );
        }
    }

    #[test]
    fn allocate_contiguous_prefers_run() {
        let mut img = blank();
        let blocks = FileAllocationTable::allocate(&mut img, 5).unwrap();
        assert_eq!(blocks.len(), 5);
        // Should be contiguous (consecutive block numbers)
        for i in 1..blocks.len() {
            assert_eq!(blocks[i], blocks[i-1] + 1, "should be contiguous");
        }
        // All should be in data range
        for &b in &blocks {
            assert!(b >= 23 && b < 1600, "block {} out of data range", b);
        }
    }

    #[test]
    fn set_chain_and_follow() {
        let mut img = blank();
        let blocks = vec![23u16, 24, 25];
        FileAllocationTable::set_chain(&mut img, &blocks);
        // Entry 23 → Next(24), 24 → Next(25), 25 → EndOfFile
        assert_eq!(FileAllocationTable::entry(&img, 23), FatEntry::Next(24));
        assert_eq!(FileAllocationTable::entry(&img, 24), FatEntry::Next(25));
        assert_eq!(FileAllocationTable::entry(&img, 25), FatEntry::EndOfFile);
    }

    #[test]
    fn chain_iterator_follows_links() {
        let mut img = blank();
        FileAllocationTable::set_chain(&mut img, &[23, 24, 25]);
        let chain = FileAllocationTable::chain(&img, 23).unwrap();
        assert_eq!(chain, vec![23u16, 24, 25]);
    }

    #[test]
    fn free_chain_marks_blocks_free() {
        let mut img = blank();
        FileAllocationTable::set_chain(&mut img, &[23, 24, 25]);
        FileAllocationTable::free_chain(&mut img, 23);
        assert_eq!(FileAllocationTable::entry(&img, 23), FatEntry::Free);
        assert_eq!(FileAllocationTable::entry(&img, 24), FatEntry::Free);
        assert_eq!(FileAllocationTable::entry(&img, 25), FatEntry::Free);
    }

    #[test]
    fn allocate_returns_disk_full_when_no_space() {
        let mut img = blank();
        // Fill all data blocks
        let all: Vec<u16> = (23..1600).collect();
        FileAllocationTable::set_chain(&mut img, &all);
        let result = FileAllocationTable::allocate(&mut img, 1);
        assert!(matches!(result, Err(crate::Error::DiskFull { .. })));
    }

    #[test]
    fn corrupt_fat_cycle_detected() {
        let mut img = blank();
        // Create a cycle: 23 → 24 → 23
        FileAllocationTable::set_next(&mut img, 23, FatEntry::Next(24));
        FileAllocationTable::set_next(&mut img, 24, FatEntry::Next(23));
        let result = FileAllocationTable::chain(&img, 23);
        assert!(matches!(result, Err(crate::Error::CorruptFat)));
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p sd1disk fat 2>&1 | head -10
```

Expected: compile error.

- [ ] **Step 3: Implement fat.rs**

```rust
// crates/sd1disk/src/fat.rs
use crate::{DiskImage, Error, Result};
use std::collections::HashSet;

// FAT starts at block 5. Each block holds 170 entries × 3 bytes = 510 bytes.
// The remaining 2 bytes in each FAT block are unused/padding.
const FAT_START_BLOCK: usize = 5;
const ENTRIES_PER_FAT_BLOCK: usize = 170;
const ENTRY_SIZE: usize = 3;
const BLOCK_SIZE: usize = 512;
const FIRST_DATA_BLOCK: u16 = 23;
const TOTAL_BLOCKS: u16 = 1600;

#[derive(Debug, PartialEq, Eq)]
pub enum FatEntry {
    Free,
    EndOfFile,
    BadBlock,
    Next(u16),
}

pub struct FileAllocationTable;

impl FileAllocationTable {
    fn entry_byte_offset(block: u16) -> usize {
        let idx = block as usize;
        let fat_block = FAT_START_BLOCK + (idx / ENTRIES_PER_FAT_BLOCK);
        let offset_in_block = (idx % ENTRIES_PER_FAT_BLOCK) * ENTRY_SIZE;
        fat_block * BLOCK_SIZE + offset_in_block
    }

    fn read_raw(image: &DiskImage, block: u16) -> u32 {
        let off = Self::entry_byte_offset(block);
        let b = &image.data[off..off + 3];
        u32::from_be_bytes([0, b[0], b[1], b[2]])
    }

    fn write_raw(image: &mut DiskImage, block: u16, value: u32) {
        let off = Self::entry_byte_offset(block);
        let bytes = value.to_be_bytes();
        image.data[off] = bytes[1];
        image.data[off + 1] = bytes[2];
        image.data[off + 2] = bytes[3];
    }

    /// Read the FAT entry for a given block number.
    pub fn entry(image: &DiskImage, block: u16) -> FatEntry {
        match Self::read_raw(image, block) {
            0x000000 => FatEntry::Free,
            0x000001 => FatEntry::EndOfFile,
            0x000002 => FatEntry::BadBlock,
            n => FatEntry::Next(n as u16),
        }
    }

    /// Follow a FAT chain starting at `start`, returning the ordered list of block numbers.
    ///
    /// Returns Err(Error::CorruptFat) if:
    ///   - a cycle is detected (block number already seen), or
    ///   - a reserved block number (0–22) appears mid-chain.
    /// Returns Err(Error::BadBlockInChain(n)) if block n is marked bad during traversal.
    pub fn chain(image: &DiskImage, start: u16) -> Result<Vec<u16>> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut current = start;
        loop {
            if seen.contains(&current) {
                return Err(Error::CorruptFat);
            }
            seen.insert(current);
            result.push(current);
            match Self::entry(image, current) {
                FatEntry::EndOfFile => break,
                FatEntry::Free => return Err(Error::CorruptFat),
                FatEntry::BadBlock => return Err(Error::BadBlockInChain(current)),
                FatEntry::Next(next) => {
                    if next < FIRST_DATA_BLOCK {
                        return Err(Error::CorruptFat);
                    }
                    current = next;
                }
            }
        }
        Ok(result)
    }

    /// Find `n` free blocks to allocate. Prefers a contiguous run; falls back to scattered.
    /// Does NOT update the OS block free count — caller must call image.set_free_blocks().
    /// Returns Err(Error::DiskFull) if fewer than n free blocks exist.
    pub fn allocate(image: &mut DiskImage, n: u16) -> Result<Vec<u16>> {
        let free: Vec<u16> = (FIRST_DATA_BLOCK..TOTAL_BLOCKS)
            .filter(|&b| Self::entry(image, b) == FatEntry::Free)
            .collect();

        if free.len() < n as usize {
            return Err(Error::DiskFull {
                needed: n,
                available: free.len() as u16,
            });
        }

        // Try to find a contiguous run of n blocks
        let mut run_start = 0;
        let mut run_len = 1usize;
        for i in 1..free.len() {
            if free[i] == free[i - 1] + 1 {
                run_len += 1;
                if run_len >= n as usize {
                    return Ok(free[run_start..run_start + n as usize].to_vec());
                }
            } else {
                run_start = i;
                run_len = 1;
            }
        }

        // No contiguous run; return first n free blocks (scattered)
        Ok(free[..n as usize].to_vec())
    }

    /// Free all blocks in the chain starting at `start`.
    /// Does NOT update the OS block free count — caller must call image.set_free_blocks().
    pub fn free_chain(image: &mut DiskImage, start: u16) {
        // Collect chain first to avoid borrow issues during mutation
        let blocks = Self::chain(image, start).unwrap_or_else(|_| vec![start]);
        for b in blocks {
            Self::write_raw(image, b, 0x000000);
        }
    }

    /// Link `blocks` as a chain: blocks[0]→blocks[1]→…→blocks[n-1]=EndOfFile.
    pub fn set_chain(image: &mut DiskImage, blocks: &[u16]) {
        for (i, &block) in blocks.iter().enumerate() {
            let raw = if i + 1 < blocks.len() {
                blocks[i + 1] as u32
            } else {
                0x000001  // EndOfFile
            };
            Self::write_raw(image, block, raw);
        }
    }

    /// Set a single FAT entry.
    pub fn set_next(image: &mut DiskImage, block: u16, next: FatEntry) {
        let raw = match next {
            FatEntry::Free => 0x000000,
            FatEntry::EndOfFile => 0x000001,
            FatEntry::BadBlock => 0x000002,
            FatEntry::Next(n) => n as u32,
        };
        Self::write_raw(image, block, raw);
    }
}

- [ ] **Step 4: Add module to lib.rs**

```rust
// Add to crates/sd1disk/src/lib.rs:
pub mod fat;
pub use fat::{FatEntry, FileAllocationTable};
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sd1disk fat
```

Expected: all 8 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sd1disk/src/fat.rs crates/sd1disk/src/lib.rs
git commit -m "feat: add FileAllocationTable with chain follow, contiguous allocation, and free"
```

---

## Task 5: Directory

**Files:**
- Create: `crates/sd1disk/src/directory.rs`
- Modify: `crates/sd1disk/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

```rust
// In crates/sd1disk/src/directory.rs, at bottom:
#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiskImage;

    fn blank() -> DiskImage { DiskImage::create() }

    #[test]
    fn blank_disk_has_no_entries() {
        let img = blank();
        for dir_idx in 0..4u8 {
            let dir = SubDirectory::new(dir_idx);
            let entries = dir.entries(&img);
            assert!(entries.is_empty(), "sub-dir {} should be empty on blank disk", dir_idx);
        }
    }

    #[test]
    fn blank_disk_has_39_free_slots_per_dir() {
        let img = blank();
        for dir_idx in 0..4u8 {
            let dir = SubDirectory::new(dir_idx);
            assert_eq!(dir.free_slots(&img), 39);
        }
    }

    #[test]
    fn add_entry_then_find_it() {
        let mut img = blank();
        let dir = SubDirectory::new(0);
        let entry = DirectoryEntry {
            type_info: 0,
            file_type: FileType::OneProgram,
            name: *b"MY_PATCH   ",
            _reserved: 0,
            size_blocks: 2,
            contiguous_blocks: 2,
            first_block: 23,
            file_number: 0,
            size_bytes: 530,
        };
        dir.add(&mut img, entry).unwrap();
        let found = dir.find(&img, "MY_PATCH").unwrap();
        assert_eq!(found.file_type, FileType::OneProgram);
        assert_eq!(found.size_bytes, 530);
    }

    #[test]
    fn find_is_case_sensitive() {
        let mut img = blank();
        let dir = SubDirectory::new(0);
        let entry = DirectoryEntry {
            type_info: 0,
            file_type: FileType::OneProgram,
            name: *b"MY_PATCH   ",
            _reserved: 0,
            size_blocks: 2,
            contiguous_blocks: 2,
            first_block: 23,
            file_number: 0,
            size_bytes: 530,
        };
        dir.add(&mut img, entry).unwrap();
        assert!(dir.find(&img, "my_patch").is_none(), "matching should be case-sensitive");
        assert!(dir.find(&img, "MY_PATCH").is_some());
    }

    #[test]
    fn remove_entry() {
        let mut img = blank();
        let dir = SubDirectory::new(0);
        let entry = DirectoryEntry {
            type_info: 0,
            file_type: FileType::OneProgram,
            name: *b"TO_DELETE  ",
            _reserved: 0,
            size_blocks: 1,
            contiguous_blocks: 1,
            first_block: 23,
            file_number: 0,
            size_bytes: 512,
        };
        dir.add(&mut img, entry).unwrap();
        dir.remove(&mut img, "TO_DELETE").unwrap();
        assert!(dir.find(&img, "TO_DELETE").is_none());
        assert_eq!(dir.free_slots(&img), 39);
    }

    #[test]
    fn name_too_long_returns_error() {
        let mut img = blank();
        let dir = SubDirectory::new(0);
        let entry = DirectoryEntry {
            type_info: 0,
            file_type: FileType::OneProgram,
            name: *b"SHORT      ",
            _reserved: 0,
            size_blocks: 1,
            contiguous_blocks: 1,
            first_block: 23,
            file_number: 0,
            size_bytes: 530,
        };
        // Override name with 12-char value by creating an entry with an invalid name
        // via the validate path in add():
        // We test the validate_name utility directly:
        assert!(validate_name("12CharactersX").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("GOOD").is_ok());
        let _ = dir.add(&mut img, entry);
    }

    #[test]
    fn file_type_round_trips() {
        let types = [
            (0x0Au8, FileType::OneProgram),
            (0x0E, FileType::OnePreset),
            (0x11, FileType::OneSequence),
            (0x14, FileType::SystemExclusive),
        ];
        for (byte, expected) in types {
            let parsed = FileType::from_byte(byte).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.to_byte(), byte);
        }
    }

    #[test]
    fn unknown_file_type_returns_error() {
        assert!(matches!(
            FileType::from_byte(0xFF),
            Err(crate::Error::InvalidFileType(0xFF))
        ));
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p sd1disk directory 2>&1 | head -10
```

Expected: compile error.

- [ ] **Step 3: Implement directory.rs**

```rust
// crates/sd1disk/src/directory.rs
use std::borrow::Cow;
use crate::{DiskImage, Error, Result};

const BLOCK_SIZE: usize = 512;
const SUBDIR_ENTRY_SIZE: usize = 26;
const SUBDIR_CAPACITY: usize = 39;
const SUBDIR_START_BLOCK: u16 = 15;  // SubDir 0 starts at block 15

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileType {
    OneProgram,
    SixPrograms,
    ThirtyPrograms,
    SixtyPrograms,
    OnePreset,
    TenPresets,
    TwentyPresets,
    OneSequence,
    ThirtySequences,
    SixtySequences,
    SystemExclusive,
    SystemSetup,
    SequencerOs,
}

impl FileType {
    pub fn from_byte(b: u8) -> Result<Self> {
        Ok(match b {
            0x0A => FileType::OneProgram,
            0x0B => FileType::SixPrograms,
            0x0C => FileType::ThirtyPrograms,
            0x0D => FileType::SixtyPrograms,
            0x0E => FileType::OnePreset,
            0x0F => FileType::TenPresets,
            0x10 => FileType::TwentyPresets,
            0x11 => FileType::OneSequence,
            0x12 => FileType::ThirtySequences,
            0x13 => FileType::SixtySequences,
            0x14 => FileType::SystemExclusive,
            0x15 => FileType::SystemSetup,
            0x16 => FileType::SequencerOs,
            other => return Err(Error::InvalidFileType(other)),
        })
    }

    pub fn to_byte(&self) -> u8 {
        match self {
            FileType::OneProgram      => 0x0A,
            FileType::SixPrograms     => 0x0B,
            FileType::ThirtyPrograms  => 0x0C,
            FileType::SixtyPrograms   => 0x0D,
            FileType::OnePreset       => 0x0E,
            FileType::TenPresets      => 0x0F,
            FileType::TwentyPresets   => 0x10,
            FileType::OneSequence     => 0x11,
            FileType::ThirtySequences => 0x12,
            FileType::SixtySequences  => 0x13,
            FileType::SystemExclusive => 0x14,
            FileType::SystemSetup     => 0x15,
            FileType::SequencerOs     => 0x16,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    pub type_info:         u8,
    pub file_type:         FileType,
    pub name:              [u8; 11],
    pub _reserved:         u8,
    pub size_blocks:       u16,
    pub contiguous_blocks: u16,
    pub first_block:       u32,
    pub file_number:       u8,
    pub size_bytes:        u32,
}

impl DirectoryEntry {
    /// Return the file name as a string. Uses lossy UTF-8 conversion; never panics.
    /// Strips trailing null bytes and spaces.
    pub fn name_str(&self) -> Cow<'_, str> {
        let trimmed: Vec<u8> = self.name.iter()
            .copied()
            .take_while(|&b| b != 0)
            .collect();
        String::from_utf8_lossy(&trimmed).into_owned().into()
    }

    fn to_bytes(&self) -> [u8; SUBDIR_ENTRY_SIZE] {
        let mut buf = [0u8; SUBDIR_ENTRY_SIZE];
        buf[0] = self.type_info;
        buf[1] = self.file_type.to_byte();
        buf[2..13].copy_from_slice(&self.name);
        buf[13] = 0;  // _reserved always zero
        buf[14..16].copy_from_slice(&self.size_blocks.to_be_bytes());
        buf[16..18].copy_from_slice(&self.contiguous_blocks.to_be_bytes());
        buf[18..22].copy_from_slice(&self.first_block.to_be_bytes());
        buf[22] = self.file_number;
        // 24-bit big-endian size_bytes
        let sb = self.size_bytes.to_be_bytes();
        buf[23] = sb[1];
        buf[24] = sb[2];
        buf[25] = sb[3];
        buf
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes[1] == 0x00 {
            return None;  // Unused slot
        }
        let file_type = FileType::from_byte(bytes[1]).ok()?;
        let mut name = [0u8; 11];
        name.copy_from_slice(&bytes[2..13]);
        let size_blocks = u16::from_be_bytes([bytes[14], bytes[15]]);
        let contiguous_blocks = u16::from_be_bytes([bytes[16], bytes[17]]);
        let first_block = u32::from_be_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let size_bytes = u32::from_be_bytes([0, bytes[23], bytes[24], bytes[25]]);
        Some(DirectoryEntry {
            type_info: bytes[0],
            file_type,
            name,
            _reserved: 0,
            size_blocks,
            contiguous_blocks,
            first_block,
            file_number: bytes[22],
            size_bytes,
        })
    }
}

pub fn validate_name(name: &str) -> Result<[u8; 11]> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 11 {
        return Err(Error::InvalidName(name.to_string()));
    }
    let mut arr = [0u8; 11];
    arr[..bytes.len()].copy_from_slice(bytes);
    // Pad with spaces to match SD-1 convention
    for b in &mut arr[bytes.len()..] {
        *b = b' ';
    }
    Ok(arr)
}

/// A stateless handle to one of the four sub-directories (index 0–3).
/// All operations take a &DiskImage or &mut DiskImage to avoid borrow conflicts with FAT ops.
pub struct SubDirectory {
    index: u8,
}

impl SubDirectory {
    pub fn new(index: u8) -> Self {
        assert!(index < 4, "SubDirectory index must be 0–3");
        Self { index }
    }

    fn base_offset(&self) -> usize {
        (SUBDIR_START_BLOCK as usize + self.index as usize * 2) * BLOCK_SIZE
    }

    fn slot_offset(&self, slot: usize) -> usize {
        self.base_offset() + slot * SUBDIR_ENTRY_SIZE
    }

    /// Return all valid (non-empty) directory entries.
    pub fn entries(&self, image: &DiskImage) -> Vec<DirectoryEntry> {
        (0..SUBDIR_CAPACITY)
            .filter_map(|i| {
                let off = self.slot_offset(i);
                DirectoryEntry::from_bytes(&image.data[off..off + SUBDIR_ENTRY_SIZE])
            })
            .collect()
    }

    /// Find an entry by name (case-sensitive, exact match on trimmed name).
    pub fn find(&self, image: &DiskImage, name: &str) -> Option<DirectoryEntry> {
        self.entries(image)
            .into_iter()
            .find(|e| e.name_str() == name)
    }

    /// Find an entry's slot index by name (for internal use).
    fn find_slot(&self, image: &DiskImage, name: &str) -> Option<usize> {
        (0..SUBDIR_CAPACITY).find(|&i| {
            let off = self.slot_offset(i);
            let bytes = &image.data[off..off + SUBDIR_ENTRY_SIZE];
            DirectoryEntry::from_bytes(bytes)
                .map(|e| e.name_str() == name)
                .unwrap_or(false)
        })
    }

    /// Return the number of free (unused) slots.
    pub fn free_slots(&self, image: &DiskImage) -> usize {
        SUBDIR_CAPACITY - self.entries(image).len()
    }

    /// Add a directory entry to the first free slot.
    /// Validates that entry.name is ≤ 11 bytes. Name validation is enforced here
    /// regardless of call site (CLI, UniFFI, or direct library use).
    pub fn add(&self, image: &mut DiskImage, entry: DirectoryEntry) -> Result<()> {
        // Validate name length
        let name_len = entry.name.iter().position(|&b| b == 0).unwrap_or(11);
        if name_len == 0 {
            return Err(Error::InvalidName("<empty>".to_string()));
        }

        let free_slot = (0..SUBDIR_CAPACITY).find(|&i| {
            let off = self.slot_offset(i);
            image.data[off + 1] == 0x00  // file_type byte == 0 means unused
        });

        let slot = free_slot.ok_or(Error::DirectoryFull)?;
        let off = self.slot_offset(slot);
        let bytes = entry.to_bytes();
        image.data[off..off + SUBDIR_ENTRY_SIZE].copy_from_slice(&bytes);
        Ok(())
    }

    /// Remove an entry by name. Returns Err(FileNotFound) if not present.
    pub fn remove(&self, image: &mut DiskImage, name: &str) -> Result<()> {
        let slot = self.find_slot(image, name)
            .ok_or_else(|| Error::FileNotFound(name.to_string()))?;
        let off = self.slot_offset(slot);
        // Zero out the slot (file_type byte = 0 marks it unused)
        image.data[off..off + SUBDIR_ENTRY_SIZE].fill(0);
        Ok(())
    }
}
```

- [ ] **Step 4: Add to lib.rs**

```rust
// Add to crates/sd1disk/src/lib.rs:
pub mod directory;
pub use directory::{DirectoryEntry, FileType, SubDirectory, validate_name};
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sd1disk directory
```

Expected: all 8 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sd1disk/src/directory.rs crates/sd1disk/src/lib.rs
git commit -m "feat: add FileType, DirectoryEntry, and SubDirectory with add/find/remove"
```

---

## Task 6: SysEx Parser

**Files:**
- Create: `crates/sd1disk/src/sysex.rs`
- Modify: `crates/sd1disk/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

```rust
// In crates/sd1disk/src/sysex.rs, at bottom:
#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid SysEx packet for testing.
    /// payload_bytes is the raw (pre-nybblization) data.
    fn make_sysex(msg_type: u8, payload_bytes: &[u8]) -> Vec<u8> {
        let mut pkt = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, msg_type];
        for &b in payload_bytes {
            pkt.push((b >> 4) & 0x0F);  // hi nybble
            pkt.push(b & 0x0F);          // lo nybble
        }
        pkt.push(0xF7);
        pkt
    }

    #[test]
    fn parse_one_program_header() {
        let payload = vec![0xABu8; 530];
        let sysex = make_sysex(0x02, &payload);
        let packet = SysExPacket::parse(&sysex).unwrap();
        assert_eq!(packet.message_type, MessageType::OneProgram);
        assert_eq!(packet.midi_channel, 0);
        assert_eq!(packet.payload, payload);
    }

    #[test]
    fn parse_one_preset_header() {
        let payload = vec![0x55u8; 48];
        let sysex = make_sysex(0x04, &payload);
        let packet = SysExPacket::parse(&sysex).unwrap();
        assert_eq!(packet.message_type, MessageType::OnePreset);
        assert_eq!(packet.payload.len(), 48);
        assert_eq!(packet.payload, payload);
    }

    #[test]
    fn denybblize_is_inverse_of_nybblize() {
        let original = (0u8..=255).collect::<Vec<_>>();
        let nybblized = nybblize(&original);
        let recovered = denybblize(&nybblized);
        assert_eq!(recovered, original);
    }

    #[test]
    fn to_bytes_round_trips() {
        let payload = vec![0x42u8; 530];
        let sysex = make_sysex(0x02, &payload);
        let packet = SysExPacket::parse(&sysex).unwrap();
        let rebuilt = packet.to_bytes(0x00);
        assert_eq!(rebuilt, sysex);
    }

    #[test]
    fn wrong_manufacturer_code_returns_error() {
        let mut bad = vec![0xF0, 0x41, 0x05, 0x00, 0x00, 0x02]; // 0x41 = Roland
        bad.extend_from_slice(&[0x00; 10]);
        bad.push(0xF7);
        assert!(SysExPacket::parse(&bad).is_err());
    }

    #[test]
    fn missing_f7_tail_returns_error() {
        let mut bad = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, 0x02];
        bad.extend_from_slice(&[0x00; 10]);
        // No F7
        assert!(SysExPacket::parse(&bad).is_err());
    }

    #[test]
    fn midi_channel_is_parsed() {
        let payload = vec![0u8; 48];
        let mut pkt = vec![0xF0, 0x0F, 0x05, 0x00, 0x09, 0x04]; // channel 9
        for &b in &payload {
            pkt.push((b >> 4) & 0x0F);
            pkt.push(b & 0x0F);
        }
        pkt.push(0xF7);
        let packet = SysExPacket::parse(&pkt).unwrap();
        assert_eq!(packet.midi_channel, 9);
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p sd1disk sysex 2>&1 | head -10
```

Expected: compile error.

- [ ] **Step 3: Implement sysex.rs**

```rust
// crates/sd1disk/src/sysex.rs
use crate::{Error, Result};

const SYSEX_START: u8 = 0xF0;
const ENSONIQ_CODE: u8 = 0x0F;
const VFX_FAMILY: u8 = 0x05;
const VFX_MODEL: u8 = 0x00;
const SYSEX_END: u8 = 0xF7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    OneProgram,
    AllPrograms,
    OnePreset,
    AllPresets,
    SingleSequence,
    AllSequences,
    TrackParameters,
}

impl MessageType {
    fn from_byte(b: u8) -> Result<Self> {
        Ok(match b {
            0x02 => MessageType::OneProgram,
            0x03 => MessageType::AllPrograms,
            0x04 => MessageType::OnePreset,
            0x05 => MessageType::AllPresets,
            0x09 => MessageType::SingleSequence,
            0x0A => MessageType::AllSequences,
            0x0B => MessageType::TrackParameters,
            other => return Err(Error::InvalidSysEx("unknown message type")),
        })
    }

    fn to_byte(&self) -> u8 {
        match self {
            MessageType::OneProgram      => 0x02,
            MessageType::AllPrograms     => 0x03,
            MessageType::OnePreset       => 0x04,
            MessageType::AllPresets      => 0x05,
            MessageType::SingleSequence  => 0x09,
            MessageType::AllSequences    => 0x0A,
            MessageType::TrackParameters => 0x0B,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            MessageType::OneProgram      => "OneProgram",
            MessageType::AllPrograms     => "AllPrograms",
            MessageType::OnePreset       => "OnePreset",
            MessageType::AllPresets      => "AllPresets",
            MessageType::SingleSequence  => "SingleSequence",
            MessageType::AllSequences    => "AllSequences",
            MessageType::TrackParameters => "TrackParameters",
        }
    }
}

pub struct SysExPacket {
    pub message_type: MessageType,
    pub midi_channel: u8,
    pub payload: Vec<u8>,  // de-nybblized internal data
}

impl SysExPacket {
    /// Parse and de-nybblize a raw SysEx byte stream.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 {
            return Err(Error::InvalidSysEx("packet too short"));
        }
        if bytes[0] != SYSEX_START {
            return Err(Error::InvalidSysEx("missing F0 start byte"));
        }
        if *bytes.last().unwrap() != SYSEX_END {
            return Err(Error::InvalidSysEx("missing F7 end byte"));
        }
        if bytes[1] != ENSONIQ_CODE {
            return Err(Error::InvalidSysEx("not an Ensoniq packet (expected 0F)"));
        }
        if bytes[2] != VFX_FAMILY {
            return Err(Error::InvalidSysEx("not a VFX family packet (expected 05)"));
        }
        if bytes[3] != VFX_MODEL {
            return Err(Error::InvalidSysEx("not a VFX model packet (expected 00)"));
        }
        let midi_channel = bytes[4];
        let message_type = MessageType::from_byte(bytes[5])?;

        // Nybblized payload is bytes[6..len-1]
        let nybbles = &bytes[6..bytes.len() - 1];
        if nybbles.len() % 2 != 0 {
            return Err(Error::InvalidSysEx("odd number of nybble bytes"));
        }
        let payload = denybblize(nybbles);

        Ok(SysExPacket { message_type, midi_channel, payload })
    }

    /// Re-nybblize and frame as a complete SysEx packet.
    pub fn to_bytes(&self, channel: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(7 + self.payload.len() * 2);
        out.push(SYSEX_START);
        out.push(ENSONIQ_CODE);
        out.push(VFX_FAMILY);
        out.push(VFX_MODEL);
        out.push(channel);
        out.push(self.message_type.to_byte());
        out.extend(nybblize(&self.payload));
        out.push(SYSEX_END);
        out
    }
}

/// Encode raw bytes as nybbles: each byte → 0000HHHH 0000LLLL.
pub fn nybblize(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for &b in data {
        out.push((b >> 4) & 0x0F);
        out.push(b & 0x0F);
    }
    out
}

/// Decode nybble pairs back to bytes: (hi << 4) | lo.
pub fn denybblize(nybbles: &[u8]) -> Vec<u8> {
    nybbles.chunks(2).map(|pair| (pair[0] << 4) | pair[1]).collect()
}
```

- [ ] **Step 4: Add to lib.rs**

```rust
// Add to crates/sd1disk/src/lib.rs:
pub mod sysex;
pub use sysex::{MessageType, SysExPacket};
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sd1disk sysex
```

Expected: all 7 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sd1disk/src/sysex.rs crates/sd1disk/src/lib.rs
git commit -m "feat: add SysExPacket parser with de-nybblization and round-trip serialization"
```

---

## Task 7: Domain Types (Program, Preset, Sequence)

**Files:**
- Create: `crates/sd1disk/src/types.rs`
- Modify: `crates/sd1disk/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

```rust
// In crates/sd1disk/src/types.rs, at bottom:
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysex::{MessageType, SysExPacket, nybblize};

    fn make_program_sysex(name: &[u8; 11]) -> SysExPacket {
        let mut payload = vec![0u8; 530];
        // Program name is at bytes 498–508 in the payload
        payload[498..509].copy_from_slice(name);
        SysExPacket {
            message_type: MessageType::OneProgram,
            midi_channel: 0,
            payload,
        }
    }

    fn make_preset_sysex() -> SysExPacket {
        SysExPacket {
            message_type: MessageType::OnePreset,
            midi_channel: 0,
            payload: vec![0xAAu8; 48],
        }
    }

    #[test]
    fn program_from_sysex_succeeds() {
        let pkt = make_program_sysex(b"MY_PROG    ");
        let prog = Program::from_sysex(&pkt).unwrap();
        assert_eq!(prog.name(), "MY_PROG");
    }

    #[test]
    fn program_to_bytes_round_trips() {
        let pkt = make_program_sysex(b"ROUND_TRIP ");
        let prog = Program::from_sysex(&pkt).unwrap();
        assert_eq!(prog.to_bytes(), pkt.payload.as_slice());
    }

    #[test]
    fn program_wrong_message_type_returns_error() {
        let pkt = SysExPacket {
            message_type: MessageType::OnePreset,  // wrong
            midi_channel: 0,
            payload: vec![0u8; 530],
        };
        assert!(matches!(Program::from_sysex(&pkt), Err(crate::Error::WrongMessageType { .. })));
    }

    #[test]
    fn program_wrong_size_returns_error() {
        let pkt = SysExPacket {
            message_type: MessageType::OneProgram,
            midi_channel: 0,
            payload: vec![0u8; 100],  // too short
        };
        assert!(Program::from_sysex(&pkt).is_err());
    }

    #[test]
    fn preset_from_sysex_succeeds() {
        let pkt = make_preset_sysex();
        let preset = Preset::from_sysex(&pkt).unwrap();
        assert_eq!(preset.to_bytes(), pkt.payload.as_slice());
    }

    #[test]
    fn program_file_type_is_one_program() {
        let pkt = make_program_sysex(b"FILETYP    ");
        let prog = Program::from_sysex(&pkt).unwrap();
        assert_eq!(prog.file_type(), crate::FileType::OneProgram);
    }

    #[test]
    fn preset_file_type_is_one_preset() {
        let pkt = make_preset_sysex();
        let preset = Preset::from_sysex(&pkt).unwrap();
        assert_eq!(preset.file_type(), crate::FileType::OnePreset);
    }

    #[test]
    fn program_to_sysex_round_trips() {
        let pkt = make_program_sysex(b"SYSEXRTRIP ");
        let prog = Program::from_sysex(&pkt).unwrap();
        let rebuilt_pkt = prog.to_sysex(0);
        let reparsed = Program::from_sysex(&rebuilt_pkt).unwrap();
        assert_eq!(reparsed.to_bytes(), prog.to_bytes());
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p sd1disk types 2>&1 | head -10
```

Expected: compile error.

- [ ] **Step 3: Implement types.rs**

```rust
// crates/sd1disk/src/types.rs
use std::borrow::Cow;
use crate::{Error, FileType, Result};
use crate::sysex::{MessageType, SysExPacket};

// Byte offset of the program name within a one-program payload
const PROGRAM_NAME_OFFSET: usize = 498;
const PROGRAM_NAME_LEN: usize = 11;
const PROGRAM_SIZE: usize = 530;
const PRESET_SIZE: usize = 48;

pub struct Program([u8; PROGRAM_SIZE]);

impl Program {
    /// Construct from a SysEx packet. Validates message type and payload size.
    pub fn from_sysex(packet: &SysExPacket) -> Result<Self> {
        if packet.message_type != MessageType::OneProgram {
            return Err(Error::WrongMessageType {
                expected: "OneProgram".to_string(),
                got: packet.message_type.display_name().to_string(),
            });
        }
        if packet.payload.len() != PROGRAM_SIZE {
            return Err(Error::InvalidSysEx("OneProgram payload must be 530 bytes"));
        }
        let mut data = [0u8; PROGRAM_SIZE];
        data.copy_from_slice(&packet.payload);
        Ok(Program(data))
    }

    /// Construct directly from raw bytes (for reading from disk).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != PROGRAM_SIZE {
            return Err(Error::InvalidSysEx("Program data must be 530 bytes"));
        }
        let mut data = [0u8; PROGRAM_SIZE];
        data.copy_from_slice(bytes);
        Ok(Program(data))
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The program name (bytes 498–508), trimmed of null/space padding.
    pub fn name(&self) -> Cow<'_, str> {
        let raw = &self.0[PROGRAM_NAME_OFFSET..PROGRAM_NAME_OFFSET + PROGRAM_NAME_LEN];
        let trimmed: Vec<u8> = raw.iter().copied().take_while(|&b| b != 0 && b != b' ').collect();
        String::from_utf8_lossy(&trimmed).into_owned().into()
    }

    /// Serialize to a SysEx packet (re-nybblizes the payload).
    pub fn to_sysex(&self, channel: u8) -> SysExPacket {
        SysExPacket {
            message_type: MessageType::OneProgram,
            midi_channel: channel,
            payload: self.0.to_vec(),
        }
    }

    pub fn file_type(&self) -> FileType {
        FileType::OneProgram
    }
}

pub struct Preset([u8; PRESET_SIZE]);

impl Preset {
    pub fn from_sysex(packet: &SysExPacket) -> Result<Self> {
        if packet.message_type != MessageType::OnePreset {
            return Err(Error::WrongMessageType {
                expected: "OnePreset".to_string(),
                got: packet.message_type.display_name().to_string(),
            });
        }
        if packet.payload.len() != PRESET_SIZE {
            return Err(Error::InvalidSysEx("OnePreset payload must be 48 bytes"));
        }
        let mut data = [0u8; PRESET_SIZE];
        data.copy_from_slice(&packet.payload);
        Ok(Preset(data))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != PRESET_SIZE {
            return Err(Error::InvalidSysEx("Preset data must be 48 bytes"));
        }
        let mut data = [0u8; PRESET_SIZE];
        data.copy_from_slice(bytes);
        Ok(Preset(data))
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn to_sysex(&self, channel: u8) -> SysExPacket {
        SysExPacket {
            message_type: MessageType::OnePreset,
            midi_channel: channel,
            payload: self.0.to_vec(),
        }
    }

    pub fn file_type(&self) -> FileType {
        FileType::OnePreset
    }
}

pub struct Sequence(Vec<u8>);

impl Sequence {
    pub fn from_sysex(packet: &SysExPacket) -> Result<Self> {
        match packet.message_type {
            MessageType::SingleSequence | MessageType::AllSequences => {}
            _ => return Err(Error::WrongMessageType {
                expected: "SingleSequence or AllSequences".to_string(),
                got: packet.message_type.display_name().to_string(),
            }),
        }
        Ok(Sequence(packet.payload.clone()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Sequence(bytes.to_vec())
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn to_sysex(&self, channel: u8) -> SysExPacket {
        SysExPacket {
            message_type: MessageType::SingleSequence,
            midi_channel: channel,
            payload: self.0.clone(),
        }
    }

    pub fn file_type(&self) -> FileType {
        FileType::OneSequence
    }
}
```

- [ ] **Step 4: Add to lib.rs**

```rust
// Add to crates/sd1disk/src/lib.rs:
pub mod types;
pub use types::{Program, Preset, Sequence};
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sd1disk types
```

Expected: all 8 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sd1disk/src/types.rs crates/sd1disk/src/lib.rs
git commit -m "feat: add Program, Preset, Sequence domain types with SysEx conversion"
```

---

## Task 8: Operations — List and Inspect

**Files:**
- Create: `crates/sd1disk/tests/operations_tests.rs`

These are integration tests using `disk_with_everything.img`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/sd1disk/tests/operations_tests.rs
use sd1disk::{DiskImage, SubDirectory};
use std::path::Path;

fn everything_img() -> DiskImage {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../disk_with_everything.img");
    DiskImage::open(&path).expect("disk_with_everything.img must exist")
}

#[test]
fn list_returns_entries_from_everything_disk() {
    let img = everything_img();
    let mut all_entries = vec![];
    for dir_idx in 0..4u8 {
        let dir = SubDirectory::new(dir_idx);
        all_entries.extend(dir.entries(&img));
    }
    // The "everything" disk should have at least one file
    assert!(!all_entries.is_empty(), "disk_with_everything.img should have files");
    // Each entry should have a valid file type
    for entry in &all_entries {
        let name = entry.name_str();
        assert!(!name.is_empty(), "entry name should not be empty");
        assert!(entry.size_blocks > 0, "entry should have non-zero size");
    }
}

#[test]
fn inspect_free_blocks_is_reasonable() {
    let img = everything_img();
    let free = img.free_blocks();
    // Must be between 0 and 1577 (1600 - 23 reserved)
    assert!(free <= 1577, "free block count {} is impossible", free);
}

#[test]
fn blank_disk_inspect() {
    let img = DiskImage::create();
    let free = img.free_blocks();
    assert!(free > 0, "blank disk should have free blocks");
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p sd1disk --test operations_tests 2>&1 | head -20
```

Expected: tests compile but may fail if `disk_with_everything.img` has unexpected content. Review output.

- [ ] **Step 3: Run all tests to check nothing regressed**

```bash
cargo test -p sd1disk
```

Expected: all previous tests still pass; new tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/sd1disk/tests/operations_tests.rs
git commit -m "test: add integration tests for list and inspect using disk_with_everything.img"
```

---

## Task 9: Operations — Write

Extend the operations integration tests with a write round-trip.

**Files:**
- Modify: `crates/sd1disk/tests/operations_tests.rs`

- [ ] **Step 1: Write the failing test**

```rust
// Add to crates/sd1disk/tests/operations_tests.rs:
use sd1disk::{DiskImage, SubDirectory, FileAllocationTable, Program};
use sd1disk::sysex::{MessageType, SysExPacket};

fn make_test_program_packet(name_bytes: &[u8; 11]) -> SysExPacket {
    let mut payload = vec![0u8; 530];
    payload[498..509].copy_from_slice(name_bytes);
    SysExPacket {
        message_type: MessageType::OneProgram,
        midi_channel: 0,
        payload,
    }
}

#[test]
fn write_program_to_blank_disk_and_find_it() {
    let mut img = DiskImage::create();
    let initial_free = img.free_blocks();

    // Parse the SysEx
    let pkt = make_test_program_packet(b"TEST_PROG  ");
    let prog = Program::from_sysex(&pkt).unwrap();

    // Determine blocks needed
    let data = prog.to_bytes();
    let n_blocks = ((data.len() + 511) / 512) as u16;  // ceil div

    // Allocate blocks
    let blocks = FileAllocationTable::allocate(&mut img, n_blocks).unwrap();
    assert!(blocks[0] >= 23, "must not allocate reserved blocks");

    // Write data into blocks
    for (i, &block_num) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(block_num).unwrap();
        if end > start {
            block[..end - start].copy_from_slice(&data[start..end]);
        }
    }

    // Link FAT chain
    FileAllocationTable::set_chain(&mut img, &blocks);

    // Build directory entry
    use sd1disk::{DirectoryEntry, validate_name};
    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: validate_name("TEST_PROG").unwrap(),
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };

    let dir = SubDirectory::new(0);
    dir.add(&mut img, entry).unwrap();

    // Update free block count
    img.set_free_blocks(initial_free - n_blocks as u32);

    // Verify we can find it
    let found = dir.find(&img, "TEST_PROG").unwrap();
    assert_eq!(found.size_bytes, data.len() as u32);
    assert_eq!(found.size_blocks, n_blocks);
    assert!(img.free_blocks() < initial_free);
}

#[test]
fn write_then_read_back_data_matches() {
    let mut img = DiskImage::create();
    let pkt = make_test_program_packet(b"READBACK   ");
    let prog = Program::from_sysex(&pkt).unwrap();
    let data = prog.to_bytes().to_vec();
    let n_blocks = ((data.len() + 511) / 512) as u16;

    let blocks = FileAllocationTable::allocate(&mut img, n_blocks).unwrap();
    for (i, &b) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(b).unwrap();
        if end > start {
            block[..end - start].copy_from_slice(&data[start..end]);
        }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);

    // Read back
    let chain = FileAllocationTable::chain(&img, blocks[0]).unwrap();
    let mut read_back = Vec::new();
    for &b in &chain {
        read_back.extend_from_slice(img.block(b).unwrap());
    }
    read_back.truncate(data.len());
    assert_eq!(read_back, data);
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p sd1disk --test operations_tests write
```

Expected: both write tests pass.

- [ ] **Step 3: Run all tests**

```bash
cargo test -p sd1disk
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/sd1disk/tests/operations_tests.rs
git commit -m "test: add write operation integration tests (allocate, chain, directory entry)"
```

---

## Task 10: Operations — Extract and Delete

- [ ] **Step 1: Write the failing tests**

```rust
// Add to crates/sd1disk/tests/operations_tests.rs:

#[test]
fn write_then_extract_matches_original() {
    let mut img = DiskImage::create();
    let pkt = make_test_program_packet(b"EXTRACT_ME ");
    let prog = Program::from_sysex(&pkt).unwrap();
    let data = prog.to_bytes().to_vec();
    let n_blocks = ((data.len() + 511) / 512) as u16;

    let blocks = FileAllocationTable::allocate(&mut img, n_blocks).unwrap();
    for (i, &b) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(b).unwrap();
        if end > start { block[..end - start].copy_from_slice(&data[start..end]); }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);
    use sd1disk::{DirectoryEntry, validate_name};
    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: validate_name("EXTRACT_ME").unwrap(),
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };
    SubDirectory::new(0).add(&mut img, entry.clone()).unwrap();

    // Extract: find entry → follow chain → read blocks → reconstruct
    let found = SubDirectory::new(0).find(&img, "EXTRACT_ME").unwrap();
    let chain = FileAllocationTable::chain(&img, found.first_block as u16).unwrap();
    let mut extracted = Vec::new();
    for &b in &chain {
        extracted.extend_from_slice(img.block(b).unwrap());
    }
    extracted.truncate(found.size_bytes as usize);

    let recovered = Program::from_bytes(&extracted).unwrap();
    assert_eq!(recovered.to_bytes(), data.as_slice());
    assert_eq!(recovered.name(), "EXTRACT_ME");
}

#[test]
fn delete_frees_blocks_and_removes_entry() {
    let mut img = DiskImage::create();
    let initial_free = img.free_blocks();
    let pkt = make_test_program_packet(b"DELETE_ME  ");
    let prog = Program::from_sysex(&pkt).unwrap();
    let data = prog.to_bytes().to_vec();
    let n_blocks = ((data.len() + 511) / 512) as u16;

    let blocks = FileAllocationTable::allocate(&mut img, n_blocks).unwrap();
    for (i, &b) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(b).unwrap();
        if end > start { block[..end - start].copy_from_slice(&data[start..end]); }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);
    use sd1disk::{DirectoryEntry, validate_name};
    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: validate_name("DELETE_ME").unwrap(),
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };
    let dir = SubDirectory::new(0);
    dir.add(&mut img, entry).unwrap();
    img.set_free_blocks(initial_free - n_blocks as u32);

    // Delete
    let found = dir.find(&img, "DELETE_ME").unwrap();
    let chain = FileAllocationTable::chain(&img, found.first_block as u16).unwrap();
    let freed = chain.len() as u32;
    FileAllocationTable::free_chain(&mut img, found.first_block as u16);
    dir.remove(&mut img, "DELETE_ME").unwrap();
    img.set_free_blocks(img.free_blocks() + freed);

    assert!(dir.find(&img, "DELETE_ME").is_none());
    assert_eq!(img.free_blocks(), initial_free);
}

#[test]
fn delete_file_not_found_returns_error() {
    let mut img = DiskImage::create();
    let dir = SubDirectory::new(0);
    let result = dir.remove(&mut img, "NONEXISTENT");
    assert!(matches!(result, Err(sd1disk::Error::FileNotFound(_))));
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p sd1disk --test operations_tests
```

Expected: all operation tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/sd1disk/tests/operations_tests.rs
git commit -m "test: add extract and delete integration tests"
```

---

## Task 11: Operations — Save/Reload Round-Trip

- [ ] **Step 1: Write the failing test**

```rust
// Add to crates/sd1disk/tests/operations_tests.rs:

#[test]
fn write_save_reload_file_survives() {
    let mut img = DiskImage::create();
    let pkt = make_test_program_packet(b"PERSISTED  ");
    let prog = Program::from_sysex(&pkt).unwrap();
    let data = prog.to_bytes().to_vec();
    let n_blocks = ((data.len() + 511) / 512) as u16;

    let blocks = FileAllocationTable::allocate(&mut img, n_blocks).unwrap();
    for (i, &b) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(b).unwrap();
        if end > start { block[..end - start].copy_from_slice(&data[start..end]); }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);
    use sd1disk::{DirectoryEntry, validate_name};
    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: validate_name("PERSISTED").unwrap(),
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };
    SubDirectory::new(0).add(&mut img, entry).unwrap();

    // Save and reload
    let path = std::env::temp_dir().join("sd1_persist_test.img");
    img.save(&path).unwrap();
    let reloaded = DiskImage::open(&path).unwrap();

    let found = SubDirectory::new(0).find(&reloaded, "PERSISTED").unwrap();
    assert_eq!(found.size_bytes, data.len() as u32);
    std::fs::remove_file(&path).ok();
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p sd1disk --test operations_tests save
```

Expected: passes.

- [ ] **Step 3: Run all library tests**

```bash
cargo test -p sd1disk
```

Expected: all pass. Note the count.

- [ ] **Step 4: Commit**

```bash
git add crates/sd1disk/tests/operations_tests.rs
git commit -m "test: add save/reload round-trip integration test"
```

---

## Task 12: CLI — All Subcommands

**Files:**
- Modify: `crates/sd1cli/src/main.rs`

- [ ] **Step 1: Write the CLI**

```rust
// crates/sd1cli/src/main.rs
use clap::{Parser, Subcommand};
use sd1disk::{
    DiskImage, SubDirectory, FileAllocationTable, Program, Preset, Sequence,
    validate_name, DirectoryEntry, FileType,
};
use sd1disk::sysex::SysExPacket;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "sd1disk", about = "Ensoniq SD-1 disk image utility")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all files on a disk image
    List {
        image: PathBuf,
    },
    /// Show disk metadata: free blocks, FAT health
    Inspect {
        image: PathBuf,
    },
    /// Write a SysEx file to a disk image
    Write {
        image: PathBuf,
        sysex: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
        dir: Option<u8>,
        #[arg(long)]
        overwrite: bool,
    },
    /// Extract a file from a disk image as SysEx
    Extract {
        image: PathBuf,
        name: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "0")]
        channel: u8,
    },
    /// Delete a file from a disk image
    Delete {
        image: PathBuf,
        name: String,
    },
    /// Create a new blank disk image
    Create {
        image: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> sd1disk::Result<()> {
    match cli.command {
        Command::List { image } => cmd_list(&image),
        Command::Inspect { image } => cmd_inspect(&image),
        Command::Write { image, sysex, name, dir, overwrite } =>
            cmd_write(&image, &sysex, name.as_deref(), dir, overwrite),
        Command::Extract { image, name, out, channel } =>
            cmd_extract(&image, &name, out.as_deref(), channel),
        Command::Delete { image, name } => cmd_delete(&image, &name),
        Command::Create { image } => cmd_create(&image),
    }
}

fn cmd_list(image_path: &Path) -> sd1disk::Result<()> {
    let img = DiskImage::open(image_path)?;
    println!("{:<12} {:<22} {:>6} {:>6} {:>4}",
        "NAME", "TYPE", "BLOCKS", "BYTES", "SLOT");
    println!("{}", "-".repeat(56));
    let mut total = 0usize;
    for dir_idx in 0..4u8 {
        let dir = SubDirectory::new(dir_idx);
        for entry in dir.entries(&img) {
            let type_str = format!("{:?}", entry.file_type);
            println!("{:<12} {:<22} {:>6} {:>6} {:>4}",
                entry.name_str(),
                type_str,
                entry.size_blocks,
                entry.size_bytes,
                entry.file_number,
            );
            total += 1;
        }
    }
    println!("\n{} file(s), {} free blocks", total, img.free_blocks());
    Ok(())
}

fn cmd_inspect(image_path: &Path) -> sd1disk::Result<()> {
    let img = DiskImage::open(image_path)?;
    println!("Disk image: {}", image_path.display());
    println!("Free blocks: {}", img.free_blocks());
    println!("Total blocks: 1600 (23 reserved, 1577 usable)");

    let mut free = 0u32;
    let mut used = 0u32;
    let mut bad = 0u32;
    for b in 23u16..1600 {
        match FileAllocationTable::entry(&img, b) {
            sd1disk::FatEntry::Free => free += 1,
            sd1disk::FatEntry::BadBlock => bad += 1,
            _ => used += 1,
        }
    }
    println!("FAT: {} free, {} used, {} bad", free, used, bad);
    Ok(())
}

fn cmd_write(
    image_path: &Path,
    sysex_path: &Path,
    name_override: Option<&str>,
    dir_override: Option<u8>,
    overwrite: bool,
) -> sd1disk::Result<()> {
    let sysex_bytes = std::fs::read(sysex_path)?;
    let packet = SysExPacket::parse(&sysex_bytes)?;

    // Determine data bytes and file type from message type
    let (data, file_type) = match &packet.message_type {
        sd1disk::sysex::MessageType::OneProgram => {
            let prog = Program::from_sysex(&packet)?;
            (prog.to_bytes().to_vec(), FileType::OneProgram)
        }
        sd1disk::sysex::MessageType::OnePreset => {
            let preset = Preset::from_sysex(&packet)?;
            (preset.to_bytes().to_vec(), FileType::OnePreset)
        }
        sd1disk::sysex::MessageType::SingleSequence |
        sd1disk::sysex::MessageType::AllSequences => {
            let seq = Sequence::from_sysex(&packet)?;
            (seq.to_bytes().to_vec(), seq.file_type())
        }
        other => {
            return Err(sd1disk::Error::InvalidSysEx("unsupported SysEx message type for write"));
        }
    };

    // Resolve file name
    let resolved_name = if let Some(n) = name_override {
        n.to_string()
    } else {
        sysex_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("UNNAMED")
            .to_string()
    };

    let name_arr = validate_name(&resolved_name)?;

    let mut img = DiskImage::open(image_path)?;

    // Find target sub-directory
    let target_dir_idx: u8 = if let Some(d) = dir_override {
        d - 1  // CLI is 1-indexed; internal is 0-indexed
    } else {
        (0..4u8)
            .find(|&i| SubDirectory::new(i).free_slots(&img) > 0)
            .ok_or(sd1disk::Error::DirectoryFull)?
    };
    let target_dir = SubDirectory::new(target_dir_idx);

    // Handle existing file
    if let Some(existing) = target_dir.find(&img, &resolved_name) {
        if !overwrite {
            return Err(sd1disk::Error::FileExists(resolved_name));
        }
        // Free old blocks before allocating new ones
        let old_chain = FileAllocationTable::chain(&img, existing.first_block as u16)?;
        let freed = old_chain.len() as u32;
        FileAllocationTable::free_chain(&mut img, existing.first_block as u16);
        target_dir.remove(&mut img, &resolved_name)?;
        img.set_free_blocks(img.free_blocks() + freed);
    }

    let n_blocks = ((data.len() + 511) / 512) as u16;
    let blocks = FileAllocationTable::allocate(&mut img, n_blocks)?;

    for (i, &block_num) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(block_num)?;
        block.fill(0);
        if end > start {
            block[..end - start].copy_from_slice(&data[start..end]);
        }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);

    let entry = DirectoryEntry {
        type_info: 0,
        file_type,
        name: name_arr,
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };
    target_dir.add(&mut img, entry)?;
    img.set_free_blocks(img.free_blocks() - n_blocks as u32);
    img.save(image_path)?;

    println!("Written: {} ({} bytes, {} block(s))", resolved_name, data.len(), n_blocks);
    Ok(())
}

fn cmd_extract(
    image_path: &Path,
    name: &str,
    out_path: Option<&Path>,
    channel: u8,
) -> sd1disk::Result<()> {
    let img = DiskImage::open(image_path)?;

    let entry = (0..4u8)
        .find_map(|i| SubDirectory::new(i).find(&img, name))
        .ok_or_else(|| sd1disk::Error::FileNotFound(name.to_string()))?;

    let chain = FileAllocationTable::chain(&img, entry.first_block as u16)?;
    let mut raw = Vec::new();
    for &b in &chain {
        raw.extend_from_slice(img.block(b)?);
    }
    raw.truncate(entry.size_bytes as usize);

    // Re-wrap in SysEx based on file type
    let sysex_bytes = match entry.file_type {
        FileType::OneProgram => {
            Program::from_bytes(&raw)?.to_sysex(channel).to_bytes(channel)
        }
        FileType::OnePreset => {
            Preset::from_bytes(&raw)?.to_sysex(channel).to_bytes(channel)
        }
        FileType::OneSequence | FileType::ThirtySequences | FileType::SixtySequences => {
            Sequence::from_bytes(&raw).to_sysex(channel).to_bytes(channel)
        }
        _ => return Err(sd1disk::Error::InvalidSysEx("unsupported file type for extract")),
    };

    let out = out_path.map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("{}.syx", name)));
    std::fs::write(&out, &sysex_bytes)?;
    println!("Extracted: {} → {}", name, out.display());
    Ok(())
}

fn cmd_delete(image_path: &Path, name: &str) -> sd1disk::Result<()> {
    let mut img = DiskImage::open(image_path)?;

    let (dir_idx, entry) = (0..4u8)
        .find_map(|i| SubDirectory::new(i).find(&img, name).map(|e| (i, e)))
        .ok_or_else(|| sd1disk::Error::FileNotFound(name.to_string()))?;

    let chain = FileAllocationTable::chain(&img, entry.first_block as u16)?;
    let freed = chain.len() as u32;
    FileAllocationTable::free_chain(&mut img, entry.first_block as u16);
    SubDirectory::new(dir_idx).remove(&mut img, name)?;
    img.set_free_blocks(img.free_blocks() + freed);
    img.save(image_path)?;

    println!("Deleted: {} ({} block(s) freed)", name, freed);
    Ok(())
}

fn cmd_create(image_path: &Path) -> sd1disk::Result<()> {
    let img = DiskImage::create();
    img.save(image_path)?;
    println!("Created blank disk image: {}", image_path.display());
    Ok(())
}
```

- [ ] **Step 2: Build the CLI**

```bash
cargo build -p sd1cli
```

Expected: compiles cleanly with no errors.

- [ ] **Step 3: Smoke test the CLI against the test images**

```bash
# List files on the "everything" disk
cargo run -p sd1cli -- list disk_with_everything.img

# Inspect the blank image
cargo run -p sd1cli -- inspect blank_image.img

# Create a new blank image
cargo run -p sd1cli -- create /tmp/test_new.img
cargo run -p sd1cli -- list /tmp/test_new.img
```

Expected: list shows files from `disk_with_everything.img`, inspect shows reasonable free block count, create produces a listable blank image.

- [ ] **Step 4: Run all tests**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sd1cli/src/main.rs
git commit -m "feat: add sd1cli with list/inspect/write/extract/delete/create subcommands"
```

---

## Task 13: Final Verification

- [ ] **Step 1: Run the full test suite**

```bash
cargo test
```

Expected: all tests pass, no warnings about unused imports or dead code.

- [ ] **Step 2: Build release binary**

```bash
cargo build --release
```

Expected: `target/release/sd1disk` binary produced.

- [ ] **Step 3: End-to-end smoke test with a real SysEx file (if available)**

If you have a `.syx` file from your SD-1:

```bash
# Write it to a blank disk
./target/release/sd1cli create /tmp/my_disk.img
./target/release/sd1cli write /tmp/my_disk.img your_patch.syx
./target/release/sd1cli list /tmp/my_disk.img

# Extract it back
./target/release/sd1cli extract /tmp/my_disk.img YOUR_PATCH --out /tmp/roundtrip.syx

# The two .syx files should be identical
diff your_patch.syx /tmp/roundtrip.syx && echo "Round-trip OK"
```

- [ ] **Step 4: Commit any fixes, then tag**

```bash
git tag v0.1.0
```

---

## Known Gaps / Future Work

- **AllPrograms SysEx** (60 programs): parsed but `cmd_write` only handles `OneProgram`. Extend by splitting the 60-program payload into 60 individual programs and writing each.
- **Preset name extraction**: `Preset` has no `name()` method yet — presets don't have a prominent name field in the spec, so this needs investigation of the actual byte layout.
- **UniFFI bindings**: add `uniffi` as a dependency and annotate the public API once the core is stable.
- **`--dir` validation**: if the specified dir index is full, the CLI currently errors without suggesting alternatives.
- **Overwrite across directories**: current `--overwrite` only checks the target directory. If the file exists in a *different* directory, it won't be found. This is a minor limitation acceptable for v0.1.
