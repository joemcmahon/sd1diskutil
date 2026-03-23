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
    pub size_bytes:        u32,  // stored as 24-bit on disk
}

impl DirectoryEntry {
    pub fn name_str(&self) -> Cow<'_, str> {
        // Trim trailing null bytes and spaces, then lossy-decode
        let trimmed = self.name.iter()
            .position(|&b| b == 0)
            .map(|i| &self.name[..i])
            .unwrap_or(&self.name);
        // Also trim trailing spaces (SD-1 names are space-padded)
        let trimmed = match trimmed.iter().rposition(|&b| b != b' ') {
            Some(i) => &trimmed[..=i],
            None => &trimmed[..0],
        };
        String::from_utf8_lossy(trimmed)
    }
}

/// Validate that a name fits in 11 bytes and is non-empty.
/// Returns the name as a space-padded [u8; 11] array.
pub fn validate_name(name: &str) -> Result<[u8; 11]> {
    if name.is_empty() || name.len() > 11 {
        return Err(Error::InvalidName(name.to_string()));
    }
    let mut arr = [b' '; 11];
    arr[..name.len()].copy_from_slice(name.as_bytes());
    Ok(arr)
}

/// SubDirectory is a stateless handle — index 0..3.
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

    fn entry_offset(&self, slot: usize) -> usize {
        self.base_offset() + slot * SUBDIR_ENTRY_SIZE
    }

    fn read_entry(&self, image: &DiskImage, slot: usize) -> Option<DirectoryEntry> {
        let off = self.entry_offset(slot);
        let data = &image.data[off..off + SUBDIR_ENTRY_SIZE];
        // A zero type byte in slot 1 (byte index 1) means empty slot
        if data[1] == 0 {
            return None;
        }
        let file_type = FileType::from_byte(data[1]).ok()?;
        let mut name = [0u8; 11];
        name.copy_from_slice(&data[2..13]);
        let size_blocks       = u16::from_be_bytes([data[14], data[15]]);
        let contiguous_blocks = u16::from_be_bytes([data[16], data[17]]);
        let first_block       = u32::from_be_bytes([data[18], data[19], data[20], data[21]]);
        let file_number       = data[22];
        let size_bytes        = u32::from_be_bytes([0, data[23], data[24], data[25]]);
        Some(DirectoryEntry {
            type_info: data[0],
            file_type,
            name,
            _reserved: data[13],
            size_blocks,
            contiguous_blocks,
            first_block,
            file_number,
            size_bytes,
        })
    }

    fn write_entry(&self, image: &mut DiskImage, slot: usize, entry: &DirectoryEntry) {
        let off = self.entry_offset(slot);
        let data = &mut image.data[off..off + SUBDIR_ENTRY_SIZE];
        data[0] = entry.type_info;
        data[1] = entry.file_type.to_byte();
        data[2..13].copy_from_slice(&entry.name);
        data[13] = 0;  // _reserved always zero
        data[14..16].copy_from_slice(&entry.size_blocks.to_be_bytes());
        data[16..18].copy_from_slice(&entry.contiguous_blocks.to_be_bytes());
        data[18..22].copy_from_slice(&entry.first_block.to_be_bytes());
        data[22] = entry.file_number;
        let sb = entry.size_bytes.to_be_bytes();
        data[23] = sb[1];
        data[24] = sb[2];
        data[25] = sb[3];
    }

    fn clear_slot(&self, image: &mut DiskImage, slot: usize) {
        let off = self.entry_offset(slot);
        image.data[off..off + SUBDIR_ENTRY_SIZE].fill(0);
    }

    pub fn entries(&self, image: &DiskImage) -> Vec<DirectoryEntry> {
        (0..SUBDIR_CAPACITY)
            .filter_map(|slot| self.read_entry(image, slot))
            .collect()
    }

    pub fn find(&self, image: &DiskImage, name: &str) -> Option<DirectoryEntry> {
        (0..SUBDIR_CAPACITY)
            .filter_map(|slot| self.read_entry(image, slot))
            .find(|e| e.name_str() == name)
    }

    pub fn add(&self, image: &mut DiskImage, entry: DirectoryEntry) -> Result<()> {
        // Validate name fits in 11 bytes; we use entry.name directly
        validate_name(entry.name_str().as_ref())?;
        // Find first free slot
        let slot = (0..SUBDIR_CAPACITY)
            .find(|&s| self.read_entry(image, s).is_none())
            .ok_or(Error::DirectoryFull)?;
        self.write_entry(image, slot, &entry);
        Ok(())
    }

    pub fn remove(&self, image: &mut DiskImage, name: &str) -> Result<()> {
        let slot = (0..SUBDIR_CAPACITY)
            .find(|&s| {
                self.read_entry(image, s)
                    .map(|e| e.name_str() == name)
                    .unwrap_or(false)
            })
            .ok_or_else(|| Error::FileNotFound(name.to_string()))?;
        self.clear_slot(image, slot);
        Ok(())
    }

    pub fn free_slots(&self, image: &DiskImage) -> usize {
        (0..SUBDIR_CAPACITY)
            .filter(|&s| self.read_entry(image, s).is_none())
            .count()
    }
}

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
        assert!(validate_name("12CharactersX").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("GOOD").is_ok());
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
