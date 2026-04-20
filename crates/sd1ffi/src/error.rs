use sd1disk::Error;

pub const SD1_OK: i32                  =  0;
pub const SD1_ERR_INVALID_IMAGE: i32   = -1;
pub const SD1_ERR_INVALID_SYSEX: i32   = -2;
pub const SD1_ERR_WRONG_MSG_TYPE: i32  = -3;
pub const SD1_ERR_FILE_NOT_FOUND: i32  = -4;
pub const SD1_ERR_FILE_EXISTS: i32     = -5;
pub const SD1_ERR_DISK_FULL: i32       = -6;
pub const SD1_ERR_DIRECTORY_FULL: i32  = -7;
pub const SD1_ERR_BLOCK_OOB: i32       = -8;
pub const SD1_ERR_INVALID_TYPE: i32    = -9;
pub const SD1_ERR_CORRUPT_FAT: i32     = -10;
pub const SD1_ERR_BAD_BLOCK: i32       = -11;
pub const SD1_ERR_INVALID_NAME: i32    = -12;
pub const SD1_ERR_INVALID_HFE: i32     = -13;
pub const SD1_ERR_HFE_CRC: i32         = -14;
pub const SD1_ERR_HFE_MISSING_SEC: i32 = -15;
pub const SD1_ERR_IO: i32              = -16;

pub fn to_error_code(e: &Error) -> i32 {
    match e {
        Error::InvalidImage(_)          => SD1_ERR_INVALID_IMAGE,
        Error::InvalidSysEx(_)          => SD1_ERR_INVALID_SYSEX,
        Error::WrongMessageType { .. }  => SD1_ERR_WRONG_MSG_TYPE,
        Error::FileNotFound(_)          => SD1_ERR_FILE_NOT_FOUND,
        Error::FileExists(_)            => SD1_ERR_FILE_EXISTS,
        Error::DiskFull { .. }          => SD1_ERR_DISK_FULL,
        Error::DirectoryFull            => SD1_ERR_DIRECTORY_FULL,
        Error::BlockOutOfRange(_)       => SD1_ERR_BLOCK_OOB,
        Error::InvalidFileType(_)       => SD1_ERR_INVALID_TYPE,
        Error::CorruptFat               => SD1_ERR_CORRUPT_FAT,
        Error::BadBlockInChain(_)       => SD1_ERR_BAD_BLOCK,
        Error::InvalidName(_)           => SD1_ERR_INVALID_NAME,
        Error::InvalidHfe(_)            => SD1_ERR_INVALID_HFE,
        Error::HfeCrcMismatch { .. }    => SD1_ERR_HFE_CRC,
        Error::HfeMissingSector { .. }  => SD1_ERR_HFE_MISSING_SEC,
        Error::Io(_)                    => SD1_ERR_IO,
    }
}

#[allow(dead_code)]
pub fn error_message(code: i32) -> &'static str {
    match code {
        SD1_OK                    => "success",
        SD1_ERR_INVALID_IMAGE     => "invalid disk image",
        SD1_ERR_INVALID_SYSEX     => "invalid SysEx data",
        SD1_ERR_WRONG_MSG_TYPE    => "wrong SysEx message type",
        SD1_ERR_FILE_NOT_FOUND    => "file not found",
        SD1_ERR_FILE_EXISTS       => "file already exists",
        SD1_ERR_DISK_FULL         => "disk full",
        SD1_ERR_DIRECTORY_FULL    => "directory full",
        SD1_ERR_BLOCK_OOB         => "block number out of range",
        SD1_ERR_INVALID_TYPE      => "invalid file type",
        SD1_ERR_CORRUPT_FAT       => "corrupt FAT",
        SD1_ERR_BAD_BLOCK         => "bad block in chain",
        SD1_ERR_INVALID_NAME      => "invalid file name",
        SD1_ERR_INVALID_HFE       => "invalid HFE file",
        SD1_ERR_HFE_CRC           => "HFE CRC mismatch",
        SD1_ERR_HFE_MISSING_SEC   => "HFE missing sector",
        SD1_ERR_IO                => "I/O error",
        _                         => "unknown error",
    }
}
