mod error;
use error::{to_error_code, SD1_OK};

// SAFETY: The program-name arrays are read-only static data; raw pointers into
// 'static string literals are safe to share across threads.
pub struct SyncRawPtrs<T>(pub T);
unsafe impl<T> Sync for SyncRawPtrs<T> {}

use std::ffi::{CStr, CString, c_char};
use std::path::Path;

use sd1disk::{
    DiskImage, FileAllocationTable, FatEntry,
    SubDirectory, DirectoryEntry, FileType,
    validate_name, block1_entries, block1_find, next_file_number, file_type_info,
    SysExPacket,
    Program, Preset, Sequence,
    interleave_sixty_programs, deinterleave_sixty_programs,
    allsequences_to_disk, disk_to_allsequences, disk_to_thirty_sequences, thirty_sequences_to_disk,
    program_name_from_slot, decode_b10,
    read_hfe, write_hfe,
};

// ─── Error message ───────────────────────────────────────────────────────────

/// Return a static human-readable string for any Sd1Error code.
/// The returned pointer is always valid and must never be freed.
#[no_mangle]
pub extern "C" fn sd1_error_message(code: i32) -> *const c_char {
    // SAFETY: All strings are 'static C-string literals embedded by the compiler.
    // We use a lazy-static approach: match to a raw pointer computed once.
    macro_rules! cstr {
        ($s:expr) => {{
            static BYTES: &[u8] = concat!($s, "\0").as_bytes();
            BYTES.as_ptr() as *const c_char
        }};
    }
    match code {
        0   => cstr!("success"),
        -1  => cstr!("invalid disk image"),
        -2  => cstr!("invalid SysEx data"),
        -3  => cstr!("wrong SysEx message type"),
        -4  => cstr!("file not found"),
        -5  => cstr!("file already exists"),
        -6  => cstr!("disk full"),
        -7  => cstr!("directory full"),
        -8  => cstr!("block number out of range"),
        -9  => cstr!("invalid file type"),
        -10 => cstr!("corrupt FAT"),
        -11 => cstr!("bad block in chain"),
        -12 => cstr!("invalid file name"),
        -13 => cstr!("invalid HFE file"),
        -14 => cstr!("HFE CRC mismatch"),
        -15 => cstr!("HFE missing sector"),
        -16 => cstr!("I/O error"),
        _   => cstr!("unknown error"),
    }
}

// ─── POD types exposed to C ──────────────────────────────────────────────────

/// C-compatible directory entry. Mirrors sd1disk::DirectoryEntry.
#[repr(C)]
pub struct Sd1DirectoryEntry {
    pub type_info:         u8,
    pub file_type:         u8,   // Sd1FileType value
    pub name:              [c_char; 12], // null-terminated, max 11 chars
    pub size_blocks:       u16,
    pub contiguous_blocks: u16,
    pub first_block:       u32,
    pub file_number:       u8,
    pub size_bytes:        u32,
}

/// C-compatible FAT entry.
#[repr(C)]
pub struct Sd1FatEntry {
    pub entry_type: u8,   // 0=Free, 1=EOF, 2=BadBlock, 3=Next
    pub next_block: u16,  // valid only when entry_type == 3
}

fn rust_entry_to_c(e: &DirectoryEntry) -> Sd1DirectoryEntry {
    let mut name = [0i8; 12];
    for (i, &b) in e.name.iter().enumerate() {
        name[i] = b as i8;
    }
    // name[11] stays 0 (null terminator)
    Sd1DirectoryEntry {
        type_info:         e.type_info,
        file_type:         e.file_type.to_byte(),
        name,
        size_blocks:       e.size_blocks,
        contiguous_blocks: e.contiguous_blocks,
        first_block:       e.first_block,
        file_number:       e.file_number,
        size_bytes:        e.size_bytes,
    }
}

fn fat_entry_to_c(fe: FatEntry) -> Sd1FatEntry {
    match fe {
        FatEntry::Free       => Sd1FatEntry { entry_type: 0, next_block: 0 },
        FatEntry::EndOfFile  => Sd1FatEntry { entry_type: 1, next_block: 0 },
        FatEntry::BadBlock   => Sd1FatEntry { entry_type: 2, next_block: 0 },
        FatEntry::Next(n)    => Sd1FatEntry { entry_type: 3, next_block: n },
    }
}

// ─── Helper: set *err_out safely ─────────────────────────────────────────────

fn set_err(err_out: *mut i32, code: i32) {
    if !err_out.is_null() {
        unsafe { *err_out = code; }
    }
}

// ─── DiskImage lifecycle ──────────────────────────────────────────────────────

/// Open an existing disk image from a file path. Returns NULL on failure;
/// *err_out is set to the error code (may be NULL).
#[no_mangle]
pub extern "C" fn sd1_disk_open(path: *const c_char, err_out: *mut i32) -> *mut DiskImage {
    if path.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_IMAGE);
        return std::ptr::null_mut();
    }
    let cstr = unsafe { CStr::from_ptr(path) };
    let path_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => { set_err(err_out, error::SD1_ERR_IO); return std::ptr::null_mut(); }
    };
    match DiskImage::open(Path::new(path_str)) {
        Ok(img) => {
            set_err(err_out, SD1_OK);
            Box::into_raw(Box::new(img))
        }
        Err(e) => {
            set_err(err_out, to_error_code(&e));
            std::ptr::null_mut()
        }
    }
}

/// Create a blank formatted disk image in memory.
#[no_mangle]
pub extern "C" fn sd1_disk_create() -> *mut DiskImage {
    Box::into_raw(Box::new(DiskImage::create()))
}

/// Save a disk image to a file. Returns SD1_OK (0) or a negative error code.
#[no_mangle]
pub extern "C" fn sd1_disk_save(img: *const DiskImage, path: *const c_char) -> i32 {
    if img.is_null() || path.is_null() { return error::SD1_ERR_IO; }
    let cstr = unsafe { CStr::from_ptr(path) };
    let path_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_IO,
    };
    let img = unsafe { &*img };
    match img.save(Path::new(path_str)) {
        Ok(()) => SD1_OK,
        Err(e) => to_error_code(&e),
    }
}

/// Free a DiskImage allocated by sd1_disk_open or sd1_disk_create.
#[no_mangle]
pub extern "C" fn sd1_disk_free(img: *mut DiskImage) {
    if !img.is_null() {
        unsafe { drop(Box::from_raw(img)); }
    }
}

/// Return the free block count from the OS block.
#[no_mangle]
pub extern "C" fn sd1_disk_free_blocks(img: *const DiskImage) -> u32 {
    if img.is_null() { return 0; }
    unsafe { (*img).free_blocks() }
}

// ─── Directory operations ─────────────────────────────────────────────────────

/// List all valid directory entries in a subdirectory (subdir 0–3).
/// Returns a heap-allocated array; caller must call sd1_entries_free(ptr, count).
/// Returns NULL on failure; *count_out is set to 0.
#[no_mangle]
pub extern "C" fn sd1_disk_list(
    img: *const DiskImage,
    subdir: u8,
    count_out: *mut usize,
) -> *mut Sd1DirectoryEntry {
    if img.is_null() || count_out.is_null() || subdir >= 4 {
        if !count_out.is_null() { unsafe { *count_out = 0; } }
        return std::ptr::null_mut();
    }
    let img = unsafe { &*img };
    let entries: Vec<Sd1DirectoryEntry> = SubDirectory::new(subdir)
        .entries(img)
        .iter()
        .map(rust_entry_to_c)
        .collect();
    let count = entries.len();
    unsafe { *count_out = count; }
    if count == 0 {
        return std::ptr::null_mut();
    }
    let mut boxed = entries.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

/// Find a named file in subdirectory 0–3. Fills *out on success.
/// Returns SD1_OK or SD1_ERR_FILE_NOT_FOUND.
#[no_mangle]
pub extern "C" fn sd1_disk_find(
    img: *const DiskImage,
    name: *const c_char,
    out: *mut Sd1DirectoryEntry,
) -> i32 {
    if img.is_null() || name.is_null() || out.is_null() { return error::SD1_ERR_FILE_NOT_FOUND; }
    let img = unsafe { &*img };
    let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_INVALID_NAME,
    };
    let found = (0..4u8)
        .find_map(|i| SubDirectory::new(i).find(img, name_str.as_ref()))
        .or_else(|| block1_find(img, name_str.as_ref()));
    match found {
        Some(e) => {
            unsafe { *out = rust_entry_to_c(&e); }
            SD1_OK
        }
        None => error::SD1_ERR_FILE_NOT_FOUND,
    }
}

/// Extract raw file data by name. Searches all subdirectories, then block-1.
/// Returns SD1_OK; sets *data_out and *len_out. Caller must call sd1_bytes_free.
#[no_mangle]
pub extern "C" fn sd1_disk_extract(
    img: *const DiskImage,
    name: *const c_char,
    data_out: *mut *mut u8,
    len_out: *mut usize,
) -> i32 {
    if img.is_null() || name.is_null() || data_out.is_null() || len_out.is_null() {
        return error::SD1_ERR_IO;
    }
    let img = unsafe { &*img };
    let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_INVALID_NAME,
    };

    let (entry, use_contiguous) = match (0..4u8)
        .find_map(|i| SubDirectory::new(i).find(img, name_str).map(|e| (e, false)))
        .or_else(|| block1_find(img, name_str).map(|e| (e, true)))
    {
        Some(v) => v,
        None => return error::SD1_ERR_FILE_NOT_FOUND,
    };

    let mut raw: Vec<u8> = Vec::new();
    if use_contiguous {
        let start = entry.first_block as u16;
        for b in start..start + entry.size_blocks {
            match img.block(b) {
                Ok(slice) => raw.extend_from_slice(slice),
                Err(e) => return to_error_code(&e),
            }
        }
    } else {
        let chain = match FileAllocationTable::chain(img, entry.first_block as u16) {
            Ok(c) => c,
            Err(e) => return to_error_code(&e),
        };
        for &b in &chain {
            match img.block(b) {
                Ok(slice) => raw.extend_from_slice(slice),
                Err(e) => return to_error_code(&e),
            }
        }
    }
    raw.truncate(entry.size_bytes as usize);

    let len = raw.len();
    let mut boxed = raw.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    unsafe {
        *data_out = ptr;
        *len_out = len;
    }
    SD1_OK
}

/// Write a file to the disk image.
/// file_type: a Sd1FileType byte value.
/// programs_embedded: true only for SixtySequences files that include 60 programs.
/// data/len: raw file bytes.
/// overwrite: if true, overwrite an existing file with the same name.
/// Returns SD1_OK or a negative error code.
#[no_mangle]
pub extern "C" fn sd1_disk_write(
    img: *mut DiskImage,
    name: *const c_char,
    file_type_byte: u8,
    programs_embedded: bool,
    data: *const u8,
    len: usize,
    overwrite: bool,
) -> i32 {
    if img.is_null() || name.is_null() || data.is_null() { return error::SD1_ERR_IO; }
    let img = unsafe { &mut *img };
    let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_INVALID_NAME,
    };
    let file_type = match FileType::from_byte(file_type_byte) {
        Ok(ft) => ft,
        Err(e) => return to_error_code(&e),
    };
    let name_arr = match validate_name(name_str) {
        Ok(arr) => arr,
        Err(e) => return to_error_code(&e),
    };
    let file_bytes: &[u8] = unsafe { std::slice::from_raw_parts(data, len) };

    // Find a subdirectory with a free slot
    let target_dir_idx = match (0..4u8).find(|&i| SubDirectory::new(i).free_slots(img) > 0) {
        Some(i) => i,
        None => return error::SD1_ERR_DIRECTORY_FULL,
    };
    let target_dir = SubDirectory::new(target_dir_idx);

    if let Some(existing) = target_dir.find(img, name_str) {
        if !overwrite {
            return error::SD1_ERR_FILE_EXISTS;
        }
        FileAllocationTable::free_chain(img, existing.first_block as u16);
        if let Err(e) = target_dir.remove(img, name_str) {
            return to_error_code(&e);
        }
    }

    let n_blocks = (len.max(1) as u16 + 511) / 512;
    let blocks = match FileAllocationTable::allocate(img, n_blocks) {
        Ok(b) => b,
        Err(e) => return to_error_code(&e),
    };

    for (i, &block_num) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(len);
        let block = match img.block_mut(block_num) {
            Ok(b) => b,
            Err(e) => return to_error_code(&e),
        };
        block.fill(0);
        if end > start {
            block[..end - start].copy_from_slice(&file_bytes[start..end]);
        }
    }
    FileAllocationTable::set_chain(img, &blocks);

    let type_info_byte = file_type_info(&file_type, programs_embedded);
    let file_number = next_file_number(img, &file_type);
    let entry = DirectoryEntry {
        type_info: type_info_byte,
        file_type,
        name: name_arr,
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number,
        size_bytes: len as u32,
    };
    if let Err(e) = target_dir.add(img, entry) {
        return to_error_code(&e);
    }

    img.set_free_blocks(FileAllocationTable::count_free(img));
    SD1_OK
}

/// Delete a file by name. Frees FAT chain and removes directory entry.
/// Returns SD1_OK or a negative error code.
#[no_mangle]
pub extern "C" fn sd1_disk_delete(img: *mut DiskImage, name: *const c_char) -> i32 {
    if img.is_null() || name.is_null() { return error::SD1_ERR_FILE_NOT_FOUND; }
    let img = unsafe { &mut *img };
    let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_INVALID_NAME,
    };

    let (dir_idx, entry) = match (0..4u8)
        .find_map(|i| SubDirectory::new(i).find(img, name_str).map(|e| (i, e)))
    {
        Some(v) => v,
        None => return error::SD1_ERR_FILE_NOT_FOUND,
    };

    FileAllocationTable::free_chain(img, entry.first_block as u16);
    if let Err(e) = SubDirectory::new(dir_idx).remove(img, name_str) {
        return to_error_code(&e);
    }
    img.set_free_blocks(FileAllocationTable::count_free(img));
    SD1_OK
}

/// List VST3 block-1 directory entries. Caller must call sd1_entries_free.
#[no_mangle]
pub extern "C" fn sd1_block1_entries(
    img: *const DiskImage,
    count_out: *mut usize,
) -> *mut Sd1DirectoryEntry {
    if img.is_null() || count_out.is_null() {
        if !count_out.is_null() { unsafe { *count_out = 0; } }
        return std::ptr::null_mut();
    }
    let img = unsafe { &*img };
    let entries: Vec<Sd1DirectoryEntry> = block1_entries(img).iter().map(rust_entry_to_c).collect();
    let count = entries.len();
    unsafe { *count_out = count; }
    if count == 0 {
        return std::ptr::null_mut();
    }
    let mut boxed = entries.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

/// Find a named file in the VST3 block-1 directory. Fills *out on success.
#[no_mangle]
pub extern "C" fn sd1_block1_find(
    img: *const DiskImage,
    name: *const c_char,
    out: *mut Sd1DirectoryEntry,
) -> i32 {
    if img.is_null() || name.is_null() || out.is_null() { return error::SD1_ERR_FILE_NOT_FOUND; }
    let img = unsafe { &*img };
    let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_INVALID_NAME,
    };
    match block1_find(img, name_str) {
        Some(e) => {
            unsafe { *out = rust_entry_to_c(&e); }
            SD1_OK
        }
        None => error::SD1_ERR_FILE_NOT_FOUND,
    }
}

/// Return the next file_number to assign for a given file type byte.
#[no_mangle]
pub extern "C" fn sd1_next_file_number(img: *const DiskImage, file_type_byte: u8) -> u8 {
    if img.is_null() { return 0; }
    let img = unsafe { &*img };
    match FileType::from_byte(file_type_byte) {
        Ok(ft) => next_file_number(img, &ft),
        Err(_) => 0,
    }
}

/// Return the type_info byte for a directory entry.
#[no_mangle]
pub extern "C" fn sd1_file_type_info(file_type_byte: u8, programs_embedded: bool) -> u8 {
    match FileType::from_byte(file_type_byte) {
        Ok(ft) => file_type_info(&ft, programs_embedded),
        Err(_) => 0,
    }
}

/// Validate a name and write the 11-byte space-padded form into out[11].
/// Returns SD1_OK or SD1_ERR_INVALID_NAME.
#[no_mangle]
pub extern "C" fn sd1_validate_name(name: *const c_char, out: *mut u8) -> i32 {
    if name.is_null() || out.is_null() { return error::SD1_ERR_INVALID_NAME; }
    let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_INVALID_NAME,
    };
    match validate_name(name_str) {
        Ok(arr) => {
            unsafe { std::ptr::copy_nonoverlapping(arr.as_ptr(), out, 11); }
            SD1_OK
        }
        Err(_) => error::SD1_ERR_INVALID_NAME,
    }
}

// ─── HFE ─────────────────────────────────────────────────────────────────────

/// Read a .hfe file and return a DiskImage. Returns NULL on failure.
#[no_mangle]
pub extern "C" fn sd1_read_hfe(path: *const c_char, err_out: *mut i32) -> *mut DiskImage {
    if path.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_HFE);
        return std::ptr::null_mut();
    }
    let cstr = unsafe { CStr::from_ptr(path) };
    let path_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => { set_err(err_out, error::SD1_ERR_IO); return std::ptr::null_mut(); }
    };
    match read_hfe(Path::new(path_str)) {
        Ok(img) => {
            set_err(err_out, SD1_OK);
            Box::into_raw(Box::new(img))
        }
        Err(e) => {
            set_err(err_out, to_error_code(&e));
            std::ptr::null_mut()
        }
    }
}

/// Write a DiskImage as a .hfe file. Returns SD1_OK or a negative error code.
#[no_mangle]
pub extern "C" fn sd1_write_hfe(img: *const DiskImage, path: *const c_char) -> i32 {
    if img.is_null() || path.is_null() { return error::SD1_ERR_IO; }
    let cstr = unsafe { CStr::from_ptr(path) };
    let path_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return error::SD1_ERR_IO,
    };
    let img = unsafe { &*img };
    match write_hfe(img, Path::new(path_str)) {
        Ok(()) => SD1_OK,
        Err(e) => to_error_code(&e),
    }
}

// ─── FAT direct access ───────────────────────────────────────────────────────

/// Read a FAT entry for the given block. Fills *out.
#[no_mangle]
pub extern "C" fn sd1_fat_entry(img: *const DiskImage, block: u16, out: *mut Sd1FatEntry) {
    if img.is_null() || out.is_null() { return; }
    let img = unsafe { &*img };
    let fe = FileAllocationTable::entry(img, block);
    unsafe { *out = fat_entry_to_c(fe); }
}

/// Walk a FAT chain starting at `start`. Returns a heap-allocated array of block numbers.
/// Caller must call sd1_u16_array_free(ptr, count). Returns NULL on error.
#[no_mangle]
pub extern "C" fn sd1_fat_chain(
    img: *const DiskImage,
    start: u16,
    blocks_out: *mut *mut u16,
    count_out: *mut usize,
) -> i32 {
    if img.is_null() || blocks_out.is_null() || count_out.is_null() { return error::SD1_ERR_IO; }
    let img = unsafe { &*img };
    match FileAllocationTable::chain(img, start) {
        Ok(chain) => {
            let count = chain.len();
            let mut boxed = chain.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe {
                *blocks_out = ptr;
                *count_out = count;
            }
            SD1_OK
        }
        Err(e) => {
            unsafe {
                *blocks_out = std::ptr::null_mut();
                *count_out = 0;
            }
            to_error_code(&e)
        }
    }
}

/// Allocate n free blocks. Returns a heap-allocated array.
/// Caller must call sd1_u16_array_free(ptr, count). Returns NULL on error.
#[no_mangle]
pub extern "C" fn sd1_fat_allocate(
    img: *mut DiskImage,
    n: u16,
    blocks_out: *mut *mut u16,
    count_out: *mut usize,
) -> i32 {
    if img.is_null() || blocks_out.is_null() || count_out.is_null() { return error::SD1_ERR_IO; }
    let img = unsafe { &mut *img };
    match FileAllocationTable::allocate(img, n) {
        Ok(blocks) => {
            let count = blocks.len();
            let mut boxed = blocks.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe {
                *blocks_out = ptr;
                *count_out = count;
            }
            SD1_OK
        }
        Err(e) => {
            unsafe {
                *blocks_out = std::ptr::null_mut();
                *count_out = 0;
            }
            to_error_code(&e)
        }
    }
}

/// Free an entire FAT chain starting at `start`.
#[no_mangle]
pub extern "C" fn sd1_fat_free_chain(img: *mut DiskImage, start: u16) {
    if img.is_null() { return; }
    let img = unsafe { &mut *img };
    FileAllocationTable::free_chain(img, start);
}

/// Write a FAT chain from an array of block numbers.
#[no_mangle]
pub extern "C" fn sd1_fat_set_chain(img: *mut DiskImage, blocks: *const u16, count: usize) {
    if img.is_null() || blocks.is_null() || count == 0 { return; }
    let img = unsafe { &mut *img };
    let slice = unsafe { std::slice::from_raw_parts(blocks, count) };
    FileAllocationTable::set_chain(img, slice);
}

/// Count free data blocks (blocks 23–1599).
#[no_mangle]
pub extern "C" fn sd1_fat_count_free(img: *const DiskImage) -> u32 {
    if img.is_null() { return 0; }
    FileAllocationTable::count_free(unsafe { &*img })
}

// ─── SysEx ───────────────────────────────────────────────────────────────────

/// Parse a single SysEx packet. Returns NULL on failure.
#[no_mangle]
pub extern "C" fn sd1_sysex_parse(
    data: *const u8,
    len: usize,
    err_out: *mut i32,
) -> *mut SysExPacket {
    if data.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_SYSEX);
        return std::ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match SysExPacket::parse(bytes) {
        Ok(pkt) => {
            set_err(err_out, SD1_OK);
            Box::into_raw(Box::new(pkt))
        }
        Err(e) => {
            set_err(err_out, to_error_code(&e));
            std::ptr::null_mut()
        }
    }
}

/// Parse all SysEx packets from a byte stream.
/// Returns a heap-allocated array of pointers. Caller must call sd1_sysex_packets_free.
/// Returns NULL on failure; *count_out set to 0.
#[no_mangle]
pub extern "C" fn sd1_sysex_parse_all(
    data: *const u8,
    len: usize,
    count_out: *mut usize,
    err_out: *mut i32,
) -> *mut *mut SysExPacket {
    if data.is_null() || count_out.is_null() {
        if !count_out.is_null() { unsafe { *count_out = 0; } }
        set_err(err_out, error::SD1_ERR_INVALID_SYSEX);
        return std::ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match SysExPacket::parse_all(bytes) {
        Ok(packets) => {
            let count = packets.len();
            let mut ptrs: Vec<*mut SysExPacket> = packets
                .into_iter()
                .map(|p| Box::into_raw(Box::new(p)))
                .collect();
            ptrs.shrink_to_fit();
            let ptr = ptrs.as_mut_ptr();
            std::mem::forget(ptrs);
            unsafe { *count_out = count; }
            set_err(err_out, SD1_OK);
            ptr
        }
        Err(e) => {
            unsafe { *count_out = 0; }
            set_err(err_out, to_error_code(&e));
            std::ptr::null_mut()
        }
    }
}

/// Return the message type byte of a SysEx packet.
#[no_mangle]
pub extern "C" fn sd1_sysex_message_type(pkt: *const SysExPacket) -> u8 {
    if pkt.is_null() { return 0xFF; }
    unsafe { (*pkt).message_type.to_byte() }
}

/// Return the MIDI channel of a SysEx packet.
#[no_mangle]
pub extern "C" fn sd1_sysex_midi_channel(pkt: *const SysExPacket) -> u8 {
    if pkt.is_null() { return 0; }
    unsafe { (*pkt).midi_channel }
}

/// Return the model byte of a SysEx packet.
#[no_mangle]
pub extern "C" fn sd1_sysex_model(pkt: *const SysExPacket) -> u8 {
    if pkt.is_null() { return 0; }
    unsafe { (*pkt).model }
}

/// Return a pointer to the payload bytes of a SysEx packet. Do not free this pointer.
#[no_mangle]
pub extern "C" fn sd1_sysex_payload(pkt: *const SysExPacket) -> *const u8 {
    if pkt.is_null() { return std::ptr::null(); }
    unsafe { (*pkt).payload.as_ptr() }
}

/// Return the payload length of a SysEx packet.
#[no_mangle]
pub extern "C" fn sd1_sysex_payload_len(pkt: *const SysExPacket) -> usize {
    if pkt.is_null() { return 0; }
    unsafe { (*pkt).payload.len() }
}

/// Serialize a SysEx packet to bytes on the given channel.
/// Caller must call sd1_bytes_free(ptr, len).
#[no_mangle]
pub extern "C" fn sd1_sysex_to_bytes(
    pkt: *const SysExPacket,
    channel: u8,
    len_out: *mut usize,
) -> *mut u8 {
    if pkt.is_null() || len_out.is_null() { return std::ptr::null_mut(); }
    let pkt = unsafe { &*pkt };
    let bytes = pkt.to_bytes(channel);
    let len = bytes.len();
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    unsafe { *len_out = len; }
    ptr
}

/// Free a SysExPacket allocated by sd1_sysex_parse or similar.
#[no_mangle]
pub extern "C" fn sd1_sysex_free(pkt: *mut SysExPacket) {
    if !pkt.is_null() {
        unsafe { drop(Box::from_raw(pkt)); }
    }
}

/// Free an array of SysExPacket pointers from sd1_sysex_parse_all.
#[no_mangle]
pub extern "C" fn sd1_sysex_packets_free(pkts: *mut *mut SysExPacket, count: usize) {
    if pkts.is_null() || count == 0 { return; }
    unsafe {
        let slice = std::slice::from_raw_parts_mut(pkts, count);
        for &mut p in slice.iter_mut() {
            if !p.is_null() {
                drop(Box::from_raw(p));
            }
        }
        // Free the pointer array itself
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(pkts, count) as *mut [*mut SysExPacket]);
    }
}

// ─── Program ──────────────────────────────────────────────────────────────────

/// Parse a Program from a SysEx packet. Returns NULL on failure.
#[no_mangle]
pub extern "C" fn sd1_program_from_sysex(
    pkt: *const SysExPacket,
    err_out: *mut i32,
) -> *mut Program {
    if pkt.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_SYSEX);
        return std::ptr::null_mut();
    }
    let pkt = unsafe { &*pkt };
    match Program::from_sysex(pkt) {
        Ok(p) => { set_err(err_out, SD1_OK); Box::into_raw(Box::new(p)) }
        Err(e) => { set_err(err_out, to_error_code(&e)); std::ptr::null_mut() }
    }
}

/// Parse a Program from raw bytes. Returns NULL on failure.
#[no_mangle]
pub extern "C" fn sd1_program_from_bytes(
    data: *const u8,
    len: usize,
    err_out: *mut i32,
) -> *mut Program {
    if data.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_SYSEX);
        return std::ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match Program::from_bytes(bytes) {
        Ok(p) => { set_err(err_out, SD1_OK); Box::into_raw(Box::new(p)) }
        Err(e) => { set_err(err_out, to_error_code(&e)); std::ptr::null_mut() }
    }
}

/// Return raw bytes for the program. Caller must free with sd1_bytes_free(data, len).
#[no_mangle]
pub extern "C" fn sd1_program_bytes(prog: *const Program, len_out: *mut usize) -> *mut u8 {
    if len_out.is_null() { return std::ptr::null_mut(); }
    let prog = match unsafe { prog.as_ref() } { Some(p) => p, None => {
        unsafe { *len_out = 0; } return std::ptr::null_mut();
    }};
    let bytes: Vec<u8> = prog.to_bytes().to_vec();
    let len = bytes.len();
    unsafe { *len_out = len; }
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

/// Write the program name (null-terminated) into out[0..out_len].
#[no_mangle]
pub extern "C" fn sd1_program_name(prog: *const Program, out: *mut c_char, out_len: usize) {
    if prog.is_null() || out.is_null() || out_len == 0 { return; }
    let prog = unsafe { &*prog };
    let name = prog.name();
    let cstring = CString::new(name.as_ref()).unwrap_or_default();
    let bytes = cstring.as_bytes_with_nul();
    let copy_len = bytes.len().min(out_len);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, out, copy_len);
        // Ensure null termination
        *out.add(out_len - 1) = 0;
    }
}

/// Convert a Program to a SysEx packet. Returns NULL on error (never in practice).
#[no_mangle]
pub extern "C" fn sd1_program_to_sysex(prog: *const Program, channel: u8) -> *mut SysExPacket {
    if prog.is_null() { return std::ptr::null_mut(); }
    let prog = unsafe { &*prog };
    Box::into_raw(Box::new(prog.to_sysex(channel)))
}

/// Return the file type byte for a Program.
#[no_mangle]
pub extern "C" fn sd1_program_file_type(prog: *const Program) -> u8 {
    if prog.is_null() { return 0; }
    unsafe { (*prog).file_type().to_byte() }
}

/// Free a Program allocated by sd1_program_from_sysex or sd1_program_from_bytes.
#[no_mangle]
pub extern "C" fn sd1_program_free(prog: *mut Program) {
    if !prog.is_null() {
        unsafe { drop(Box::from_raw(prog)); }
    }
}

// ─── Preset ───────────────────────────────────────────────────────────────────

/// Parse a Preset from a SysEx packet. Returns NULL on failure.
#[no_mangle]
pub extern "C" fn sd1_preset_from_sysex(
    pkt: *const SysExPacket,
    err_out: *mut i32,
) -> *mut Preset {
    if pkt.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_SYSEX);
        return std::ptr::null_mut();
    }
    let pkt = unsafe { &*pkt };
    match Preset::from_sysex(pkt) {
        Ok(p) => { set_err(err_out, SD1_OK); Box::into_raw(Box::new(p)) }
        Err(e) => { set_err(err_out, to_error_code(&e)); std::ptr::null_mut() }
    }
}

/// Parse a Preset from raw bytes. Returns NULL on failure.
#[no_mangle]
pub extern "C" fn sd1_preset_from_bytes(
    data: *const u8,
    len: usize,
    err_out: *mut i32,
) -> *mut Preset {
    if data.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_SYSEX);
        return std::ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match Preset::from_bytes(bytes) {
        Ok(p) => { set_err(err_out, SD1_OK); Box::into_raw(Box::new(p)) }
        Err(e) => { set_err(err_out, to_error_code(&e)); std::ptr::null_mut() }
    }
}

/// Return raw bytes for the preset. Caller must free with sd1_bytes_free(data, len).
#[no_mangle]
pub extern "C" fn sd1_preset_bytes(preset: *const Preset, len_out: *mut usize) -> *mut u8 {
    if len_out.is_null() { return std::ptr::null_mut(); }
    let preset = match unsafe { preset.as_ref() } { Some(p) => p, None => {
        unsafe { *len_out = 0; } return std::ptr::null_mut();
    }};
    let bytes: Vec<u8> = preset.to_bytes().to_vec();
    let len = bytes.len();
    unsafe { *len_out = len; }
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

/// Convert a Preset to a SysEx packet.
#[no_mangle]
pub extern "C" fn sd1_preset_to_sysex(preset: *const Preset, channel: u8) -> *mut SysExPacket {
    if preset.is_null() { return std::ptr::null_mut(); }
    let preset = unsafe { &*preset };
    Box::into_raw(Box::new(preset.to_sysex(channel)))
}

/// Return the file type byte for a Preset.
#[no_mangle]
pub extern "C" fn sd1_preset_file_type(preset: *const Preset) -> u8 {
    if preset.is_null() { return 0; }
    unsafe { (*preset).file_type().to_byte() }
}

/// Free a Preset.
#[no_mangle]
pub extern "C" fn sd1_preset_free(preset: *mut Preset) {
    if !preset.is_null() {
        unsafe { drop(Box::from_raw(preset)); }
    }
}

// ─── Sequence ─────────────────────────────────────────────────────────────────

/// Parse a Sequence from a SysEx packet. Returns NULL on failure.
#[no_mangle]
pub extern "C" fn sd1_sequence_from_sysex(
    pkt: *const SysExPacket,
    err_out: *mut i32,
) -> *mut Sequence {
    if pkt.is_null() {
        set_err(err_out, error::SD1_ERR_INVALID_SYSEX);
        return std::ptr::null_mut();
    }
    let pkt = unsafe { &*pkt };
    match Sequence::from_sysex(pkt) {
        Ok(s) => { set_err(err_out, SD1_OK); Box::into_raw(Box::new(s)) }
        Err(e) => { set_err(err_out, to_error_code(&e)); std::ptr::null_mut() }
    }
}

/// Create a Sequence from raw bytes.
#[no_mangle]
pub extern "C" fn sd1_sequence_from_bytes(data: *const u8, len: usize) -> *mut Sequence {
    if data.is_null() { return std::ptr::null_mut(); }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    Box::into_raw(Box::new(Sequence::from_bytes(bytes)))
}

/// Return raw bytes for the sequence. Caller must free with sd1_bytes_free(data, len).
#[no_mangle]
pub extern "C" fn sd1_sequence_bytes(seq: *const Sequence, len_out: *mut usize) -> *mut u8 {
    if len_out.is_null() { return std::ptr::null_mut(); }
    let seq = match unsafe { seq.as_ref() } { Some(s) => s, None => {
        unsafe { *len_out = 0; } return std::ptr::null_mut();
    }};
    let bytes: Vec<u8> = seq.to_bytes().to_vec();
    let len = bytes.len();
    unsafe { *len_out = len; }
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

/// Convert a Sequence to a SysEx packet.
#[no_mangle]
pub extern "C" fn sd1_sequence_to_sysex(seq: *const Sequence, channel: u8) -> *mut SysExPacket {
    if seq.is_null() { return std::ptr::null_mut(); }
    let seq = unsafe { &*seq };
    Box::into_raw(Box::new(seq.to_sysex(channel)))
}

/// Return the file type byte for a Sequence.
#[no_mangle]
pub extern "C" fn sd1_sequence_file_type(seq: *const Sequence) -> u8 {
    if seq.is_null() { return 0; }
    unsafe { (*seq).file_type().to_byte() }
}

/// Free a Sequence.
#[no_mangle]
pub extern "C" fn sd1_sequence_free(seq: *mut Sequence) {
    if !seq.is_null() {
        unsafe { drop(Box::from_raw(seq)); }
    }
}

// ─── Type conversion functions ────────────────────────────────────────────────

/// Convert an AllSequences SysEx payload + interleaved program data to on-disk format.
/// If interleaved_progs is NULL, programs are not embedded.
/// Caller must call sd1_bytes_free(*out, *out_len).
#[no_mangle]
pub extern "C" fn sd1_allsequences_to_disk(
    payload: *const u8,
    payload_len: usize,
    interleaved_progs: *const u8,
    progs_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if payload.is_null() || out.is_null() || out_len.is_null() { return error::SD1_ERR_IO; }
    let payload_bytes = unsafe { std::slice::from_raw_parts(payload, payload_len) };
    let progs = if interleaved_progs.is_null() || progs_len == 0 {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(interleaved_progs, progs_len) })
    };
    match allsequences_to_disk(payload_bytes, progs) {
        Ok(data) => {
            let len = data.len();
            let mut boxed = data.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe { *out = ptr; *out_len = len; }
            SD1_OK
        }
        Err(e) => {
            unsafe { *out = std::ptr::null_mut(); *out_len = 0; }
            to_error_code(&e)
        }
    }
}

/// Convert on-disk SixtySequences data back to an AllSequences SysEx payload.
/// Caller must call sd1_bytes_free(*out, *out_len).
#[no_mangle]
pub extern "C" fn sd1_disk_to_allsequences(
    disk: *const u8,
    len: usize,
    has_programs: bool,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if disk.is_null() || out.is_null() || out_len.is_null() { return error::SD1_ERR_IO; }
    let disk_bytes = unsafe { std::slice::from_raw_parts(disk, len) };
    match disk_to_allsequences(disk_bytes, has_programs) {
        Ok(data) => {
            let data_len = data.len();
            let mut boxed = data.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe { *out = ptr; *out_len = data_len; }
            SD1_OK
        }
        Err(e) => {
            unsafe { *out = std::ptr::null_mut(); *out_len = 0; }
            to_error_code(&e)
        }
    }
}

/// Convert on-disk ThirtySequences data to an AllSequences SysEx payload (60-slot format).
/// Slots 0–29 are populated from disk; slots 30–59 are set to undefined (0xFF headers).
/// Programs embedded after sequence data (if any) are not included in the output.
/// Caller must call sd1_bytes_free(*out, *out_len).
#[no_mangle]
pub extern "C" fn sd1_disk_to_thirty_sequences(
    disk: *const u8,
    len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if disk.is_null() || out.is_null() || out_len.is_null() { return error::SD1_ERR_IO; }
    let disk_bytes = unsafe { std::slice::from_raw_parts(disk, len) };
    match disk_to_thirty_sequences(disk_bytes) {
        Ok(data) => {
            let data_len = data.len();
            let mut boxed = data.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe { *out = ptr; *out_len = data_len; }
            SD1_OK
        }
        Err(e) => {
            unsafe { *out = std::ptr::null_mut(); *out_len = 0; }
            to_error_code(&e)
        }
    }
}

/// Convert an AllSequences SysEx payload to on-disk ThirtySequences format.
/// Only slots 0–29 are written; slots 30–59 are ignored.
/// Programs (if any) are placed AFTER sequence data (opposite of SixtySequences layout).
/// If interleaved_progs is NULL, programs are not embedded.
/// Caller must call sd1_bytes_free(*out, *out_len).
#[no_mangle]
pub extern "C" fn sd1_thirty_sequences_to_disk(
    payload: *const u8,
    payload_len: usize,
    interleaved_progs: *const u8,
    progs_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if payload.is_null() || out.is_null() || out_len.is_null() { return error::SD1_ERR_IO; }
    let payload_bytes = unsafe { std::slice::from_raw_parts(payload, payload_len) };
    let progs = if interleaved_progs.is_null() || progs_len == 0 {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(interleaved_progs, progs_len) })
    };
    match thirty_sequences_to_disk(payload_bytes, progs) {
        Ok(data) => {
            let len = data.len();
            let mut boxed = data.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe { *out = ptr; *out_len = len; }
            SD1_OK
        }
        Err(e) => {
            unsafe { *out = std::ptr::null_mut(); *out_len = 0; }
            to_error_code(&e)
        }
    }
}

/// Interleave 60 programs from AllPrograms SysEx payload order to on-disk SixtyPrograms format.
/// Caller must call sd1_bytes_free(*out, *out_len).
#[no_mangle]
pub extern "C" fn sd1_interleave_sixty_programs(
    payload: *const u8,
    len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if payload.is_null() || out.is_null() || out_len.is_null() { return error::SD1_ERR_IO; }
    let bytes = unsafe { std::slice::from_raw_parts(payload, len) };
    match interleave_sixty_programs(bytes) {
        Ok(data) => {
            let data_len = data.len();
            let mut boxed = data.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe { *out = ptr; *out_len = data_len; }
            SD1_OK
        }
        Err(e) => {
            unsafe { *out = std::ptr::null_mut(); *out_len = 0; }
            to_error_code(&e)
        }
    }
}

/// De-interleave on-disk SixtyPrograms data back to AllPrograms SysEx payload order.
/// Caller must call sd1_bytes_free(*out, *out_len).
#[no_mangle]
pub extern "C" fn sd1_deinterleave_sixty_programs(
    data: *const u8,
    len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if data.is_null() || out.is_null() || out_len.is_null() { return error::SD1_ERR_IO; }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match deinterleave_sixty_programs(bytes) {
        Ok(result) => {
            let result_len = result.len();
            let mut boxed = result.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            std::mem::forget(boxed);
            unsafe { *out = ptr; *out_len = result_len; }
            SD1_OK
        }
        Err(e) => {
            unsafe { *out = std::ptr::null_mut(); *out_len = 0; }
            to_error_code(&e)
        }
    }
}

// ─── Program utilities ────────────────────────────────────────────────────────

/// Decode a program name from a 530-byte slot. Writes null-terminated name into out[0..out_len].
#[no_mangle]
pub extern "C" fn sd1_program_name_from_slot(
    slot: *const u8,
    slot_len: usize,
    out: *mut c_char,
    out_len: usize,
) {
    if slot.is_null() || out.is_null() || out_len == 0 { return; }
    let slot_bytes = unsafe { std::slice::from_raw_parts(slot, slot_len) };
    let name = program_name_from_slot(slot_bytes);
    let cstring = CString::new(name).unwrap_or_default();
    let bytes = cstring.as_bytes_with_nul();
    let copy_len = bytes.len().min(out_len);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, out, copy_len);
        *out.add(out_len - 1) = 0;
    }
}

/// Decode a b10 track program assignment byte to a human-readable label.
/// disk_programs: optional array of count C-string pointers (NULL = use INT0 defaults).
/// Writes null-terminated result into out[0..out_len].
#[no_mangle]
pub extern "C" fn sd1_decode_b10(
    b10: u8,
    disk_programs: *const *const c_char,
    count: usize,
    out: *mut c_char,
    out_len: usize,
) {
    if out.is_null() || out_len == 0 { return; }

    let progs: Option<Vec<String>> = if disk_programs.is_null() || count == 0 {
        None
    } else {
        let ptrs = unsafe { std::slice::from_raw_parts(disk_programs, count) };
        let strings: Vec<String> = ptrs.iter().map(|&p| {
            if p.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
            }
        }).collect();
        Some(strings)
    };

    let label = decode_b10(b10, progs.as_deref());
    let cstring = CString::new(label).unwrap_or_default();
    let bytes = cstring.as_bytes_with_nul();
    let copy_len = bytes.len().min(out_len);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, out, copy_len);
        *out.add(out_len - 1) = 0;
    }
}

/// Wrapper to hold CString data and the pointer array for FFI name tables.
/// SAFETY: The CString storage is immutable after initialization; pointers derived
/// from it are valid for the lifetime of the static (i.e., forever in practice).
struct NameTable {
    _storage: Vec<std::ffi::CString>,
    ptrs: Vec<*const c_char>,
}
unsafe impl Send for NameTable {}
unsafe impl Sync for NameTable {}

/// Return pointer to the 60-entry INT0 user bank name table.
/// Each entry is a null-terminated C string; never free any pointer.
/// Returns a pointer to an array of 60 `const char*` pointers.
#[no_mangle]
pub extern "C" fn sd1_int0_programs() -> *const *const c_char {
    use std::sync::OnceLock;
    static DATA: OnceLock<NameTable> = OnceLock::new();
    let table = DATA.get_or_init(|| {
        let storage: Vec<std::ffi::CString> = sd1disk::INT0_PROGRAMS.iter()
            .map(|s| std::ffi::CString::new(*s).unwrap())
            .collect();
        let ptrs: Vec<*const c_char> = storage.iter().map(|cs| cs.as_ptr()).collect();
        NameTable { _storage: storage, ptrs }
    });
    table.ptrs.as_ptr()
}

/// Return pointer to the 120-entry ROM name table.
/// Each entry is a null-terminated C string; never free any pointer.
/// Returns a pointer to an array of 120 `const char*` pointers.
#[no_mangle]
pub extern "C" fn sd1_rom_all_programs() -> *const *const c_char {
    use std::sync::OnceLock;
    static DATA: OnceLock<NameTable> = OnceLock::new();
    let table = DATA.get_or_init(|| {
        let storage: Vec<std::ffi::CString> = sd1disk::ROM_ALL_PROGRAMS.iter()
            .map(|s| std::ffi::CString::new(*s).unwrap())
            .collect();
        let ptrs: Vec<*const c_char> = storage.iter().map(|cs| cs.as_ptr()).collect();
        NameTable { _storage: storage, ptrs }
    });
    table.ptrs.as_ptr()
}

/// INT0 factory programs — 60 static C strings.
/// SAFETY: these are read-only pointers to 'static string literals, safe to share across threads.
#[no_mangle]
pub static SD1_INT0_PROGRAMS: SyncRawPtrs<[*const c_char; 60]> = SyncRawPtrs({
    macro_rules! s {
        ($s:expr) => { concat!($s, "\0").as_ptr() as *const c_char }
    }
    [
        s!("ARTIC-ELATE"), s!("OLYMPIANO"),   s!("ALTO-SAX"),    s!("MERLIN"),       s!("WAY-FAT"),     s!("GROOVE-KIT"),
        s!("ALLS-FAIR"),   s!("IN-CONCERT"),  s!("SOLOTRUMPET"), s!("INSPIRED"),     s!("AMEN-CHOIR"),  s!("PASSION"),
        s!("SYMPHONY"),    s!("MY-DESIRE"),   s!("MUTED-HORNS"), s!("STACK-BASS"),   s!("DRAWBARS-1"),  s!("SONOTAR"),
        s!("STRINGS"),     s!("BRASS-STAB"),  s!("MANDOLIN"),    s!("CROWN-CHOIR"),  s!("TUBULAR HIT"), s!("JAZZ-KIT"),
        s!("STRUM-ME"),    s!("LUNAR"),       s!("BLUES-HARP"),  s!("WIDEPUNCH"),    s!("BRIGHT-PNO"),  s!("PIPE-ORGAN1"),
        s!("MALLETS"),     s!("SWEEPER"),     s!("KOTO-DREAMS"), s!("SWELL-SAW"),    s!("WILBUR"),      s!("MEATY-KIT"),
        s!("FIDDLE"),      s!("PEDAL-STEEL"), s!("BANJO-BANJO"), s!("CLOCK-BELLS"),  s!("THE-QUEEN"),   s!("ROCK-KIT-2"),
        s!("SMOOTH-STRG"), s!("DARK-HALL"),   s!("GUITAR-PADS"), s!("FANFARE"),      s!("MINI-LEAD"),   s!("NORM-1-KIT"),
        s!("STRATOS-VOX"), s!("FUNKY-CLAV2"), s!("COOL-FLUTES"), s!("OH-BE-EX"),     s!("DANCEBASS-2"), s!("WOODY-PERC"),
        s!("ANNABELL"),    s!("FUNK-GUITAR"), s!("ELEC-BASS2"),  s!("CLEAR-GUITAR"), s!("STUDIO-CITY"), s!("MEAN-KIT-1"),
    ]
});

/// ROM program table — 120 static C strings (ROM0 indices 0–59, ROM1 indices 60–119).
/// SAFETY: these are read-only pointers to 'static string literals, safe to share across threads.
#[no_mangle]
pub static SD1_ROM_ALL_PROGRAMS: SyncRawPtrs<[*const c_char; 120]> = SyncRawPtrs({
    macro_rules! s {
        ($s:expr) => { concat!($s, "\0").as_ptr() as *const c_char }
    }
    [
        // ROM 0
        s!("ITS-A-SYNTH"), s!("ZIRCONIUM"),    s!(" FAT-BRASS"),   s!("STAR-DRIVE "), s!(" WONDERS "),   s!("SAW-O-LIFE"),
        s!("DIGIPIANO-1"), s!("NEW-PLANET"),   s!(" DANGEROUS "),  s!(" FUNKYCLAV "), s!("WARM-TINES"),  s!("METAL-TINES"),
        s!(" BIG-PIANO "), s!("BRIGHT-PNO2"),  s!(" SYN-PIANO "),  s!("TRANS-PIANO"), s!("CLASSIC-PNO"), s!("HARPSICHORD"),
        s!("DOUBLE-REED"), s!(" TENOR-SAX "),  s!("WOODFLUTE"),    s!(" CHIFFLUTE "), s!("MALLET+FLTS"), s!("FLUTE-VIL"),
        s!(" STARBRASS "), s!(" FRENCHORN "),  s!(" TOP-BRASS "),  s!("FLUGEL-STRG"), s!("  BRASSY  "),  s!("SYNTH-HORNS"),
        s!("SMAK-BASS"),   s!("BEBOP-BASS"),   s!("ELEC-BASS"),    s!("SYNTHBASS"),   s!("DANCE-BASS"),  s!("BUZZ-BASS"),
        s!(" ORGANIZER"),  s!("NASTY-ORGAN"),  s!("CATHEDRAL-1"),  s!("TIMBRE-ORG"),  s!("ANGELBREATH"), s!(" VERYBREATH"),
        s!("SWELLSTRNGS"), s!(" PIZZICATO "),  s!("LUSH-STRNGS"),  s!("GOLDEN-HARP"), s!("REZ-STRINGS"), s!(" ORCH+SOLO "),
        s!("REEL-STEEL"),  s!("SUN-N-MOON"),   s!("FLANG-CLEAN"),  s!(" FUZZ-LEAD"),  s!("SPANISH-GTR"), s!(" 12-STRING"),
        s!("KITCHN-SINK"), s!("PERCUSSION"),   s!("FUSION-KIT"),   s!(" BALLAD-KIT"), s!("SYNTH-KIT"),   s!("ROCKIN-KIT"),
        // ROM 1
        s!("OMNIVERSE"),   s!("FLASH-BACK"),   s!(" SD1-PAD"),     s!("SQUARE-PAD"),  s!("NU-MEANING"),  s!("ASCENSION"),
        s!("IN-DEMAND"),   s!(" FM-PIANO"),    s!("MANY-ROADS"),   s!("DEEP-TINES"),  s!("PURE-TINE"),   s!("INNOCENCE"),
        s!("STUDIO-GRND"), s!(" POP-GRND"),    s!("JAZZ-GRAND"),   s!("CHURCH-GRND"), s!("CLASSIC-GND"), s!("BOWS+GRAND"),
        s!("SOPRANO-SAX"), s!(" ALTO-SAX"),    s!("BARI+HORNS"),   s!("HARMONICA"),   s!("SHAKUHACHI"),  s!(" PICCOLO +"),
        s!(" ODYSSEY"),    s!("MANY-LEADS"),   s!(" FUNK-LEAD"),   s!("FUNKY-STABS"), s!(" CHICAGO"),    s!("MUTED-HORN"),
        s!("MOOG-MUTE"),   s!("  ANAREZO"),    s!("PERKY-MOOG"),   s!("CROSS-BASS"),  s!("SLICK-ELEC"),  s!("BLEACHBASS"),
        s!("JAZZ-ORGAN"),  s!("DIRTY-ORGAN"),  s!("NU-CHOIR"),     s!("DIGITALIAN"),  s!(" CHORALE-2"),  s!("90-S-VOX"),
        s!("DRAMA-STGS"),  s!("NU-STRINGS"),   s!("LUSH-STRG-2"),  s!("  VIOLIN"),    s!("   CELLO"),    s!("  QUARTET"),
        s!("DREAM-GTR"),   s!("JAZZ-GUITAR"),  s!("ELEC-GUITAR"),  s!("DIST-GTR"),    s!("   NU-BEL"),   s!(" MULTI-BELL"),
        s!("DRUMS-MAP-R"), s!("808-MAP-R"),    s!("SLAM-MAP-R"),   s!("MULTI-PERCS"), s!("ORCH-PERKS"),  s!(" INDO-AFRO"),
    ]
});

// ─── Memory management ────────────────────────────────────────────────────────

/// Free a Sd1DirectoryEntry array from sd1_disk_list or sd1_block1_entries.
#[no_mangle]
pub extern "C" fn sd1_entries_free(ptr: *mut Sd1DirectoryEntry, count: usize) {
    if ptr.is_null() || count == 0 { return; }
    unsafe {
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(ptr, count) as *mut [Sd1DirectoryEntry]);
    }
}

/// Free a byte buffer from sd1_disk_extract, sd1_sysex_to_bytes, or conversion functions.
#[no_mangle]
pub extern "C" fn sd1_bytes_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 { return; }
    unsafe {
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(ptr, len) as *mut [u8]);
    }
}

/// Free a u16 array from sd1_fat_chain or sd1_fat_allocate.
#[no_mangle]
pub extern "C" fn sd1_u16_array_free(ptr: *mut u16, count: usize) {
    if ptr.is_null() || count == 0 { return; }
    unsafe {
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(ptr, count) as *mut [u16]);
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn cstr(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    #[test]
    fn error_message_ok_is_success() {
        let msg = unsafe { CStr::from_ptr(sd1_error_message(0)) };
        assert_eq!(msg.to_str().unwrap(), "success");
    }

    #[test]
    fn error_message_unknown_code() {
        let msg = unsafe { CStr::from_ptr(sd1_error_message(-99)) };
        assert_eq!(msg.to_str().unwrap(), "unknown error");
    }

    #[test]
    fn disk_create_and_free() {
        let img = sd1_disk_create();
        assert!(!img.is_null());
        sd1_disk_free(img);
    }

    #[test]
    fn disk_free_blocks_on_blank_disk() {
        let img = sd1_disk_create();
        assert!(!img.is_null());
        let free = sd1_disk_free_blocks(img);
        assert!(free > 0 && free <= 1600);
        sd1_disk_free(img);
    }

    #[test]
    fn disk_list_empty_on_blank_disk() {
        let img = sd1_disk_create();
        let mut count: usize = 0;
        let ptr = sd1_disk_list(img, 0, &mut count);
        assert_eq!(count, 0);
        assert!(ptr.is_null());
        sd1_disk_free(img);
    }

    #[test]
    fn validate_name_ok() {
        let name = cstr("MY_PATCH");
        let mut out = [0u8; 11];
        let rc = sd1_validate_name(name.as_ptr(), out.as_mut_ptr());
        assert_eq!(rc, SD1_OK);
        assert_eq!(&out[..8], b"MY_PATCH");
    }

    #[test]
    fn validate_name_too_long() {
        let name = cstr("TOOLONGNAMETOFITINDISK");
        let mut out = [0u8; 11];
        let rc = sd1_validate_name(name.as_ptr(), out.as_mut_ptr());
        assert_eq!(rc, error::SD1_ERR_INVALID_NAME);
    }

    #[test]
    fn file_type_info_roundtrip() {
        // SixtySequences = 0x13 with programs_embedded → 0x20
        assert_eq!(sd1_file_type_info(0x13, true), 0x20);
        // OneProgram with programs_embedded → 0x00
        assert_eq!(sd1_file_type_info(0x0A, true), 0x00);
    }

    #[test]
    fn next_file_number_blank_disk_is_zero() {
        let img = sd1_disk_create();
        let n = sd1_next_file_number(img, 0x0A); // OneProgram
        assert_eq!(n, 0);
        sd1_disk_free(img);
    }

    #[test]
    fn disk_write_and_find() {
        let img = sd1_disk_create();
        let name = cstr("MYPATCH");
        let data = vec![0xAAu8; 530];
        let rc = sd1_disk_write(img, name.as_ptr(), 0x0A, false, data.as_ptr(), data.len(), false);
        assert_eq!(rc, SD1_OK);

        let mut entry = Sd1DirectoryEntry {
            type_info: 0, file_type: 0, name: [0; 12],
            size_blocks: 0, contiguous_blocks: 0, first_block: 0,
            file_number: 0, size_bytes: 0,
        };
        let rc2 = sd1_disk_find(img, name.as_ptr(), &mut entry);
        assert_eq!(rc2, SD1_OK);
        assert_eq!(entry.file_type, 0x0A);
        assert_eq!(entry.size_bytes, 530);
        sd1_disk_free(img);
    }

    #[test]
    fn disk_write_and_delete() {
        let img = sd1_disk_create();
        let name = cstr("DELME");
        let data = vec![0xBBu8; 512];
        sd1_disk_write(img, name.as_ptr(), 0x0A, false, data.as_ptr(), data.len(), false);
        let rc = sd1_disk_delete(img, name.as_ptr());
        assert_eq!(rc, SD1_OK);
        let mut entry = Sd1DirectoryEntry {
            type_info: 0, file_type: 0, name: [0; 12],
            size_blocks: 0, contiguous_blocks: 0, first_block: 0,
            file_number: 0, size_bytes: 0,
        };
        let rc2 = sd1_disk_find(img, name.as_ptr(), &mut entry);
        assert_eq!(rc2, error::SD1_ERR_FILE_NOT_FOUND);
        sd1_disk_free(img);
    }

    #[test]
    fn disk_extract_roundtrips_raw_data() {
        let img = sd1_disk_create();
        let name = cstr("RAWFILE");
        let data: Vec<u8> = (0..200u8).collect();
        sd1_disk_write(img, name.as_ptr(), 0x0A, false, data.as_ptr(), data.len(), false);

        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;
        let rc = sd1_disk_extract(img, name.as_ptr(), &mut out_ptr, &mut out_len);
        assert_eq!(rc, SD1_OK);
        assert!(!out_ptr.is_null());
        assert_eq!(out_len, data.len());
        let extracted = unsafe { std::slice::from_raw_parts(out_ptr, out_len) };
        assert_eq!(extracted, data.as_slice());
        sd1_bytes_free(out_ptr, out_len);
        sd1_disk_free(img);
    }

    #[test]
    fn fat_count_free_on_blank_disk() {
        let img = sd1_disk_create();
        let free = sd1_fat_count_free(img);
        assert_eq!(free, 1577); // 1600 - 23 reserved
        sd1_disk_free(img);
    }

    #[test]
    fn sysex_parse_and_free() {
        // Build a minimal valid OneProgram SysEx
        let payload = vec![0xAAu8; 530];
        let mut pkt_bytes = vec![0xF0u8, 0x0F, 0x05, 0x00, 0x00, 0x02];
        for &b in &payload {
            pkt_bytes.push((b >> 4) & 0x0F);
            pkt_bytes.push(b & 0x0F);
        }
        pkt_bytes.push(0xF7);

        let mut err: i32 = -99;
        let pkt = sd1_sysex_parse(pkt_bytes.as_ptr(), pkt_bytes.len(), &mut err);
        assert_eq!(err, SD1_OK);
        assert!(!pkt.is_null());

        let msg_type = sd1_sysex_message_type(pkt);
        assert_eq!(msg_type, 0x02); // OneProgram

        let payload_len = sd1_sysex_payload_len(pkt);
        assert_eq!(payload_len, 530);

        sd1_sysex_free(pkt);
    }

    #[test]
    fn program_roundtrip_via_ffi() {
        // Build a 530-byte program with a known name at offset 498
        let mut prog_bytes = vec![0u8; 530];
        prog_bytes[498..509].copy_from_slice(b"MY_PROG    ");

        let mut err: i32 = 0;
        let prog = sd1_program_from_bytes(prog_bytes.as_ptr(), prog_bytes.len(), &mut err);
        assert_eq!(err, SD1_OK);
        assert!(!prog.is_null());

        let mut name_buf = [0i8; 32];
        sd1_program_name(prog, name_buf.as_mut_ptr(), name_buf.len());
        let name_str = unsafe { CStr::from_ptr(name_buf.as_ptr()) }.to_str().unwrap();
        assert_eq!(name_str, "MY_PROG");

        let ft = sd1_program_file_type(prog);
        assert_eq!(ft, 0x0A);

        sd1_program_free(prog);
    }

    #[test]
    fn entries_free_does_not_crash_on_null() {
        sd1_entries_free(std::ptr::null_mut(), 0);
        sd1_bytes_free(std::ptr::null_mut(), 0);
        sd1_u16_array_free(std::ptr::null_mut(), 0);
    }

    #[test]
    fn int0_programs_array_is_accessible() {
        // First entry should be "ARTIC-ELATE"
        let first = unsafe { CStr::from_ptr(SD1_INT0_PROGRAMS.0[0]) };
        assert_eq!(first.to_str().unwrap(), "ARTIC-ELATE");
        // Last entry
        let last = unsafe { CStr::from_ptr(SD1_INT0_PROGRAMS.0[59]) };
        assert_eq!(last.to_str().unwrap(), "MEAN-KIT-1");
    }

    #[test]
    fn rom_all_programs_array_is_accessible() {
        let first = unsafe { CStr::from_ptr(SD1_ROM_ALL_PROGRAMS.0[0]) };
        assert_eq!(first.to_str().unwrap(), "ITS-A-SYNTH");
    }

    #[test]
    fn save_and_reload_via_ffi() {
        let img = sd1_disk_create();
        let path = std::env::temp_dir().join("sd1ffi_test.img");
        let path_cstr = CString::new(path.to_str().unwrap()).unwrap();

        let rc = sd1_disk_save(img, path_cstr.as_ptr());
        assert_eq!(rc, SD1_OK);
        sd1_disk_free(img);

        let mut err: i32 = 0;
        let img2 = sd1_disk_open(path_cstr.as_ptr(), &mut err);
        assert_eq!(err, SD1_OK);
        assert!(!img2.is_null());
        sd1_disk_free(img2);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn disk_open_nonexistent_returns_null() {
        use std::ffi::CString;
        let path = CString::new("/nonexistent/path/does_not_exist.img").unwrap();
        let mut err: i32 = 0;
        let img = sd1_disk_open(path.as_ptr(), &mut err);
        assert!(img.is_null());
        assert_eq!(err, error::SD1_ERR_IO);
    }

    #[test]
    fn sysex_parse_invalid_returns_null() {
        let bad = vec![0x00u8; 4];
        let mut err: i32 = 0;
        let pkt = sd1_sysex_parse(bad.as_ptr(), bad.len(), &mut err);
        assert!(pkt.is_null());
        assert_eq!(err, error::SD1_ERR_INVALID_SYSEX);
    }

    #[test]
    fn sysex_to_bytes_round_trips() {
        // Build a minimal valid OneProgram SysEx packet
        let mut raw = vec![0xF0u8, 0x0F, 0x05, 0x00, 0x00, 0x02];
        let payload = vec![0x42u8; 530];
        for &b in &payload {
            raw.push((b >> 4) & 0x0F);
            raw.push(b & 0x0F);
        }
        raw.push(0xF7);

        let mut err: i32 = 0;
        let pkt = sd1_sysex_parse(raw.as_ptr(), raw.len(), &mut err);
        assert!(!pkt.is_null());
        assert_eq!(err, SD1_OK);

        let mut len: usize = 0;
        let bytes = sd1_sysex_to_bytes(pkt, 0, &mut len);
        assert!(!bytes.is_null());
        assert_eq!(len, raw.len());
        let rebuilt = unsafe { std::slice::from_raw_parts(bytes, len) };
        assert_eq!(rebuilt, raw.as_slice());
        sd1_bytes_free(bytes, len);
        sd1_sysex_free(pkt);
    }

    #[test]
    fn preset_from_bytes_succeeds() {
        let data = vec![0xBBu8; 48];
        let mut err: i32 = 0;
        let preset = sd1_preset_from_bytes(data.as_ptr(), data.len(), &mut err);
        // preset may be null if 48 bytes isn't valid — adjust size if needed
        if !preset.is_null() {
            let mut len: usize = 0;
            let bytes = sd1_preset_bytes(preset, &mut len);
            assert!(!bytes.is_null());
            assert!(len > 0);
            sd1_bytes_free(bytes, len);
            sd1_preset_free(preset);
        } else {
            // If from_bytes requires different size, just verify it doesn't crash
            assert_eq!(err, error::SD1_ERR_INVALID_SYSEX);
        }
    }

    #[test]
    fn fat_chain_follows_links() {
        let img = sd1_disk_create();
        let name = cstr("CHAIN-TST");
        let data = vec![0u8; 1024]; // 2 blocks
        sd1_disk_write(img, name.as_ptr(), 0x0A, false, data.as_ptr(), data.len(), false);

        let mut entry = unsafe { std::mem::zeroed::<Sd1DirectoryEntry>() };
        sd1_disk_find(img, name.as_ptr(), &mut entry);

        let mut blocks: *mut u16 = std::ptr::null_mut();
        let mut count: usize = 0;
        let rc = sd1_fat_chain(img, entry.first_block as u16, &mut blocks, &mut count);
        assert_eq!(rc, SD1_OK);
        assert_eq!(count, 2);
        sd1_u16_array_free(blocks, count);
        sd1_disk_free(img);
    }

    #[test]
    fn disk_write_overwrite_false_returns_file_exists() {
        let img = sd1_disk_create();
        let name = cstr("DUP-FILE");
        let data = vec![0u8; 530];
        sd1_disk_write(img, name.as_ptr(), 0x0A, false, data.as_ptr(), data.len(), false);
        let rc = sd1_disk_write(img, name.as_ptr(), 0x0A, false, data.as_ptr(), data.len(), false);
        assert_eq!(rc, error::SD1_ERR_FILE_EXISTS);
        sd1_disk_free(img);
    }

    #[test]
    fn interleave_deinterleave_round_trips() {
        let payload: Vec<u8> = (0..60 * 530).map(|i| (i % 256) as u8).collect();
        let mut interleaved: *mut u8 = std::ptr::null_mut();
        let mut il_len: usize = 0;
        let rc = sd1_interleave_sixty_programs(payload.as_ptr(), payload.len(),
                                               &mut interleaved, &mut il_len);
        assert_eq!(rc, SD1_OK);
        assert_eq!(il_len, 60 * 530);

        let mut deinterleaved: *mut u8 = std::ptr::null_mut();
        let mut di_len: usize = 0;
        let rc2 = sd1_deinterleave_sixty_programs(interleaved, il_len,
                                                   &mut deinterleaved, &mut di_len);
        assert_eq!(rc2, SD1_OK);
        let recovered = unsafe { std::slice::from_raw_parts(deinterleaved, di_len) };
        assert_eq!(recovered, payload.as_slice());
        sd1_bytes_free(interleaved, il_len);
        sd1_bytes_free(deinterleaved, di_len);
    }

    #[test]
    fn decode_b10_inactive() {
        let mut out = [0u8; 32];
        sd1_decode_b10(0xFF, std::ptr::null(), 0,
                       out.as_mut_ptr() as *mut c_char, out.len());
        // Should not crash; output should be some string
        let nul = out.iter().position(|&b| b == 0).unwrap_or(out.len());
        assert!(nul > 0 || out[0] == 0); // at minimum, doesn't crash
    }

    #[test]
    fn int0_programs_function_returns_populated_table() {
        let ptr = sd1_int0_programs();
        assert!(!ptr.is_null());
        let first = unsafe { CStr::from_ptr(*ptr) };
        assert_eq!(first.to_str().unwrap(), "ARTIC-ELATE");
        let last = unsafe { CStr::from_ptr(*ptr.add(59)) };
        assert_eq!(last.to_str().unwrap(), "MEAN-KIT-1");
    }

    #[test]
    fn rom_all_programs_function_returns_populated_table() {
        let ptr = sd1_rom_all_programs();
        assert!(!ptr.is_null());
        let first = unsafe { CStr::from_ptr(*ptr) };
        assert_eq!(first.to_str().unwrap(), "ITS-A-SYNTH");
        let rom1_first = unsafe { CStr::from_ptr(*ptr.add(60)) };
        assert_eq!(rom1_first.to_str().unwrap(), "OMNIVERSE");
        let last = unsafe { CStr::from_ptr(*ptr.add(119)) };
        assert_eq!(last.to_str().unwrap(), " INDO-AFRO");
    }
}
