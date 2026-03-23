// crates/sd1disk/src/fat.rs
use crate::{DiskImage, Error, Result};
use std::collections::HashSet;

const FAT_START_BLOCK: usize = 4;
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

    pub fn entry(image: &DiskImage, block: u16) -> FatEntry {
        match Self::read_raw(image, block) {
            0x000000 => FatEntry::Free,
            0x000001 => FatEntry::EndOfFile,
            0x000002 => FatEntry::BadBlock,
            n => FatEntry::Next(n as u16),
        }
    }

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

    pub fn free_chain(image: &mut DiskImage, start: u16) {
        let blocks = Self::chain(image, start).unwrap_or_else(|_| vec![start]);
        for b in blocks {
            Self::write_raw(image, b, 0x000000);
        }
    }

    pub fn set_chain(image: &mut DiskImage, blocks: &[u16]) {
        for (i, &block) in blocks.iter().enumerate() {
            let raw = if i + 1 < blocks.len() {
                blocks[i + 1] as u32
            } else {
                0x000001
            };
            Self::write_raw(image, block, raw);
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiskImage;

    fn blank() -> DiskImage { DiskImage::create() }

    // Regression: FAT must start at block 4 (hardware format). An earlier version
    // used block 5, causing the SD-1 to see all files as missing (BAD DEVICE ID).
    #[test]
    fn fat_starts_at_block_4_hardware_compatible() {
        let mut img = blank();
        // Write a chain at block 23 and verify the bytes land in block 4
        FileAllocationTable::set_chain(&mut img, &[23u16, 24]);
        // Block 4 offset 69 = entry 23, byte 0 (should be 0x00 then 0x00 then 0x18=24)
        let off = 4 * 512 + 23 * 3;
        assert_eq!(
            &img.data[off..off + 3], &[0x00, 0x00, 24],
            "FAT chain for block 23 must be stored in disk block 4 (hardware format)"
        );
        // Block 5 at the same relative offset must be untouched (all zero)
        let off5 = 5 * 512 + 23 * 3;
        assert_eq!(
            &img.data[off5..off5 + 3], &[0x00, 0x00, 0x00],
            "block 5 must NOT contain FAT data (only block 4 does on real hardware)"
        );
    }

    #[test]
    fn reserved_blocks_are_end_of_file() {
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
        for i in 1..blocks.len() {
            assert_eq!(blocks[i], blocks[i-1] + 1, "should be contiguous");
        }
        for &b in &blocks {
            assert!(b >= 23 && b < 1600, "block {} out of data range", b);
        }
    }

    #[test]
    fn set_chain_and_follow() {
        let mut img = blank();
        let blocks = vec![23u16, 24, 25];
        FileAllocationTable::set_chain(&mut img, &blocks);
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
        let all: Vec<u16> = (23..1600).collect();
        FileAllocationTable::set_chain(&mut img, &all);
        let result = FileAllocationTable::allocate(&mut img, 1);
        assert!(matches!(result, Err(crate::Error::DiskFull { .. })));
    }

    #[test]
    fn corrupt_fat_cycle_detected() {
        let mut img = blank();
        FileAllocationTable::set_next(&mut img, 23, FatEntry::Next(24));
        FileAllocationTable::set_next(&mut img, 24, FatEntry::Next(23));
        let result = FileAllocationTable::chain(&img, 23);
        assert!(matches!(result, Err(crate::Error::CorruptFat)));
    }
}
