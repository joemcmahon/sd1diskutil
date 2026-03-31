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
    /// HFE file has a bad or unsupported header
    InvalidHfe(&'static str),
    /// CRC mismatch detected while decoding an HFE sector
    HfeCrcMismatch { track: u8, side: u8, sector: u8 },
    /// A sector was not found in the HFE track data
    HfeMissingSector { track: u8, side: u8, sector: u8 },
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
            Error::InvalidHfe(msg) => write!(f, "Invalid HFE file: {}", msg),
            Error::HfeCrcMismatch { track, side, sector } => write!(
                f, "HFE CRC mismatch at track {} side {} sector {}", track, side, sector
            ),
            Error::HfeMissingSector { track, side, sector } => write!(
                f, "HFE missing sector at track {} side {} sector {}", track, side, sector
            ),
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

    #[test]
    fn hfe_errors_display() {
        let e = Error::InvalidHfe("bad signature");
        assert!(format!("{}", e).contains("bad signature"));

        let e = Error::HfeCrcMismatch { track: 3, side: 1, sector: 7 };
        let s = format!("{}", e);
        assert!(s.contains("3") && s.contains("1") && s.contains("7"));

        let e = Error::HfeMissingSector { track: 0, side: 0, sector: 5 };
        assert!(format!("{}", e).contains("5"));
    }
}
