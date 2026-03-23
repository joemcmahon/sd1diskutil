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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image() -> DiskImage {
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
        let free = img.free_blocks();
        assert!(free <= 1600, "free blocks should be <= 1600, got {}", free);
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
