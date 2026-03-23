pub mod error;
pub use error::{Error, Result};

pub mod image;
pub use image::DiskImage;

pub mod fat;
pub use fat::{FatEntry, FileAllocationTable};
