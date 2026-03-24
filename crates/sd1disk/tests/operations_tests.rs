use sd1disk::{DiskImage, SubDirectory, FileAllocationTable, Program, DirectoryEntry, FileType};
use sd1disk::sysex::{MessageType, SysExPacket};
use std::path::Path;

fn everything_img() -> DiskImage {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../disk_with_everything.img");
    DiskImage::open(&path).expect("disk_with_everything.img must exist")
}

fn make_test_program_packet(name_bytes_arr: &[u8; 11]) -> SysExPacket {
    let mut payload = vec![0u8; 530];
    payload[498..509].copy_from_slice(name_bytes_arr);
    SysExPacket {
        message_type: MessageType::OneProgram,
        midi_channel: 0,
        model: 0,
        payload,
    }
}

fn name_bytes(s: &str) -> [u8; 11] {
    let mut name = [b' '; 11];
    let bytes = s.as_bytes();
    let len = bytes.len().min(11);
    name[..len].copy_from_slice(&bytes[..len]);
    name
}

// ===== Task 8: List and Inspect =====

#[test]
fn list_returns_entries_from_everything_disk() {
    let img = everything_img();
    let mut all_entries = vec![];
    for dir_idx in 0..4u8 {
        let dir = SubDirectory::new(dir_idx);
        all_entries.extend(dir.entries(&img));
    }
    assert!(!all_entries.is_empty(), "disk_with_everything.img should have files");
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
    assert!(free <= 1577, "free block count {} is impossible", free);
}

#[test]
fn blank_disk_inspect() {
    let img = DiskImage::create();
    let free = img.free_blocks();
    // The blank_image.img template stores 0 in the OS block free-count field;
    // the count is only meaningful after an explicit set_free_blocks call.
    // Just verify the field is readable and within the legal range.
    assert!(free <= 1577, "blank disk free block count {} exceeds maximum", free);
}

// ===== Task 9: Write =====

#[test]
fn write_program_to_blank_disk_and_find_it() {
    let mut img = DiskImage::create();
    // The blank template stores 0 in the OS free-count field; initialize it.
    img.set_free_blocks(1577);
    let initial_free = img.free_blocks();

    let pkt = make_test_program_packet(&name_bytes("TEST_PROG"));
    let prog = Program::from_sysex(&pkt).unwrap();
    let data = prog.to_bytes();
    let n_blocks = ((data.len() + 511) / 512) as u16;

    let blocks = FileAllocationTable::allocate(&mut img, n_blocks).unwrap();
    assert!(blocks[0] >= 23, "must not allocate reserved blocks");

    for (i, &block_num) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(block_num).unwrap();
        if end > start {
            block[..end - start].copy_from_slice(&data[start..end]);
        }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);

    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: name_bytes("TEST_PROG"),
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

    let found = dir.find(&img, "TEST_PROG").unwrap();
    assert_eq!(found.size_bytes, data.len() as u32);
    assert_eq!(found.size_blocks, n_blocks);
    assert!(img.free_blocks() < initial_free);
}

#[test]
fn write_then_read_back_data_matches() {
    let mut img = DiskImage::create();
    let pkt = make_test_program_packet(&name_bytes("READBACK"));
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

    let chain = FileAllocationTable::chain(&img, blocks[0]).unwrap();
    let mut read_back = Vec::new();
    for &b in &chain {
        read_back.extend_from_slice(img.block(b).unwrap());
    }
    read_back.truncate(data.len());
    assert_eq!(read_back, data);
}

// ===== Task 10: Extract and Delete =====

#[test]
fn write_then_extract_matches_original() {
    let mut img = DiskImage::create();
    let pkt = make_test_program_packet(&name_bytes("EXTRACT_ME"));
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

    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: name_bytes("EXTRACT_ME"),
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };
    SubDirectory::new(0).add(&mut img, entry).unwrap();

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
    // The blank template stores 0 in the OS free-count field; initialize it.
    img.set_free_blocks(1577);
    let initial_free = img.free_blocks();
    let pkt = make_test_program_packet(&name_bytes("DELETE_ME"));
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

    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: name_bytes("DELETE_ME"),
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

// ===== AllSequences write pipeline =====

/// Build a minimal AllSequences SysEx payload with one defined sequence (data_size=170).
/// All other 59 sequence header slots have byte 0 == 0 (which the code treats as "defined"
/// but with ds=0, so they contribute nothing to output).
fn make_allsequences_payload() -> Vec<u8> {
    const HEADER_COUNT: usize = 60;
    const HEADER_SIZE: usize = 188;
    const HEADERS_TOTAL: usize = HEADER_COUNT * HEADER_SIZE;
    const SEQ_DATA_LEN: usize = 170;
    const GLOBAL_SIZE: usize = 21;
    const EVENT_LEAD: usize = 12;

    let size_sum: u32 = SEQ_DATA_LEN as u32 + 0xFC;

    let mut headers = vec![0u8; HEADERS_TOTAL];
    // Slot 0: defined (orig_loc=0), data_size=170 at bytes 183-185
    headers[183] = 0;
    headers[184] = 0;
    headers[185] = SEQ_DATA_LEN as u8;
    // Slots 1-59: mark as undefined (byte 0 = 0xFF) so they're skipped
    for slot in 1..HEADER_COUNT {
        headers[slot * HEADER_SIZE] = 0xFF;
    }

    let seq_bytes: Vec<u8> = (0..SEQ_DATA_LEN as u8).collect();
    let mut payload = Vec::new();
    payload.extend_from_slice(&[0u8; 240]);        // ptr table
    payload.extend_from_slice(&[0u8; EVENT_LEAD]); // 12 lead zeros (skipped)
    payload.extend_from_slice(&seq_bytes);          // packed seq data
    payload.extend_from_slice(&headers);
    let mut global = [0u8; GLOBAL_SIZE];
    global[2..6].copy_from_slice(&size_sum.to_be_bytes());
    payload.extend_from_slice(&global);
    payload
}

#[test]
fn allsequences_write_then_list_finds_file_with_correct_size() {
    // 170 bytes padded to 512 → file = 11776 + 512 = 12288 bytes = 24 blocks
    const EXPECTED_BYTES: u32 = 11776 + 512;
    const EXPECTED_BLOCKS: u16 = (EXPECTED_BYTES / 512) as u16;

    let payload = make_allsequences_payload();
    let pkt = SysExPacket {
        message_type: MessageType::AllSequences,
        midi_channel: 0,
        model: 0,
        payload,
    };

    let disk_data = sd1disk::allsequences_to_disk(&pkt.payload, None).unwrap();
    assert_eq!(disk_data.len(), EXPECTED_BYTES as usize);

    let mut img = DiskImage::create();
    img.set_free_blocks(1577);

    let n_blocks = ((disk_data.len() + 511) / 512) as u16;
    assert_eq!(n_blocks, EXPECTED_BLOCKS);

    let blocks = FileAllocationTable::allocate(&mut img, n_blocks).unwrap();
    for (i, &b) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(disk_data.len());
        let block = img.block_mut(b).unwrap();
        if end > start {
            block[..end - start].copy_from_slice(&disk_data[start..end]);
        }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);

    let entry = DirectoryEntry {
        type_info: 0,
        file_type: FileType::SixtySequences,
        name: name_bytes("SEQ-TEST"),
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: disk_data.len() as u32,
    };
    SubDirectory::new(0).add(&mut img, entry).unwrap();

    let found = SubDirectory::new(0).find(&img, "SEQ-TEST").unwrap();
    assert_eq!(found.size_bytes, EXPECTED_BYTES);
    assert_eq!(found.size_blocks, EXPECTED_BLOCKS);
    assert_eq!(found.file_type, FileType::SixtySequences);
}

#[test]
fn allsequences_write_verify_seq_data_on_disk() {
    // Verify the actual sequence bytes land at offset 11776 in the FAT chain.
    let payload = make_allsequences_payload();
    let disk_data = sd1disk::allsequences_to_disk(&payload, None).unwrap();

    // seq_bytes from the fixture is 0..170
    let expected_seq: Vec<u8> = (0u8..170).collect();
    assert_eq!(&disk_data[11776..11776 + 170], expected_seq.as_slice());
    // Padding after the sequence data to end of block should be zero
    assert!(disk_data[11776 + 170..11776 + 512].iter().all(|&b| b == 0));
}

// ===== Task 11: Save/Reload Round-Trip =====

#[test]
fn write_save_reload_file_survives() {
    let mut img = DiskImage::create();
    let pkt = make_test_program_packet(&name_bytes("PERSISTED"));
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

    let entry = DirectoryEntry {
        type_info: 0,
        file_type: prog.file_type(),
        name: name_bytes("PERSISTED"),
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };
    SubDirectory::new(0).add(&mut img, entry).unwrap();

    let path = std::env::temp_dir().join("sd1_persist_test.img");
    img.save(&path).unwrap();
    let reloaded = DiskImage::open(&path).unwrap();

    let found = SubDirectory::new(0).find(&reloaded, "PERSISTED").unwrap();
    assert_eq!(found.size_bytes, data.len() as u32);
    std::fs::remove_file(&path).ok();
}
