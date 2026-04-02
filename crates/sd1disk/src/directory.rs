// crates/sd1disk/src/directory.rs
use std::borrow::Cow;
use crate::{DiskImage, Error, Result};

const BLOCK_SIZE: usize = 512;
const SUBDIR_ENTRY_SIZE: usize = 26;
const SUBDIR_CAPACITY: usize = 39;
const SUBDIR_START_BLOCK: u16 = 15;  // SubDir 0 starts at block 15

/// Byte offset within block 1 where the VST3 plugin writes directory entries.
const BLOCK1_DIR_OFFSET: usize = BLOCK_SIZE + 0x1e;

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

/// Parse one 26-byte directory entry from a raw slice. Returns None for empty or invalid slots.
fn parse_entry(data: &[u8]) -> Option<DirectoryEntry> {
    if data.len() < SUBDIR_ENTRY_SIZE { return None; }
    if data[1] == 0 { return None; }
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

/// Read all valid directory entries written by the VST3 plugin at block 1 offset 0x1e.
/// On normal (hardware-formatted) disks the OS data at block 1 contains no valid file-type
/// bytes at that offset, so this returns an empty Vec.
pub fn block1_entries(image: &DiskImage) -> Vec<DirectoryEntry> {
    let base = BLOCK1_DIR_OFFSET;
    (0..SUBDIR_CAPACITY)
        .filter_map(|slot| {
            let off = base + slot * SUBDIR_ENTRY_SIZE;
            if off + SUBDIR_ENTRY_SIZE > image.data.len() { return None; }
            parse_entry(&image.data[off..off + SUBDIR_ENTRY_SIZE])
        })
        .collect()
}

/// Find a named entry in the VST3 block-1 directory. Case-sensitive.
pub fn block1_find(image: &DiskImage, name: &str) -> Option<DirectoryEntry> {
    block1_entries(image).into_iter().find(|e| e.name_str() == name)
}

/// Return the next file_number to assign for a given file type.
/// Counts all existing entries of that type across all 4 subdirectories.
pub fn next_file_number(img: &DiskImage, file_type: &FileType) -> u8 {
    (0..4u8)
        .flat_map(|i| SubDirectory::new(i).entries(img))
        .filter(|e| &e.file_type == file_type)
        .count() as u8
}

/// Return the type_info byte for a directory entry.
/// Only SixtySequences with embedded programs uses 0x20; everything else is 0x00.
pub fn file_type_info(file_type: &FileType, programs_embedded: bool) -> u8 {
    if *file_type == FileType::SixtySequences && programs_embedded { 0x20 } else { 0x00 }
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
        parse_entry(&image.data[off..off + SUBDIR_ENTRY_SIZE])
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

    // VST3 writes its directory to block 1 at offset 0x1e, zeroing block 15.
    // block1_entries() must read those entries correctly.
    #[test]
    fn block1_entries_reads_vst3_directory() {
        let mut img = blank();
        // Manually write two entries at block 1 offset 0x1e (VST3 format)
        let offset = 1 * BLOCK_SIZE + 0x1e;
        // Entry 0: TwentyPresets "SEQ-DB FPST"
        let e0: &[u8] = &[
            0x0f, 0x10,                                              // type_info, file_type
            b'S', b'E', b'Q', b'-', b'D', b'B', b' ', b'F', b'P', b'S', b'T', // name (11)
            0x00,                                                    // _reserved
            0x00, 0x02,                                              // size_blocks
            0x00, 0x02,                                              // contiguous_blocks
            0x00, 0x00, 0x00, 0x17,                                  // first_block = 23
            0x00,                                                    // file_number
            0x00, 0x03, 0xc0,                                        // size_bytes = 960
        ];
        img.data[offset..offset + SUBDIR_ENTRY_SIZE].copy_from_slice(e0);
        // Entry 1: SixtySequences "SEQ-DB FSEQ"
        let e1: &[u8] = &[
            0x2f, 0x13,
            b'S', b'E', b'Q', b'-', b'D', b'B', b' ', b'F', b'S', b'E', b'Q',
            0x00,
            0x00, 0xb2,
            0x00, 0xb2,
            0x00, 0x00, 0x00, 0x19,
            0x00,
            0x01, 0x64, 0x00,
        ];
        img.data[offset + SUBDIR_ENTRY_SIZE..offset + 2 * SUBDIR_ENTRY_SIZE].copy_from_slice(e1);

        let entries = block1_entries(&img);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name_str(), "SEQ-DB FPST");
        assert_eq!(entries[0].file_type, FileType::TwentyPresets);
        assert_eq!(entries[0].size_bytes, 960);
        assert_eq!(entries[1].name_str(), "SEQ-DB FSEQ");
        assert_eq!(entries[1].file_type, FileType::SixtySequences);
        assert_eq!(entries[1].first_block, 25);
    }

    #[test]
    fn block1_entries_returns_empty_on_normal_disk() {
        // A blank disk has the SD-1 OS data at block 1; block1_entries must return nothing.
        let img = blank();
        let entries = block1_entries(&img);
        assert!(entries.is_empty(), "normal disk block 1 should yield no directory entries");
    }

    #[test]
    fn block1_find_locates_entry_by_name() {
        let mut img = blank();
        let offset = 1 * BLOCK_SIZE + 0x1e;
        let e0: &[u8] = &[
            0x0f, 0x13,
            b'M', b'Y', b'F', b'I', b'L', b'E', b' ', b' ', b' ', b' ', b' ',
            0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x17, 0x00, 0x00, 0x02, 0x00,
        ];
        img.data[offset..offset + SUBDIR_ENTRY_SIZE].copy_from_slice(e0);
        assert!(block1_find(&img, "MYFILE").is_some());
        assert!(block1_find(&img, "NOTHERE").is_none());
    }

    fn make_entry(file_type: FileType, name: &[u8; 11], file_number: u8) -> DirectoryEntry {
        DirectoryEntry {
            type_info: 0,
            file_type,
            name: *name,
            _reserved: 0,
            size_blocks: 1,
            contiguous_blocks: 1,
            first_block: 23,
            file_number,
            size_bytes: 512,
        }
    }

    // ── next_file_number ─────────────────────────────────────────────────────

    #[test]
    fn next_file_number_is_zero_on_blank_disk() {
        let img = blank();
        assert_eq!(next_file_number(&img, &FileType::SixtySequences), 0);
        assert_eq!(next_file_number(&img, &FileType::OneProgram), 0);
    }

    #[test]
    fn next_file_number_counts_one_existing_file() {
        let mut img = blank();
        SubDirectory::new(0)
            .add(&mut img, make_entry(FileType::SixtySequences, b"SEQS1      ", 0))
            .unwrap();
        assert_eq!(next_file_number(&img, &FileType::SixtySequences), 1);
    }

    #[test]
    fn next_file_number_counts_across_all_subdirs() {
        let mut img = blank();
        SubDirectory::new(0)
            .add(&mut img, make_entry(FileType::SixtySequences, b"SEQS1      ", 0))
            .unwrap();
        SubDirectory::new(1)
            .add(&mut img, make_entry(FileType::SixtySequences, b"SEQS2      ", 1))
            .unwrap();
        assert_eq!(next_file_number(&img, &FileType::SixtySequences), 2);
    }

    #[test]
    fn next_file_number_ignores_different_file_types() {
        let mut img = blank();
        SubDirectory::new(0)
            .add(&mut img, make_entry(FileType::OneProgram, b"PROG1      ", 0))
            .unwrap();
        // OneProgram entry must not affect SixtySequences count
        assert_eq!(next_file_number(&img, &FileType::SixtySequences), 0);
        // And OneProgram count reflects existing entries
        assert_eq!(next_file_number(&img, &FileType::OneProgram), 1);
    }

    // ── file_type_info ───────────────────────────────────────────────────────

    #[test]
    fn file_type_info_is_zero_for_all_normal_file_types() {
        for ft in [
            FileType::OneProgram,
            FileType::SixtyPrograms,
            FileType::OnePreset,
            FileType::TwentyPresets,
            FileType::OneSequence,
            FileType::SixtySequences,
        ] {
            assert_eq!(
                file_type_info(&ft, false), 0x00,
                "expected 0x00 for {:?} without embedded programs", ft
            );
        }
    }

    #[test]
    fn file_type_info_is_0x20_for_sequences_with_programs_embedded() {
        assert_eq!(file_type_info(&FileType::SixtySequences, true), 0x20);
    }

    #[test]
    fn file_type_info_ignores_embed_flag_for_non_sequence_types() {
        // Only SixtySequences supports embedded programs; all others stay 0x00
        assert_eq!(file_type_info(&FileType::OneProgram, true), 0x00);
        assert_eq!(file_type_info(&FileType::SixtyPrograms, true), 0x00);
        assert_eq!(file_type_info(&FileType::TwentyPresets, true), 0x00);
    }
}
