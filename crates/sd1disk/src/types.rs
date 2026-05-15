// crates/sd1disk/src/types.rs
use std::borrow::Cow;
use crate::{Error, FileType, Result};
use crate::sysex::{MessageType, SysExPacket};

const PROGRAM_NAME_OFFSET: usize = 498;
const PROGRAM_NAME_LEN: usize = 11;
const PROGRAM_SIZE: usize = 530;
const SIXTY_PROGRAMS_COUNT: usize = 60;
const PRESET_SIZE: usize = 48;

pub struct Program([u8; PROGRAM_SIZE]);

impl Program {
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

    pub fn name(&self) -> Cow<'_, str> {
        let raw = &self.0[PROGRAM_NAME_OFFSET..PROGRAM_NAME_OFFSET + PROGRAM_NAME_LEN];
        let trimmed: Vec<u8> = raw.iter().copied().take_while(|&b| b != 0 && b != b' ').collect();
        String::from_utf8_lossy(&trimmed).into_owned().into()
    }

    pub fn to_sysex(&self, channel: u8) -> SysExPacket {
        SysExPacket {
            message_type: MessageType::OneProgram,
            midi_channel: channel,
            model: 0,
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
            model: 0,
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
            model: 0,
            payload: self.0.clone(),
        }
    }

    pub fn file_type(&self) -> FileType {
        FileType::OneSequence
    }
}

/// Convert AllPrograms SysEx payload (60 × 530 bytes, programs in order) to the
/// SD-1 on-disk SixtyPrograms format.
///
/// File byte layout: the 31800 bytes are a byte-level interleave of two 15900-byte streams:
///   even bytes (positions 0,2,4,...) = programs 0–29 concatenated  (b10 = 0–29)
///   odd bytes  (positions 1,3,5,...) = programs 30–59 concatenated (b10 = 30–59)
///
/// Within each 1060-byte pair k, even byte positions hold program k and odd positions hold
/// program k+30, so the hardware can find program b10 by extracting the even or odd stream.
pub fn interleave_sixty_programs(payload: &[u8]) -> Result<Vec<u8>> {
    let expected = SIXTY_PROGRAMS_COUNT * PROGRAM_SIZE;
    if payload.len() != expected {
        return Err(Error::InvalidSysEx("AllPrograms payload must be exactly 60 × 530 bytes"));
    }
    let half = 30 * PROGRAM_SIZE; // 15900
    let even_data = &payload[..half];       // programs 0–29
    let odd_data  = &payload[half..];       // programs 30–59
    let mut result = vec![0u8; expected];
    for i in 0..half {
        result[2 * i]     = even_data[i];
        result[2 * i + 1] = odd_data[i];
    }
    Ok(result)
}

/// Convert an AllSequences SysEx payload to the on-disk SixtySequences format.
///
/// SysEx AllSequences payload layout:
///   [0..240]            – 60 × 4-byte internal memory pointer table (SD-1 private; not written to disk)
///   [240..-(29+11160)]  – sequence event data; first 12 bytes are SD-1-internal zeros,
///                          actual packed event data starts at byte 252.
///   [-(29+11160)..-29]  – 60 × 186-byte sequence headers
///   [-29..]             – 29-byte global section
///                          [0..8]   SD-1 internal state (not stored on disk)
///                          [8..10]  current selected sequence number (BE u16)
///                          [10..14] declared event-area size = 12 + packed_events (BE u32)
///                          [14..29] global sequencer information
///
/// On-disk SixtySequences (No Programs) layout:
///   [0..11280]          – 60 × 188-byte sequence headers (186-byte SysEx header + 2 trailing zeros)
///   [11280..11301]      – 21-byte global section (SysEx global[8..29], stripping 8 internal bytes)
///   [11301..11776]      – zeros (475 bytes)
///   [11776..]           – sequence event data (block-padded per sequence)
///
/// If `interleaved_programs` is `Some`, it must be exactly 60 × 530 = 31800 bytes of
/// already-interleaved program data (output of `interleave_sixty_programs`). The programs
/// are embedded between the global section and the sequence data, producing the
/// "SixtySequences + 60 Programs" on-disk layout:
///
/// ```text
/// 00000–11279  Sequence headers (60 × 188)
/// 11280–11300  Global section (21 bytes)
/// 11301–11775  Zeros (475 bytes)
/// 11776–43575  60 Programs interleaved (31800 bytes)   ← only when programs provided
/// 43576–44031  Zeros (456 bytes)                       ← only when programs provided
/// 44032–…      Sequence data (block-padded)             ← offset shifts with programs
/// ```
///
/// Without programs the sequence data starts at 11776 (no-programs layout).
pub fn allsequences_to_disk(payload: &[u8], interleaved_programs: Option<&[u8]>) -> Result<Vec<u8>> {
    const PTR_TABLE_SIZE: usize = 240;
    const SYSEX_HEADER_SIZE: usize = 186;   // header size in SysEx input
    const DISK_HEADER_SIZE: usize = 188;    // header size on disk output
    const HEADER_COUNT: usize = 60;
    const SYSEX_GLOBAL_SIZE: usize = 29;    // global section size in SysEx input
    const DISK_GLOBAL_SIZE: usize = 21;     // global section size on disk output
    const GLOBAL_INTERNAL_BYTES: usize = 8; // leading SysEx global bytes not stored on disk
    const SYSEX_HEADERS_TOTAL: usize = SYSEX_HEADER_SIZE * HEADER_COUNT; // 11160
    const DISK_HEADERS_TOTAL: usize = DISK_HEADER_SIZE * HEADER_COUNT;   // 11280
    const DISK_GLOBAL_START: usize = DISK_HEADERS_TOTAL;                  // 11280
    const DISK_GLOBAL_END: usize = DISK_GLOBAL_START + DISK_GLOBAL_SIZE;  // 11301
    const MIN_PAYLOAD: usize = PTR_TABLE_SIZE + SYSEX_HEADERS_TOTAL + SYSEX_GLOBAL_SIZE;
    const EVENT_LEAD_ZEROS: usize = 12;
    const PROGRAMS_DISK_OFFSET: usize = 11776;
    const PROGRAMS_SIZE: usize = 60 * 530; // 31800
    const SEQ_DATA_WITH_PROGRAMS: usize = 44032;
    const SEQ_DATA_NO_PROGRAMS: usize = 11776;

    if let Some(progs) = interleaved_programs {
        if progs.len() != PROGRAMS_SIZE {
            return Err(Error::InvalidSysEx(
                "interleaved programs must be exactly 60 × 530 bytes",
            ));
        }
    }

    if payload.len() < MIN_PAYLOAD {
        return Err(Error::InvalidSysEx("AllSequences payload too short"));
    }

    let sysex_global = &payload[payload.len() - SYSEX_GLOBAL_SIZE..];
    let headers_start = payload.len() - SYSEX_GLOBAL_SIZE - SYSEX_HEADERS_TOTAL;
    let headers_sec = &payload[headers_start..payload.len() - SYSEX_GLOBAL_SIZE];
    let event_data = &payload[PTR_TABLE_SIZE..headers_start];

    if event_data.len() < EVENT_LEAD_ZEROS {
        return Err(Error::InvalidSysEx("AllSequences payload: event data section too short"));
    }

    // SysEx global[10..14] (BE u32) = declared event-area size = EVENT_LEAD_ZEROS + packed_events.
    let declared_size = u32::from_be_bytes([
        sysex_global[10], sysex_global[11], sysex_global[12], sysex_global[13],
    ]);
    let seq_data_len = (declared_size as usize).saturating_sub(EVENT_LEAD_ZEROS);

    let event_start = EVENT_LEAD_ZEROS;
    if event_data.len() < event_start + seq_data_len {
        return Err(Error::InvalidSysEx("AllSequences payload: event data too short for declared seq_data_len"));
    }
    let actual_event_data = &event_data[event_start..event_start + seq_data_len];

    // Compute on-disk padded size: each defined sequence rounded up to 512-byte block.
    const BLOCK_SIZE: usize = 512;
    let padded_total: usize = (0..HEADER_COUNT)
        .filter_map(|slot| {
            let hdr = &headers_sec[slot * SYSEX_HEADER_SIZE..(slot + 1) * SYSEX_HEADER_SIZE];
            if hdr[0] == 0xFF { return None; }
            let ds = u32::from_be_bytes([0, hdr[183], hdr[184], hdr[185]]) as usize;
            Some((ds + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE)
        })
        .sum();

    let seq_data_offset = if interleaved_programs.is_some() {
        SEQ_DATA_WITH_PROGRAMS
    } else {
        SEQ_DATA_NO_PROGRAMS
    };

    let file_size = seq_data_offset + padded_total;
    let mut out = vec![0u8; file_size];

    // Write headers: expand each 186-byte SysEx header to 188 bytes on disk (2 trailing zeros).
    for slot in 0..HEADER_COUNT {
        let sysex_hdr = &headers_sec[slot * SYSEX_HEADER_SIZE..(slot + 1) * SYSEX_HEADER_SIZE];
        let dst = slot * DISK_HEADER_SIZE;
        out[dst..dst + SYSEX_HEADER_SIZE].copy_from_slice(sysex_hdr);
        // bytes [dst+186..dst+188] remain zero (already zeroed)
    }

    // Write global: on-disk global = SysEx global[8..29] (strip 8 SD-1-internal bytes).
    let disk_global = &sysex_global[GLOBAL_INTERNAL_BYTES..];
    out[DISK_GLOBAL_START..DISK_GLOBAL_END].copy_from_slice(disk_global);

    if let Some(progs) = interleaved_programs {
        out[PROGRAMS_DISK_OFFSET..PROGRAMS_DISK_OFFSET + PROGRAMS_SIZE].copy_from_slice(progs);
    }

    // Write each defined sequence's data at its block-padded position.
    let mut in_pos = 0usize;
    let mut out_pos = seq_data_offset;
    for slot in 0..HEADER_COUNT {
        let hdr = &headers_sec[slot * SYSEX_HEADER_SIZE..(slot + 1) * SYSEX_HEADER_SIZE];
        if hdr[0] == 0xFF { continue; }
        let ds = u32::from_be_bytes([0, hdr[183], hdr[184], hdr[185]]) as usize;
        if ds == 0 { continue; }
        out[out_pos..out_pos + ds].copy_from_slice(&actual_event_data[in_pos..in_pos + ds]);
        in_pos += ds;
        out_pos += (ds + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE;
    }

    Ok(out)
}

/// Reconstruct a hardware-compatible SD-1 AllSequences SysEx (F0…F7, nibble-encoded)
/// from the SD-1 on-disk SixtySequences format, ready to send to a real SD-1.
///
/// On-disk layout (no programs):
///   [0..11280]     – 60 × 188-byte sequence headers
///   [11280..11301] – 21-byte global section
///   [11301..11776] – zeros
///   [11776..]      – sequence event data (each sequence block-padded to 512 bytes)
///
/// On-disk layout (with embedded programs, `has_programs = true`):
///   same header/global prefix, seq data starts at 44032 instead of 11776.
///
/// Seq 59 is always undefined in this format (hardware limitation: no ptr-table entry).
pub fn disk_to_allsequences(disk: &[u8], has_programs: bool) -> Result<Vec<u8>> {
    const DISK_HEADER_SIZE: usize = 188;    // on-disk header size
    const SYSEX_HEADER_SIZE: usize = 186;   // SysEx output header size
    const HEADER_COUNT: usize = 60;
    const HW_SEQ_COUNT: usize = 59;  // seqs 0..58; seq 59 has no hw ptr entry
    const HW_POOL_STATE_BYTES: usize = 9;
    const EVENT_LEAD_ZEROS: usize = 12;
    const POOL_PREAMBLE: usize = EVENT_LEAD_ZEROS + HW_POOL_STATE_BYTES; // 21
    const HW_BASE_ADDRESS: u32 = 0x0004_9000;
    const DISK_HEADERS_TOTAL: usize = DISK_HEADER_SIZE * HEADER_COUNT; // 11280
    const DISK_GLOBAL_SIZE: usize = 21;     // on-disk global size
    const SYSEX_GLOBAL_SIZE: usize = 29;    // SysEx output global size
    const GLOBAL_INTERNAL_BYTES: usize = 8; // zero bytes prepended in SysEx global
    const DISK_GLOBAL_START: usize = DISK_HEADERS_TOTAL;                  // 11280
    const DISK_GLOBAL_END: usize = DISK_GLOBAL_START + DISK_GLOBAL_SIZE;  // 11301
    const SEQ_DATA_NO_PROGRAMS: usize = 11776;
    const SEQ_DATA_WITH_PROGRAMS: usize = 44032;
    const BLOCK_SIZE: usize = 512;
    const PTR_TABLE_SIZE: usize = 240;

    let min_size = if has_programs { SEQ_DATA_WITH_PROGRAMS } else { SEQ_DATA_NO_PROGRAMS };
    if disk.len() < min_size {
        return Err(Error::InvalidSysEx("on-disk SixtySequences data too short"));
    }

    let disk_headers = &disk[..DISK_HEADERS_TOTAL];
    let disk_global = &disk[DISK_GLOBAL_START..DISK_GLOBAL_END];
    let seq_data_offset = if has_programs { SEQ_DATA_WITH_PROGRAMS } else { SEQ_DATA_NO_PROGRAMS };

    // Pass 1: unpack real event data for defined seqs 0..58 (seq 59 has no HW ptr entry).
    // Also read stale ds from disk bytes 183..185 for undefined (0xFF) seqs 0..58
    // so their stale ptr-table entries can be reconstructed for a lossless round-trip.
    let mut packed_events: Vec<u8> = Vec::new();
    let mut stale_ds = [0u32; HW_SEQ_COUNT]; // stale ds for each undefined slot
    let mut real_offset: u32 = 0;             // cumulative real event bytes
    let mut stale_offset: u32 = 0;            // cumulative stale bytes (for undefined slots)
    // cumulative_offset[slot]: byte offset (past pool preamble) where this slot starts.
    // For defined slots: into the real event data region.
    // For undefined slots: past real event data, in a stale extension region.
    let mut cumulative_offset = [0u32; HW_SEQ_COUNT];
    let mut in_pos = seq_data_offset;
    let mut sum_ds: u32 = 0;
    for slot in 0..HEADER_COUNT {
        let disk_hdr = &disk_headers[slot * DISK_HEADER_SIZE..(slot + 1) * DISK_HEADER_SIZE];
        if disk_hdr[0] == 0xFF {
            // Undefined slot: no event data on disk.
            // Read stale ds from bytes 183..185 (stamped there by allsequences_hardware_sysex_to_disk).
            let sds = u32::from_be_bytes([0, disk_hdr[183], disk_hdr[184], disk_hdr[185]]);
            if slot < HW_SEQ_COUNT {
                // Place this slot's ptr past the real event data, in a stale extension region.
                cumulative_offset[slot] = sum_ds + stale_offset;
                stale_ds[slot] = sds;
                stale_offset += sds;
            }
            continue;
        }
        let ds = u32::from_be_bytes([0, disk_hdr[183], disk_hdr[184], disk_hdr[185]]);
        if slot < HW_SEQ_COUNT {
            // Defined slot: record real cumulative offset.
            cumulative_offset[slot] = real_offset;
        }
        if ds == 0 { continue; }
        if in_pos + ds as usize > disk.len() {
            return Err(Error::InvalidSysEx("on-disk SixtySequences: sequence data out of bounds"));
        }
        if slot < HW_SEQ_COUNT {
            packed_events.extend_from_slice(&disk[in_pos..in_pos + ds as usize]);
            real_offset += ds;
            sum_ds += ds;
        }
        in_pos += (ds as usize + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE;
    }

    // Build HW ptr table:
    //   entry 0 = HW_BASE_ADDRESS
    //   entries 1..59 for seqs 0..58:
    //     - Defined slot with ds>0: POOL_PREAMBLE + cumulative_offset[slot]
    //     - Defined slot with ds=0 and i>0: 0 (triggers hardware decoder's "no event data" rule)
    //     - Undefined (0xFF) slot: POOL_PREAMBLE + cumulative_offset[slot] (stale or past-end)
    // Entry 0 of ptr_table is special: it stores HW_BASE_ADDRESS (base RAM address).
    let mut ptr_table = vec![0u8; PTR_TABLE_SIZE];
    ptr_table[0..4].copy_from_slice(&HW_BASE_ADDRESS.to_be_bytes());
    for slot in 0..HW_SEQ_COUNT {
        let disk_hdr = &disk_headers[slot * DISK_HEADER_SIZE..(slot + 1) * DISK_HEADER_SIZE];
        let hw_val: u32;
        if disk_hdr[0] != 0xFF {
            // Defined slot
            let ds_from_disk = u32::from_be_bytes([0, disk_hdr[183], disk_hdr[184], disk_hdr[185]]);
            if ds_from_disk == 0 && slot > 0 {
                // Defined with ds=0: emit ptr=0 so hardware decoder gives ds[slot]=0 (no stamp corruption).
                hw_val = 0;
            } else {
                hw_val = POOL_PREAMBLE as u32 + cumulative_offset[slot];
            }
        } else {
            // Undefined slot: stale offset past real event data
            hw_val = POOL_PREAMBLE as u32 + cumulative_offset[slot];
        }
        ptr_table[(slot + 1) * 4..(slot + 2) * 4].copy_from_slice(&hw_val.to_be_bytes());
    }

    // Build HW pool: EVENT_LEAD_ZEROS + HW_POOL_STATE_BYTES zeros + packed_events
    //   + stale_offset dummy bytes (to accommodate stale ptr-table chain)
    //   + EVENT_LEAD_ZEROS sentinel.
    // total_packed = POOL_PREAMBLE + sum_ds + stale_offset (hardware decoder must see this
    // as the fallback "end" so stale chain resolves correctly).
    let mut pool: Vec<u8> = Vec::new();
    pool.extend_from_slice(&[0u8; EVENT_LEAD_ZEROS]);
    pool.extend_from_slice(&[0u8; HW_POOL_STATE_BYTES]);
    pool.extend_from_slice(&packed_events);
    pool.extend(std::iter::repeat(0u8).take(stale_offset as usize)); // stale region (zeros; content unused)
    pool.extend_from_slice(&[0u8; EVENT_LEAD_ZEROS]);

    // Build SysEx global: prepend 8 zero bytes to the 21-byte on-disk global,
    // then overwrite the declared field with EVENT_LEAD_ZEROS + sum_ds.
    let mut sysex_global = [0u8; SYSEX_GLOBAL_SIZE];
    sysex_global[GLOBAL_INTERNAL_BYTES..].copy_from_slice(disk_global);
    let declared = EVENT_LEAD_ZEROS as u32 + sum_ds;
    sysex_global[10..14].copy_from_slice(&declared.to_be_bytes());

    // Build payload:
    //   ptr_table (240)
    //   + pool (POOL_PREAMBLE + packed_events + stale region + EVENT_LEAD_ZEROS sentinel)
    //   + 59 sequence headers (seqs 0..58, first 186 bytes each)
    //   + 1 all-0xFF header for seq 59 (hardware limitation; bytes 183..185 preserved from disk)
    //   + sysex_global (29)
    let mut payload = Vec::new();
    payload.extend_from_slice(&ptr_table);
    payload.extend_from_slice(&pool);
    for slot in 0..HW_SEQ_COUNT {
        let disk_hdr = &disk_headers[slot * DISK_HEADER_SIZE..(slot + 1) * DISK_HEADER_SIZE];
        payload.extend_from_slice(&disk_hdr[..SYSEX_HEADER_SIZE]);
    }
    // Seq 59: always undefined in hardware format (no ptr-table entry).
    // Preserve bytes 183..185 from disk so they round-trip cleanly through the hardware
    // decoder (which does NOT stamp slot 59, preserving whatever bytes the SysEx contained).
    {
        let disk_hdr59 = &disk_headers[59 * DISK_HEADER_SIZE..(59 + 1) * DISK_HEADER_SIZE];
        let mut hdr59 = [0xFFu8; SYSEX_HEADER_SIZE];
        hdr59[183] = disk_hdr59[183];
        hdr59[184] = disk_hdr59[184];
        hdr59[185] = disk_hdr59[185];
        payload.extend_from_slice(&hdr59);
    }
    payload.extend_from_slice(&sysex_global);

    Ok(sysex_nibble_encode_wrap(&payload))
}

/// Reconstruct a hardware-compatible SD-1 AllSequences SysEx (F0…F7, nibble-encoded)
/// from the SD-1 on-disk ThirtySequences format, ready to send to a real SD-1.
///
/// On-disk layout:
///   [0..5640]    – 30 × 188-byte sequence headers
///   [5640..5661] – 21-byte global section
///   [5661..6144] – zeros (483 bytes)
///   [6144..]     – sequence event data (each sequence block-padded to 512 bytes)
///                  Programs (if any) appear after the sequence data and are ignored here.
///
/// Seq 59 is always undefined in the output (hardware limitation: no ptr-table entry).
pub fn disk_to_thirty_sequences(disk: &[u8]) -> Result<Vec<u8>> {
    const DISK_HEADER_SIZE: usize = 188;
    const SYSEX_HEADER_SIZE: usize = 186;
    const HEADER_COUNT: usize = 30;        // slots on disk
    const HW_SEQ_COUNT: usize = 59;        // seqs 0..58 have hw ptr entries; seq 59 does not
    const HEADER_COUNT_SIXTY: usize = 60;
    const HW_POOL_STATE_BYTES: usize = 9;
    const EVENT_LEAD_ZEROS: usize = 12;
    const POOL_PREAMBLE: usize = EVENT_LEAD_ZEROS + HW_POOL_STATE_BYTES; // 21
    const HW_BASE_ADDRESS: u32 = 0x0004_9000;
    const DISK_HEADERS_TOTAL: usize = DISK_HEADER_SIZE * HEADER_COUNT; // 5640
    const DISK_GLOBAL_SIZE: usize = 21;
    const SYSEX_GLOBAL_SIZE: usize = 29;
    const GLOBAL_INTERNAL_BYTES: usize = 8;
    const DISK_GLOBAL_START: usize = DISK_HEADERS_TOTAL;                    // 5640
    const DISK_GLOBAL_END: usize = DISK_GLOBAL_START + DISK_GLOBAL_SIZE;    // 5661
    const SEQ_DATA_OFFSET: usize = 6144;
    const BLOCK_SIZE: usize = 512;
    const PTR_TABLE_SIZE: usize = 240;

    if disk.len() < SEQ_DATA_OFFSET {
        return Err(Error::InvalidSysEx("on-disk ThirtySequences data too short"));
    }

    let disk_headers = &disk[..DISK_HEADERS_TOTAL];
    let disk_global  = &disk[DISK_GLOBAL_START..DISK_GLOBAL_END];

    // Unpack per-sequence event data for the 30 on-disk slots, removing block padding.
    let mut packed_events: Vec<u8> = Vec::new();
    // cumulative_offset[slot] for seqs 0..29 (from disk); seqs 30..58 will point past event data
    let mut cumulative_offset = [0u32; HW_SEQ_COUNT];
    let mut offset: u32 = 0;
    let mut in_pos = SEQ_DATA_OFFSET;
    let mut sum_ds: u32 = 0;
    for slot in 0..HEADER_COUNT {
        cumulative_offset[slot] = offset;
        let disk_hdr = &disk_headers[slot * DISK_HEADER_SIZE..(slot + 1) * DISK_HEADER_SIZE];
        if disk_hdr[0] == 0xFF { continue; }
        let ds = u32::from_be_bytes([0, disk_hdr[183], disk_hdr[184], disk_hdr[185]]);
        if ds == 0 { continue; }
        if in_pos + ds as usize > disk.len() {
            return Err(Error::InvalidSysEx("on-disk ThirtySequences: sequence data out of bounds"));
        }
        packed_events.extend_from_slice(&disk[in_pos..in_pos + ds as usize]);
        in_pos += (ds as usize + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE;
        offset += ds;
        sum_ds += ds;
    }
    // Seqs 30..58: undefined (0xFF headers), ptr entries point past event data.
    for slot in HEADER_COUNT..HW_SEQ_COUNT {
        cumulative_offset[slot] = offset; // same as sum_ds (past end of event data)
    }

    // Build HW ptr table:
    //   entry 0 = HW_BASE_ADDRESS
    //   entries 1..59 = POOL_PREAMBLE + cumulative_offset[slot] for seqs 0..58
    let mut ptr_table = vec![0u8; PTR_TABLE_SIZE];
    ptr_table[0..4].copy_from_slice(&HW_BASE_ADDRESS.to_be_bytes());
    for slot in 0..HW_SEQ_COUNT {
        let hw_val = POOL_PREAMBLE as u32 + cumulative_offset[slot];
        ptr_table[(slot + 1) * 4..(slot + 2) * 4].copy_from_slice(&hw_val.to_be_bytes());
    }

    // Build HW pool: EVENT_LEAD_ZEROS + HW_POOL_STATE_BYTES zeros + packed_events + EVENT_LEAD_ZEROS sentinel
    let mut pool: Vec<u8> = Vec::new();
    pool.extend_from_slice(&[0u8; EVENT_LEAD_ZEROS]);
    pool.extend_from_slice(&[0u8; HW_POOL_STATE_BYTES]);
    pool.extend_from_slice(&packed_events);
    pool.extend_from_slice(&[0u8; EVENT_LEAD_ZEROS]);

    // Build SysEx global: prepend 8 zero bytes to the 21-byte on-disk global,
    // then always overwrite the declared field with EVENT_LEAD_ZEROS + sum_ds.
    let mut sysex_global = [0u8; SYSEX_GLOBAL_SIZE];
    sysex_global[GLOBAL_INTERNAL_BYTES..].copy_from_slice(disk_global);
    let declared = EVENT_LEAD_ZEROS as u32 + sum_ds;
    sysex_global[10..14].copy_from_slice(&declared.to_be_bytes());

    // Build payload:
    //   ptr_table (240)
    //   + pool (POOL_PREAMBLE + packed_events + EVENT_LEAD_ZEROS sentinel)
    //   + 30 sequence headers (seqs 0..29 from disk, first 186 bytes each)
    //   + 29 all-0xFF headers (seqs 30..58, undefined)
    //   + 1 all-0xFF header for seq 59 (hardware limitation: no ptr entry)
    //   + sysex_global (29)
    let undefined_hdr = [0xFFu8; SYSEX_HEADER_SIZE];
    let mut payload = Vec::new();
    payload.extend_from_slice(&ptr_table);
    payload.extend_from_slice(&pool);
    // Seqs 0..29 from disk
    for slot in 0..HEADER_COUNT {
        let disk_hdr = &disk_headers[slot * DISK_HEADER_SIZE..(slot + 1) * DISK_HEADER_SIZE];
        payload.extend_from_slice(&disk_hdr[..SYSEX_HEADER_SIZE]);
    }
    // Seqs 30..59: all undefined
    for _ in HEADER_COUNT..HEADER_COUNT_SIXTY {
        payload.extend_from_slice(&undefined_hdr);
    }
    payload.extend_from_slice(&sysex_global);

    Ok(sysex_nibble_encode_wrap(&payload))
}

/// Convert an AllSequences SysEx payload to the SD-1 on-disk ThirtySequences format.
///
/// Only sequence slots 0–29 are written; slots 30–59 in the payload are ignored.
/// Programs (if provided) are embedded AFTER the sequence data, unlike SixtySequences
/// which places programs before sequence data.
///
/// On-disk ThirtySequences (no programs) layout:
/// ```text
/// 00000–05639  Sequence headers (30 × 188)
/// 05640–05660  Global section (21 bytes)
/// 05661–06143  Zeros (483 bytes)
/// 06144–…      Sequence data (block-padded to 512 bytes per sequence)
/// ```
///
/// On-disk ThirtySequences (with programs) layout:
/// ```text
/// 00000–05639  Sequence headers (30 × 188)
/// 05640–05660  Global section (21 bytes)
/// 05661–06143  Zeros (483 bytes)
/// 06144–…      Sequence data (block-padded)
/// …            60 Programs interleaved (31800 bytes)
/// …            Zeros (456 bytes)
/// ```
pub fn thirty_sequences_to_disk(payload: &[u8], interleaved_programs: Option<&[u8]>) -> Result<Vec<u8>> {
    const PTR_TABLE_SIZE: usize = 240;
    const SYSEX_HEADER_SIZE: usize = 186;
    const DISK_HEADER_SIZE: usize = 188;
    const HEADER_COUNT: usize = 30;
    const HEADER_COUNT_SIXTY: usize = 60;
    const SYSEX_GLOBAL_SIZE: usize = 29;
    const DISK_GLOBAL_SIZE: usize = 21;
    const GLOBAL_INTERNAL_BYTES: usize = 8;
    const SYSEX_HEADERS_TOTAL_THIRTY: usize = SYSEX_HEADER_SIZE * HEADER_COUNT;        // 5580
    const SYSEX_HEADERS_TOTAL_SIXTY: usize  = SYSEX_HEADER_SIZE * HEADER_COUNT_SIXTY;  // 11160
    const DISK_HEADERS_TOTAL: usize = DISK_HEADER_SIZE * HEADER_COUNT;                 // 5640
    const DISK_GLOBAL_START: usize = DISK_HEADERS_TOTAL;                               // 5640
    const DISK_GLOBAL_END: usize = DISK_GLOBAL_START + DISK_GLOBAL_SIZE;               // 5661
    const EVENT_LEAD_ZEROS: usize = 12;
    const SEQ_DATA_OFFSET: usize = 6144;
    const BLOCK_SIZE: usize = 512;
    const PROGRAMS_SIZE: usize = 60 * 530;  // 31800 (always 60 programs even in ThirtySeq)
    const PROGRAMS_PADDING: usize = 456;

    if let Some(progs) = interleaved_programs {
        if progs.len() != PROGRAMS_SIZE {
            return Err(Error::InvalidSysEx(
                "interleaved programs must be exactly 60 × 530 bytes",
            ));
        }
    }

    let min_thirty = PTR_TABLE_SIZE + SYSEX_HEADERS_TOTAL_THIRTY + SYSEX_GLOBAL_SIZE;
    let min_sixty  = PTR_TABLE_SIZE + SYSEX_HEADERS_TOTAL_SIXTY  + SYSEX_GLOBAL_SIZE;
    if payload.len() < min_thirty {
        return Err(Error::InvalidSysEx("AllSequences payload too short"));
    }

    // Locate SysEx headers (186-byte stride) and global (29 bytes).
    let total_header_bytes = if payload.len() >= min_sixty { SYSEX_HEADERS_TOTAL_SIXTY } else { SYSEX_HEADERS_TOTAL_THIRTY };
    let sysex_global  = &payload[payload.len() - SYSEX_GLOBAL_SIZE..];
    let headers_start = payload.len() - SYSEX_GLOBAL_SIZE - total_header_bytes;
    let headers_sec   = &payload[headers_start..payload.len() - SYSEX_GLOBAL_SIZE];
    let event_data    = &payload[PTR_TABLE_SIZE..headers_start];

    if event_data.len() < EVENT_LEAD_ZEROS {
        return Err(Error::InvalidSysEx("AllSequences payload: event data section too short"));
    }
    let event_content = &event_data[EVENT_LEAD_ZEROS..];

    // Compute total event bytes from slot ds values (robust even if global is zeroed).
    let total_ds: usize = (0..HEADER_COUNT)
        .filter_map(|slot| {
            let hdr = &headers_sec[slot * SYSEX_HEADER_SIZE..(slot + 1) * SYSEX_HEADER_SIZE];
            if hdr[0] == 0xFF { return None; }
            let ds = u32::from_be_bytes([0, hdr[183], hdr[184], hdr[185]]) as usize;
            if ds == 0 { None } else { Some(ds) }
        })
        .sum();

    if event_content.len() < total_ds {
        return Err(Error::InvalidSysEx("AllSequences payload: event data too short for declared sequence sizes"));
    }

    // Compute on-disk padded size for slots 0–29 only.
    let padded_total: usize = (0..HEADER_COUNT)
        .filter_map(|slot| {
            let hdr = &headers_sec[slot * SYSEX_HEADER_SIZE..(slot + 1) * SYSEX_HEADER_SIZE];
            if hdr[0] == 0xFF { return None; }
            let ds = u32::from_be_bytes([0, hdr[183], hdr[184], hdr[185]]) as usize;
            if ds == 0 { return None; }
            Some((ds + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE)
        })
        .sum();

    let prog_section = if interleaved_programs.is_some() { PROGRAMS_SIZE + PROGRAMS_PADDING } else { 0 };
    let file_size = SEQ_DATA_OFFSET + padded_total + prog_section;
    let mut out = vec![0u8; file_size];

    // Write headers: expand each 186-byte SysEx header to 188 bytes on disk (2 trailing zeros).
    for slot in 0..HEADER_COUNT {
        let sysex_hdr = &headers_sec[slot * SYSEX_HEADER_SIZE..(slot + 1) * SYSEX_HEADER_SIZE];
        let dst = slot * DISK_HEADER_SIZE;
        out[dst..dst + SYSEX_HEADER_SIZE].copy_from_slice(sysex_hdr);
        // bytes [dst+186..dst+188] remain zero
    }

    // Write global: on-disk global = SysEx global[8..29] (strip 8 SD-1-internal bytes).
    let disk_global = &sysex_global[GLOBAL_INTERNAL_BYTES..];
    out[DISK_GLOBAL_START..DISK_GLOBAL_END].copy_from_slice(disk_global);

    // Write sequence data at SEQ_DATA_OFFSET.
    let mut in_pos  = 0usize;
    let mut out_pos = SEQ_DATA_OFFSET;
    for slot in 0..HEADER_COUNT {
        let hdr = &headers_sec[slot * SYSEX_HEADER_SIZE..(slot + 1) * SYSEX_HEADER_SIZE];
        if hdr[0] == 0xFF { continue; }
        let ds = u32::from_be_bytes([0, hdr[183], hdr[184], hdr[185]]) as usize;
        if ds == 0 { continue; }
        out[out_pos..out_pos + ds].copy_from_slice(&event_content[in_pos..in_pos + ds]);
        in_pos  += ds;
        out_pos += (ds + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE;
    }

    // Write programs AFTER sequence data (ThirtySeq layout).
    if let Some(progs) = interleaved_programs {
        out[out_pos..out_pos + PROGRAMS_SIZE].copy_from_slice(progs);
    }

    Ok(out)
}

/// Convert an AllSequences SysEx file to SD-1 on-disk SixtySequences format,
/// automatically detecting whether the file is a hardware RAM dump or a
/// library-generated SysEx.
///
/// Detection: after nibble-decoding the AllSequences payload, `decoded[0..4]`
/// is the first ptr-table entry.
/// - **Non-zero** → hardware dump (base RAM address, e.g. `0x00049000`).
///   Routed to `allsequences_hardware_sysex_to_disk`.
/// - **Zero** → library-generated (cumulative seq-0 offset = 0).
///   Routed to `allsequences_to_disk`.
///
/// Multi-message files (e.g. from SysEx Librarian) are supported.
/// `interleaved_programs`, if provided, must be exactly 60 × 530 = 31800 bytes.
pub fn allsequences_sysex_to_disk(raw: &[u8], interleaved_programs: Option<&[u8]>) -> Result<Vec<u8>> {
    // Find AllSequences message and nibble-decode its payload.
    let msg_start = raw.windows(6)
        .position(|w| w[0] == 0xF0 && w[1] == 0x0F && w[2] == 0x05 && w[5] == 0x0A)
        .ok_or(Error::InvalidSysEx("SysEx: AllSequences (0x0A) message not found"))?;
    let nibble_start = msg_start + 6;
    let f7_offset = raw[nibble_start..].iter().position(|&b| b == 0xF7)
        .ok_or(Error::InvalidSysEx("SysEx: missing F7 terminator"))?;
    let decoded = decode_sysex_nibbles(&raw[nibble_start..nibble_start + f7_offset]);

    if decoded.len() < 4 {
        return Err(Error::InvalidSysEx("SysEx: AllSequences payload too short after nibble decode"));
    }

    let base_addr = u32::from_be_bytes([decoded[0], decoded[1], decoded[2], decoded[3]]);
    if base_addr != 0 {
        // Hardware RAM dump: base address in ptr_table[0], pool preamble present.
        allsequences_hardware_sysex_to_disk(raw, interleaved_programs)
    } else {
        // Library-generated SysEx: ptr_table[0] = 0, standard payload layout.
        allsequences_to_disk(&decoded, interleaved_programs)
    }
}

/// Nibble-decode a hardware SD-1 SysEx data section.
/// Each pair of bytes `(hi, lo)` decodes to one output byte: `(hi << 4) | lo`.
pub fn decode_sysex_nibbles(nibbles: &[u8]) -> Vec<u8> {
    nibbles.chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0] << 4) | c[1])
        .collect()
}

/// Nibble-encode `payload` and wrap it in a hardware SD-1 AllSequences SysEx frame.
/// Frame: `F0 0F 05 00 00 0A [nibble-encoded bytes] F7`
fn sysex_nibble_encode_wrap(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + payload.len() * 2 + 1);
    out.extend_from_slice(&[0xF0, 0x0F, 0x05, 0x00, 0x00, 0x0A]);
    for &b in payload {
        out.push((b >> 4) & 0x0F);
        out.push(b & 0x0F);
    }
    out.push(0xF7);
    out
}

/// Convert a hardware AllSequences SysEx dump to SD-1 on-disk SixtySequences format.
///
/// Hardware SysEx frame: `F0 0F 05 [ch] [model] 0A [nibble-encoded payload] F7`
/// Multi-message files (e.g. from SysEx Librarian) are supported; the first
/// AllSequences (0x0A) message found is processed.
///
/// Hardware decoded payload layout:
///   `[0..4]`       – base RAM address (machine-specific; discarded)
///   `[4..240]`     – 59 × 4-byte BE pool offsets for seqs 0–58 (relative to pool start)
///   `[240..240+P]` – event pool: 12 lead zeros + 9-byte HW state + packed seq data (P bytes total)
///   `[240+P..]`    – 60 × 186-byte sequence headers then 29-byte global section
///
/// Seq 59 has no ptr-table entry and is always treated as undefined.
///
/// Output is the same on-disk SixtySequences layout produced by `allsequences_to_disk`.
/// `interleaved_programs`, if provided, must be exactly 60 × 530 = 31800 bytes.
pub fn allsequences_hardware_sysex_to_disk(raw: &[u8], interleaved_programs: Option<&[u8]>) -> Result<Vec<u8>> {
    const PTR_TABLE_SIZE: usize = 240;
    const SYSEX_HEADER_SIZE: usize = 186;
    const DISK_HEADER_SIZE: usize = 188;
    const HEADER_COUNT: usize = 60;
    const SYSEX_GLOBAL_SIZE: usize = 29;
    const DISK_GLOBAL_SIZE: usize = 21;
    const GLOBAL_INTERNAL_BYTES: usize = 8;
    const SYSEX_HEADERS_TOTAL: usize = SYSEX_HEADER_SIZE * HEADER_COUNT; // 11160
    const DISK_HEADERS_TOTAL: usize = DISK_HEADER_SIZE * HEADER_COUNT;   // 11280
    const DISK_GLOBAL_START: usize = DISK_HEADERS_TOTAL;                  // 11280
    const DISK_GLOBAL_END: usize = DISK_GLOBAL_START + DISK_GLOBAL_SIZE;  // 11301
    const EVENT_LEAD_ZEROS: usize = 12;
    const POOL_PREAMBLE: usize = 21; // 12 lead zeros + 9 HW state bytes
    const BLOCK_SIZE: usize = 512;
    const PROGRAMS_DISK_OFFSET: usize = 11776;
    const PROGRAMS_SIZE: usize = 60 * 530;    // 31800
    const SEQ_DATA_WITH_PROGRAMS: usize = 44032;
    const SEQ_DATA_NO_PROGRAMS: usize = 11776;
    // minimum: ptr table + preamble + 1 header + global
    const MIN_DECODED: usize = PTR_TABLE_SIZE + POOL_PREAMBLE + SYSEX_HEADERS_TOTAL + SYSEX_GLOBAL_SIZE;

    if let Some(progs) = interleaved_programs {
        if progs.len() != PROGRAMS_SIZE {
            return Err(Error::InvalidSysEx("interleaved programs must be exactly 60 × 530 bytes"));
        }
    }

    // Find the AllSequences message: F0 0F 05 xx xx 0A
    let msg_start = raw.windows(6)
        .position(|w| w[0] == 0xF0 && w[1] == 0x0F && w[2] == 0x05 && w[5] == 0x0A)
        .ok_or(Error::InvalidSysEx("hardware SysEx: AllSequences (0x0A) message not found"))?;

    let nibble_start = msg_start + 6;
    let f7_offset = raw[nibble_start..].iter().position(|&b| b == 0xF7)
        .ok_or(Error::InvalidSysEx("hardware SysEx: missing F7 terminator"))?;
    let decoded = decode_sysex_nibbles(&raw[nibble_start..nibble_start + f7_offset]);

    if decoded.len() < MIN_DECODED {
        return Err(Error::InvalidSysEx("hardware AllSequences: payload too short after nibble decode"));
    }

    // Pool occupies decoded[PTR_TABLE_SIZE..PTR_TABLE_SIZE+pool_size].
    // pool_size includes the 12 lead zeros at pool[0..12]; total_packed excludes them.
    let pool_size = decoded.len() - PTR_TABLE_SIZE - SYSEX_HEADERS_TOTAL - SYSEX_GLOBAL_SIZE;
    if pool_size < POOL_PREAMBLE {
        return Err(Error::InvalidSysEx("hardware AllSequences: event pool too small"));
    }
    let total_packed = pool_size.saturating_sub(EVENT_LEAD_ZEROS) as u32;
    let pool = &decoded[PTR_TABLE_SIZE..PTR_TABLE_SIZE + pool_size];
    let sysex_headers_start = PTR_TABLE_SIZE + pool_size;
    let sysex_global_start = sysex_headers_start + SYSEX_HEADERS_TOTAL;

    // decoded[0..4] = base RAM address (discard).
    // decoded[4..240] = 59 pool offsets for seqs 0..58 (relative to pool[0]).
    let mut seq_offsets = [0u32; 59];
    for i in 0..59usize {
        let o = 4 + i * 4;
        seq_offsets[i] = u32::from_be_bytes([decoded[o], decoded[o+1], decoded[o+2], decoded[o+3]]);
    }

    // Compute ds[i] for seqs 0..58 from ptr-table differences.
    // ds[59] = 0 (no ptr-table entry; always undefined in hardware format).
    let mut ds = [0u32; HEADER_COUNT];
    for i in 0..59usize {
        if seq_offsets[i] == 0 && i > 0 {
            ds[i] = 0;
            continue;
        }
        let next = (i + 1..59)
            .find(|&j| seq_offsets[j] > 0)
            .map(|j| seq_offsets[j])
            .unwrap_or(total_packed);
        ds[i] = next.saturating_sub(seq_offsets[i]);
    }

    // Only count sequences that are actually written (non-0xFF header).
    // Hardware ptr tables can have stale non-zero entries for sequences whose
    // headers were subsequently marked undefined (0xFF) — e.g. after deletion.
    let sum_ds: usize = (0..59usize)
        .filter(|&slot| {
            let d = ds[slot] as usize;
            if d == 0 { return false; }
            decoded[sysex_headers_start + slot * SYSEX_HEADER_SIZE] != 0xFF
        })
        .map(|slot| ds[slot] as usize)
        .sum();
    // Clean declared strips the 21-byte pool preamble that hardware includes.
    let clean_declared = (EVENT_LEAD_ZEROS + sum_ds) as u32;

    // Compute block-padded on-disk size.
    let padded_total: usize = (0..HEADER_COUNT).filter_map(|slot| {
        let d = ds[slot] as usize;
        if d == 0 { return None; }
        let hdr_src = sysex_headers_start + slot * SYSEX_HEADER_SIZE;
        if decoded[hdr_src] == 0xFF { return None; }
        Some((d + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE)
    }).sum();

    let seq_data_offset = if interleaved_programs.is_some() { SEQ_DATA_WITH_PROGRAMS } else { SEQ_DATA_NO_PROGRAMS };
    let file_size = seq_data_offset + padded_total;
    let mut out = vec![0u8; file_size];

    // Write disk headers: expand each 186-byte SysEx header to 188 bytes (2 trailing zeros).
    // Stamp our ptr-table-derived ds into [183..186] for seqs 0..58.
    for slot in 0..HEADER_COUNT {
        let hdr_src = sysex_headers_start + slot * SYSEX_HEADER_SIZE;
        let sysex_hdr = &decoded[hdr_src..hdr_src + SYSEX_HEADER_SIZE];
        let dst = slot * DISK_HEADER_SIZE;
        out[dst..dst + SYSEX_HEADER_SIZE].copy_from_slice(sysex_hdr);
        if slot < 59 {
            let d = ds[slot];
            out[dst + 183] = ((d >> 16) & 0xFF) as u8;
            out[dst + 184] = ((d >> 8)  & 0xFF) as u8;
            out[dst + 185] = ( d        & 0xFF) as u8;
        }
        // bytes [dst+186..dst+188] remain zero
    }

    // Write disk global: sysex_global[8..29] stripped of 8 SD-1-internal bytes.
    // Overwrite declared field with clean value (strips 21-byte hardware pool preamble).
    let sysex_global = &decoded[sysex_global_start..sysex_global_start + SYSEX_GLOBAL_SIZE];
    out[DISK_GLOBAL_START..DISK_GLOBAL_END].copy_from_slice(&sysex_global[GLOBAL_INTERNAL_BYTES..]);
    out[DISK_GLOBAL_START + 2..DISK_GLOBAL_START + 6].copy_from_slice(&clean_declared.to_be_bytes());

    if let Some(progs) = interleaved_programs {
        out[PROGRAMS_DISK_OFFSET..PROGRAMS_DISK_OFFSET + PROGRAMS_SIZE].copy_from_slice(progs);
    }

    // Write sequence event data (block-padded per sequence).
    let mut out_pos = seq_data_offset;
    for slot in 0..59usize {
        let d = ds[slot] as usize;
        if d == 0 { continue; }
        let dst = slot * DISK_HEADER_SIZE;
        if out[dst] == 0xFF { continue; }
        let pool_off = seq_offsets[slot] as usize;
        if pool_off + d > pool.len() {
            return Err(Error::InvalidSysEx("hardware AllSequences: sequence event data out of bounds"));
        }
        out[out_pos..out_pos + d].copy_from_slice(&pool[pool_off..pool_off + d]);
        out_pos += (d + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE;
    }
    // slot 59: ds[59] = 0, no event data to write

    Ok(out)
}

/// INT0 user bank: 10 banks × 6 patches, indexed by b10 value (0–59).
pub const INT0_PROGRAMS: [&str; 60] = [
    // bank 0
    "ARTIC-ELATE", "OLYMPIANO",   "ALTO-SAX",    "MERLIN",       "WAY-FAT",     "GROOVE-KIT",
    // bank 1
    "ALLS-FAIR",   "IN-CONCERT",  "SOLOTRUMPET", "INSPIRED",     "AMEN-CHOIR",  "PASSION",
    // bank 2
    "SYMPHONY",    "MY-DESIRE",   "MUTED-HORNS", "STACK-BASS",   "DRAWBARS-1",  "SONOTAR",
    // bank 3
    "STRINGS",     "BRASS-STAB",  "MANDOLIN",    "CROWN-CHOIR",  "TUBULAR HIT", "JAZZ-KIT",
    // bank 4
    "STRUM-ME",    "LUNAR",       "BLUES-HARP",  "WIDEPUNCH",    "BRIGHT-PNO",  "PIPE-ORGAN1",
    // bank 5
    "MALLETS",     "SWEEPER",     "KOTO-DREAMS", "SWELL-SAW",    "WILBUR",      "MEATY-KIT",
    // bank 6
    "FIDDLE",      "PEDAL-STEEL", "BANJO-BANJO", "CLOCK-BELLS",  "THE-QUEEN",   "ROCK-KIT-2",
    // bank 7
    "SMOOTH-STRG", "DARK-HALL",   "GUITAR-PADS", "FANFARE",      "MINI-LEAD",   "NORM-1-KIT",
    // bank 8
    "STRATOS-VOX", "FUNKY-CLAV2", "COOL-FLUTES", "OH-BE-EX",     "DANCEBASS-2", "WOODY-PERC",
    // bank 9
    "ANNABELL",    "FUNK-GUITAR", "ELEC-BASS2",  "CLEAR-GUITAR", "STUDIO-CITY", "MEAN-KIT-1",
];

/// ROM program table: ROM0 (indices 0–59) then ROM1 (indices 60–119).
/// Accessed as rom_index = (b10 & 0x7F) + 8.  Indices 0–7 are unreachable (enc < 0).
pub const ROM_ALL_PROGRAMS: [&str; 120] = [
    // ROM 0 — 60 programs (indices 0–59)
    // bank 0
    "ITS-A-SYNTH", "ZIRCONIUM",    " FAT-BRASS",   "STAR-DRIVE ", " WONDERS ",   "SAW-O-LIFE",
    // bank 1
    "DIGIPIANO-1", "NEW-PLANET",   " DANGEROUS ",  " FUNKYCLAV ", "WARM-TINES",  "METAL-TINES",
    // bank 2
    " BIG-PIANO ", "BRIGHT-PNO2",  " SYN-PIANO ",  "TRANS-PIANO", "CLASSIC-PNO", "HARPSICHORD",
    // bank 3
    "DOUBLE-REED", " TENOR-SAX ",  "WOODFLUTE",    " CHIFFLUTE ", "MALLET+FLTS", "FLUTE-VIL",
    // bank 4
    " STARBRASS ", " FRENCHORN ",  " TOP-BRASS ",  "FLUGEL-STRG", "  BRASSY  ",  "SYNTH-HORNS",
    // bank 5
    "SMAK-BASS",   "BEBOP-BASS",   "ELEC-BASS",    "SYNTHBASS",   "DANCE-BASS",  "BUZZ-BASS",
    // bank 6
    " ORGANIZER",  "NASTY-ORGAN",  "CATHEDRAL-1",  "TIMBRE-ORG",  "ANGELBREATH", " VERYBREATH",
    // bank 7
    "SWELLSTRNGS", " PIZZICATO ",  "LUSH-STRNGS",  "GOLDEN-HARP", "REZ-STRINGS", " ORCH+SOLO ",
    // bank 8
    "REEL-STEEL",  "SUN-N-MOON",   "FLANG-CLEAN",  " FUZZ-LEAD",  "SPANISH-GTR", " 12-STRING",
    // bank 9
    "KITCHN-SINK", "PERCUSSION",   "FUSION-KIT",   " BALLAD-KIT", "SYNTH-KIT",   "ROCKIN-KIT",
    // ROM 1 — 60 programs (indices 60–119)
    // bank 0
    "OMNIVERSE",   "FLASH-BACK",   " SD1-PAD",     "SQUARE-PAD",  "NU-MEANING",  "ASCENSION",
    // bank 1
    "IN-DEMAND",   " FM-PIANO",    "MANY-ROADS",   "DEEP-TINES",  "PURE-TINE",   "INNOCENCE",
    // bank 2
    "STUDIO-GRND", " POP-GRND",    "JAZZ-GRAND",   "CHURCH-GRND", "CLASSIC-GND", "BOWS+GRAND",
    // bank 3
    "SOPRANO-SAX", " ALTO-SAX",    "BARI+HORNS",   "HARMONICA",   "SHAKUHACHI",  " PICCOLO +",
    // bank 4
    " ODYSSEY",    "MANY-LEADS",   " FUNK-LEAD",   "FUNKY-STABS", " CHICAGO",    "MUTED-HORN",
    // bank 5
    "MOOG-MUTE",   "  ANAREZO",    "PERKY-MOOG",   "CROSS-BASS",  "SLICK-ELEC",  "BLEACHBASS",
    // bank 6
    "JAZZ-ORGAN",  "DIRTY-ORGAN",  "NU-CHOIR",     "DIGITALIAN",  " CHORALE-2",  "90-S-VOX",
    // bank 7
    "DRAMA-STGS",  "NU-STRINGS",   "LUSH-STRG-2",  "  VIOLIN",    "   CELLO",    "  QUARTET",
    // bank 8
    "DREAM-GTR",   "JAZZ-GUITAR",  "ELEC-GUITAR",  "DIST-GTR",    "   NU-BEL",   " MULTI-BELL",
    // bank 9
    "DRUMS-MAP-R", "808-MAP-R",    "SLAM-MAP-R",   "MULTI-PERCS", "ORCH-PERKS",  " INDO-AFRO",
];

/// Decode a program name from a 530-byte program slot.
///
/// Masks the MSB of each name byte (the SD-1 uses high bits for mute flags),
/// then strips trailing nulls and spaces.
pub fn program_name_from_slot(slot_data: &[u8]) -> String {
    let raw = &slot_data[PROGRAM_NAME_OFFSET..PROGRAM_NAME_OFFSET + PROGRAM_NAME_LEN];
    let masked: Vec<u8> = raw.iter().map(|&b| b & 0x7F).collect();
    let end = masked.iter().rposition(|&b| b != 0 && b != b' ')
        .map(|i| i + 1)
        .unwrap_or(0);
    String::from_utf8_lossy(&masked[..end]).into_owned()
}

/// Decode a track program assignment byte (b10) to a human-readable label.
///
/// Encoding:
/// - `0x00–0x3B` (0–59): RAM slot; resolved via `disk_programs` if provided,
///   otherwise falls back to `INT0_PROGRAMS` (factory init bank).
/// - `0x7F`: no program change on sequence recall.
/// - `0x80–0xFE`: ROM program; `enc = b10 & 0x7F`, `rom_index = enc + 8`.
/// - `0xFF`: track inactive.
pub fn decode_b10(b10: u8, disk_programs: Option<&[String]>) -> String {
    match b10 {
        0xFF => "(inactive)".to_string(),
        0x7F => "(no prog change)".to_string(),
        0x00..=0x3B => {
            let idx = b10 as usize;
            let name = disk_programs
                .and_then(|p| p.get(idx))
                .map(|s| s.as_str())
                .or_else(|| INT0_PROGRAMS.get(idx).copied())
                .unwrap_or("?");
            format!("RAM[{}]={}", idx, name)
        }
        b if b & 0x80 != 0 => {
            let enc = b & 0x7F;
            let rom_index = enc as usize + 8;
            let bank_label = if rom_index < 60 { "ROM0" } else { "ROM1" };
            let name = ROM_ALL_PROGRAMS.get(rom_index).copied().unwrap_or("?");
            format!("{}[enc={}]={}", bank_label, enc, name)
        }
        other => format!("b10=0x{:02X}(?)", other),
    }
}

/// Reverse of `interleave_sixty_programs`: convert on-disk SixtyPrograms data back
/// to the AllPrograms SysEx payload order (programs 0,1,2,...,59 in sequence).
///
/// Even bytes (positions 0,2,4,...) form programs 0–29; odd bytes form programs 30–59.
/// Concatenating the two de-interleaved streams gives the original payload.
pub fn deinterleave_sixty_programs(data: &[u8]) -> Result<Vec<u8>> {
    let expected = SIXTY_PROGRAMS_COUNT * PROGRAM_SIZE;
    if data.len() != expected {
        return Err(Error::InvalidSysEx("SixtyPrograms disk data must be exactly 60 × 530 bytes"));
    }
    let half = 30 * PROGRAM_SIZE; // 15900
    let mut result = vec![0u8; expected];
    // even bytes → programs 0–29 (first half of output)
    for (i, &b) in data.iter().step_by(2).enumerate() {
        result[i] = b;
    }
    // odd bytes → programs 30–59 (second half of output)
    for (i, &b) in data.iter().skip(1).step_by(2).enumerate() {
        result[half + i] = b;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysex::MessageType;

    fn make_program_sysex(name: &[u8; 11]) -> SysExPacket {
        let mut payload = vec![0u8; 530];
        payload[498..509].copy_from_slice(name);
        SysExPacket {
            message_type: MessageType::OneProgram,
            midi_channel: 0,
            model: 0,
            payload,
        }
    }

    fn make_preset_sysex() -> SysExPacket {
        SysExPacket {
            message_type: MessageType::OnePreset,
            midi_channel: 0,
            model: 0,
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
            message_type: MessageType::OnePreset,
            midi_channel: 0,
            model: 0,
            payload: vec![0u8; 530],
        };
        assert!(matches!(Program::from_sysex(&pkt), Err(crate::Error::WrongMessageType { .. })));
    }

    #[test]
    fn program_wrong_size_returns_error() {
        let pkt = SysExPacket {
            message_type: MessageType::OneProgram,
            midi_channel: 0,
            model: 0,
            payload: vec![0u8; 100],
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
    fn allsequences_to_disk_layout() {
        // Build a minimal AllSequences payload with one defined sequence (orig_loc=0, ds=170).
        // 170 bytes < 512, so on disk it occupies one full 512-byte block.
        const HEADER_COUNT: usize = 60;
        const HEADER_SIZE: usize = 186;
        const HEADERS_TOTAL: usize = HEADER_COUNT * HEADER_SIZE;
        const SEQ_DATA_LEN: usize = 170;  // one sequence, 170 unpadded bytes
        const GLOBAL_SIZE: usize = 29;
        const EVENT_LEAD: usize = 12;

        // declared = EVENT_LEAD + SEQ_DATA_LEN
        let declared: u32 = EVENT_LEAD as u32 + SEQ_DATA_LEN as u32;

        // Build one defined sequence header: orig_loc=0, data_size=170 at bytes 183-185
        let mut headers = vec![0u8; HEADERS_TOTAL];
        headers[0] = 0;  // orig_loc = 0 (defined)
        headers[183] = 0; headers[184] = 0; headers[185] = SEQ_DATA_LEN as u8;
        // All other slots remain 0xFF-unmarked (byte 0 = 0 = defined), but we only care
        // about slots where byte 0 != 0xFF. Remaining 59 slots have byte 0 = 0 too, which
        // makes them "defined" with ds=0. Zero-size sequences contribute nothing to output.

        // Build packed event data: 12 lead zeros + 170 bytes of seq data
        let seq_bytes: Vec<u8> = (0..SEQ_DATA_LEN as u8).collect();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; 240]);       // ptr table
        payload.extend_from_slice(&[0u8; EVENT_LEAD]); // 12 lead zeros (skipped)
        payload.extend_from_slice(&seq_bytes);         // 170 bytes of seq data
        payload.extend_from_slice(&headers);
        let mut global = [0u8; GLOBAL_SIZE];
        global[10..14].copy_from_slice(&declared.to_be_bytes());
        payload.extend_from_slice(&global);

        let disk = allsequences_to_disk(&payload, None).unwrap();

        // File size = 11776 + 512 (170 bytes padded to one 512-byte block)
        assert_eq!(disk.len(), 11776 + 512);
        // Each SysEx header (186 bytes) is expanded to 188 bytes on disk (2 trailing zeros).
        // Verify slot 0: first 186 bytes match, trailing 2 bytes are zero.
        assert_eq!(&disk[..186], &headers[..186]);
        assert_eq!(disk[186], 0); assert_eq!(disk[187], 0);
        // Global at [11280..11301] = sysex_global[8..29] (strips 8 SD-1-internal bytes).
        assert_eq!(&disk[11280..11301], &global[8..]);
        // Padding at [11301..11776] all zeros
        assert!(disk[11301..11776].iter().all(|&b| b == 0));
        // Sequence data at [11776..11776+170] — matches seq_bytes
        assert_eq!(&disk[11776..11776 + SEQ_DATA_LEN], seq_bytes.as_slice());
        // Padding bytes [11776+170..11776+512] are zero
        assert!(disk[11776 + SEQ_DATA_LEN..11776 + 512].iter().all(|&b| b == 0));
    }

    #[test]
    fn allsequences_to_disk_rejects_short_payload() {
        let result = allsequences_to_disk(&[0u8; 100], None);
        assert!(result.is_err());
    }

    #[test]
    fn disk_to_allsequences_round_trips_via_disk() {
        // Build a minimal payload with one defined sequence (slot 0, ds=170),
        // slots 1..59 all marked undefined (0xFF) — matching real SD-1 disk layout.
        // Undefined headers have bytes 183..185 pre-zeroed to match what
        // allsequences_hardware_sysex_to_disk stamps (ds=0), so the round-trip is
        // lossless from the first iteration.
        const HEADER_COUNT: usize = 60;
        const HEADER_SIZE: usize = 186;
        const HEADERS_TOTAL: usize = HEADER_COUNT * HEADER_SIZE;
        const SEQ_DATA_LEN: usize = 170;
        const GLOBAL_SIZE: usize = 29;
        const EVENT_LEAD: usize = 12;

        let declared: u32 = EVENT_LEAD as u32 + SEQ_DATA_LEN as u32;
        let mut headers = vec![0u8; HEADERS_TOTAL];
        // Slot 0: defined, ds=170
        headers[183] = 0; headers[184] = 0; headers[185] = SEQ_DATA_LEN as u8;
        // Slots 1..59: undefined (0xFF except bytes 183..185 which are 0).
        // Hardware path stamps ds=0 into bytes 183..185 of undefined headers, so
        // pre-zeroing those bytes ensures the round-trip does not change them.
        for s in 1..HEADER_COUNT {
            let off = s * HEADER_SIZE;
            for b in 0..HEADER_SIZE { headers[off + b] = 0xFF; }
            headers[off + 183] = 0; headers[off + 184] = 0; headers[off + 185] = 0;
        }

        let seq_bytes: Vec<u8> = (0..SEQ_DATA_LEN as u8).collect();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; 240]);
        payload.extend_from_slice(&[0u8; EVENT_LEAD]);
        payload.extend_from_slice(&seq_bytes);
        payload.extend_from_slice(&headers);
        let mut global = [0u8; GLOBAL_SIZE];
        global[10..14].copy_from_slice(&declared.to_be_bytes());
        payload.extend_from_slice(&global);

        // Convert to disk format.
        let disk = allsequences_to_disk(&payload, None).unwrap();

        // disk_to_allsequences now returns hardware-compatible SysEx (F0…F7, nibble-encoded).
        let recovered = disk_to_allsequences(&disk, false).unwrap();

        // Round-trip: feed HW SysEx back through allsequences_sysex_to_disk and verify
        // the reconstructed disk is byte-for-byte identical.
        let disk2 = allsequences_sysex_to_disk(&recovered, None).unwrap();
        assert_eq!(disk, disk2, "disk→hw_sysex→disk round-trip must be lossless");
    }

    #[test]
    fn disk_to_allsequences_rejects_short_disk() {
        let result = disk_to_allsequences(&[0u8; 100], false);
        assert!(result.is_err());
    }

    // ── ThirtySequences round-trip ────────────────────────────────────────────

    fn make_thirty_seq_payload(ds: usize) -> Vec<u8> {
        // Build a minimal AllSequences payload with one defined sequence (slot 0, ds bytes).
        const HEADER_COUNT: usize = 60;
        const HEADER_SIZE: usize = 186;
        const GLOBAL_SIZE: usize = 29;
        const EVENT_LEAD: usize = 12;
        let declared: u32 = EVENT_LEAD as u32 + ds as u32;
        let mut headers = vec![0u8; HEADER_COUNT * HEADER_SIZE];
        headers[0] = 0x00;   // slot 0 defined
        headers[183] = (ds >> 16) as u8;
        headers[184] = (ds >> 8) as u8;
        headers[185] = ds as u8;
        // slots 1-29: byte 0 = 0 (defined, ds=0 → no data); slots 30-59: 0xFF
        for s in 30..HEADER_COUNT {
            headers[s * HEADER_SIZE] = 0xFF;
        }
        let seq_bytes: Vec<u8> = (0..ds as u8).collect();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; 240]);
        payload.extend_from_slice(&[0u8; EVENT_LEAD]);
        payload.extend_from_slice(&seq_bytes);
        payload.extend_from_slice(&headers);
        let mut global = [0u8; GLOBAL_SIZE];
        global[10..14].copy_from_slice(&declared.to_be_bytes());
        payload.extend_from_slice(&global);
        payload
    }

    #[test]
    fn thirty_sequences_to_disk_layout() {
        const DS: usize = 170;
        let payload = make_thirty_seq_payload(DS);
        let disk = thirty_sequences_to_disk(&payload, None).unwrap();

        // File size = 6144 + 512 (DS padded to one 512-byte block)
        assert_eq!(disk.len(), 6144 + 512);
        // Slot 0 header: first 186 SysEx bytes appear in first 186 disk bytes, trailing 2 are zero.
        let sysex_headers_start = payload.len() - 29 - 60 * 186;
        assert_eq!(&disk[..186], &payload[sysex_headers_start..sysex_headers_start + 186]);
        assert_eq!(disk[186], 0); assert_eq!(disk[187], 0);
        // Global at [5640..5661] = sysex_global[8..29]
        let global_in_payload = &payload[payload.len() - 29..];
        assert_eq!(&disk[5640..5661], &global_in_payload[8..]);
        // Padding at [5661..6144] all zeros
        assert!(disk[5661..6144].iter().all(|&b| b == 0));
        // Sequence data at [6144..6144+DS]
        let seq_bytes: Vec<u8> = (0..DS as u8).collect();
        assert_eq!(&disk[6144..6144 + DS], seq_bytes.as_slice());
    }

    #[test]
    fn thirty_sequences_round_trips_via_disk() {
        const DS: usize = 170;
        let payload = make_thirty_seq_payload(DS);
        let disk = thirty_sequences_to_disk(&payload, None).unwrap();
        // disk_to_thirty_sequences returns HW SysEx; import via auto-detect to 60-seq disk.
        let hw_sysex = disk_to_thirty_sequences(&disk).unwrap();
        let disk_sixty = allsequences_sysex_to_disk(&hw_sysex, None).unwrap();
        // Seq 0 event data lands at the 60-seq data offset (11776).
        let seq_bytes: Vec<u8> = (0..DS as u8).collect();
        assert_eq!(&disk_sixty[11776..11776 + DS], seq_bytes.as_slice(),
            "seq 0 event data must survive ThirtySeq→HW SysEx→SixtySeq conversion");
    }

    #[test]
    fn thirty_sequences_round_trips_with_zeroed_global() {
        const DS: usize = 200;
        const HEADER_COUNT: usize = 60;
        const HEADER_SIZE: usize = 186;
        let mut headers = vec![0u8; HEADER_COUNT * HEADER_SIZE];
        headers[0] = 0x00;
        headers[183] = 0; headers[184] = 0; headers[185] = DS as u8;
        for s in 30..HEADER_COUNT { headers[s * HEADER_SIZE] = 0xFF; }

        let seq_bytes: Vec<u8> = (0..DS as u8).collect();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; 240]);
        payload.extend_from_slice(&[0u8; 12]);
        payload.extend_from_slice(&seq_bytes);
        payload.extend_from_slice(&headers);
        payload.extend_from_slice(&[0u8; 29]); // global all zeros

        let disk = thirty_sequences_to_disk(&payload, None).unwrap();
        assert_eq!(disk.len(), 6144 + 512);

        // Export as HW SysEx; declared is synthesized correctly even with zeroed on-disk global.
        let hw_sysex = disk_to_thirty_sequences(&disk).unwrap();
        let disk_sixty = allsequences_sysex_to_disk(&hw_sysex, None).unwrap();
        assert_eq!(&disk_sixty[11776..11776 + DS], seq_bytes.as_slice(),
            "event data must survive zeroed-global thirty-seq round-trip");

        // Second round-trip via SixtySeq must be stable.
        let hw_sysex2 = disk_to_allsequences(&disk_sixty, false).unwrap();
        let disk_sixty2 = allsequences_sysex_to_disk(&hw_sysex2, None).unwrap();
        assert_eq!(disk_sixty, disk_sixty2, "second round-trip must be stable");
    }

    #[test]
    fn disk_to_thirty_sequences_rejects_short_disk() {
        let result = disk_to_thirty_sequences(&[0u8; 100]);
        assert!(result.is_err());
    }

    #[test]
    fn thirty_sequences_to_disk_rejects_short_payload() {
        let result = thirty_sequences_to_disk(&[0u8; 100], None);
        assert!(result.is_err());
    }

    #[test]
    fn thirty_sequences_slots_30_to_59_ignored_on_write() {
        // Slots 30-59 defined in the payload should NOT appear in the ThirtySeq output.
        const DS: usize = 100;
        const HEADER_COUNT: usize = 60;
        const HEADER_SIZE: usize = 186;
        let declared: u32 = 12 + (DS * 2) as u32; // two sequences + lead zeros
        let mut headers = vec![0u8; HEADER_COUNT * HEADER_SIZE];
        // slot 0: defined, ds=DS
        headers[0] = 0x00;
        headers[183] = 0; headers[184] = 0; headers[185] = DS as u8;
        // slot 30: also defined, ds=DS (should be ignored in ThirtySeq output)
        let off30 = 30 * HEADER_SIZE;
        headers[off30] = 0x1E;
        headers[off30 + 183] = 0; headers[off30 + 184] = 0; headers[off30 + 185] = DS as u8;

        let seq_bytes: Vec<u8> = (0..(DS * 2) as u8).collect();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; 240]);
        payload.extend_from_slice(&[0u8; 12]);
        payload.extend_from_slice(&seq_bytes);
        payload.extend_from_slice(&headers);
        let mut global = [0u8; 29];
        global[10..14].copy_from_slice(&declared.to_be_bytes());
        payload.extend_from_slice(&global);

        let disk = thirty_sequences_to_disk(&payload, None).unwrap();
        // Only slot 0's data (DS bytes, padded to 512) should appear after offset 6144.
        // If slot 30 were also written, file would be 6144 + 1024 bytes.
        assert_eq!(disk.len(), 6144 + 512, "slot 30 must not be written to ThirtySeq output");
        let first_ds_bytes: Vec<u8> = (0..DS as u8).collect();
        assert_eq!(&disk[6144..6144 + DS], first_ds_bytes.as_slice());
    }

    #[test]
    fn program_to_sysex_round_trips() {
        let pkt = make_program_sysex(b"SYSEXRTRIP ");
        let prog = Program::from_sysex(&pkt).unwrap();
        let rebuilt_pkt = prog.to_sysex(0);
        let reparsed = Program::from_sysex(&rebuilt_pkt).unwrap();
        assert_eq!(reparsed.to_bytes(), prog.to_bytes());
    }

    // ── program_name_from_slot ────────────────────────────────────────────────

    fn slot_with_name(name: &[u8; 11]) -> Vec<u8> {
        let mut slot = vec![0u8; PROGRAM_SIZE];
        slot[PROGRAM_NAME_OFFSET..PROGRAM_NAME_OFFSET + PROGRAM_NAME_LEN].copy_from_slice(name);
        slot
    }

    #[test]
    fn program_name_from_slot_trims_trailing_spaces() {
        let slot = slot_with_name(b"COOL-FLUTES");
        assert_eq!(program_name_from_slot(&slot), "COOL-FLUTES");
    }

    #[test]
    fn program_name_from_slot_trims_trailing_nulls_and_spaces() {
        let slot = slot_with_name(b"STRINGS    ");
        assert_eq!(program_name_from_slot(&slot), "STRINGS");
    }

    #[test]
    fn program_name_from_slot_masks_high_bits() {
        // Name bytes with MSB set (SD-1 mute flags) should be masked to 7-bit ASCII
        let mut name = *b"COOL-FLUTES";
        name[0] |= 0x80; // 'C' with high bit set
        let slot = slot_with_name(&name);
        assert_eq!(program_name_from_slot(&slot), "COOL-FLUTES");
    }

    #[test]
    fn program_name_from_slot_all_null_returns_empty() {
        let slot = slot_with_name(b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00");
        assert_eq!(program_name_from_slot(&slot), "");
    }

    #[test]
    fn program_name_from_slot_preserves_internal_spaces() {
        let slot = slot_with_name(b"TUBULAR HIT");
        assert_eq!(program_name_from_slot(&slot), "TUBULAR HIT");
    }

    // ── decode_b10 ───────────────────────────────────────────────────────────

    #[test]
    fn decode_b10_inactive() {
        assert_eq!(decode_b10(0xFF, None), "(inactive)");
    }

    #[test]
    fn decode_b10_no_prog_change() {
        assert_eq!(decode_b10(0x7F, None), "(no prog change)");
    }

    #[test]
    fn decode_b10_ram_slot_0_fallback_to_int0() {
        assert_eq!(decode_b10(0x00, None), "RAM[0]=ARTIC-ELATE");
    }

    #[test]
    fn decode_b10_ram_slot_5_fallback_to_int0() {
        assert_eq!(decode_b10(0x05, None), "RAM[5]=GROOVE-KIT");
    }

    #[test]
    fn decode_b10_ram_last_slot_fallback_to_int0() {
        assert_eq!(decode_b10(0x3B, None), "RAM[59]=MEAN-KIT-1");
    }

    #[test]
    fn decode_b10_ram_uses_disk_programs_when_provided() {
        let progs: Vec<String> = (0..60).map(|i| format!("MYPROG{:02}", i)).collect();
        assert_eq!(decode_b10(0x03, Some(&progs)), "RAM[3]=MYPROG03");
    }

    #[test]
    fn decode_b10_rom0_enc0() {
        // b10=0x80: enc=0, rom_index=8, ROM0 → " DANGEROUS "
        assert_eq!(decode_b10(0x80, None), "ROM0[enc=0]= DANGEROUS ");
    }

    #[test]
    fn decode_b10_rom0_reel_steel() {
        // REEL-STEEL is ROM0 bank8 patch0 → index 48 → enc = 48-8 = 40 → b10 = 0x80|40 = 0xA8
        assert_eq!(decode_b10(0xA8, None), "ROM0[enc=40]=REEL-STEEL");
    }

    #[test]
    fn decode_b10_rom1_first_entry() {
        // ROM1 starts at index 60 → enc = 60-8 = 52 → b10 = 0x80|52 = 0xB4
        assert_eq!(decode_b10(0xB4, None), "ROM1[enc=52]=OMNIVERSE");
    }

    #[test]
    fn decode_b10_undefined_range_shows_hex() {
        // 0x3C–0x7E are not defined (between RAM and ROM)
        assert_eq!(decode_b10(0x3C, None), "b10=0x3C(?)");
    }

    // ── decode_sysex_nibbles ────────────────────────────────────────────────────

    #[test]
    fn decode_sysex_nibbles_basic() {
        // 0xAB nibble-encoded as [0x0A, 0x0B]; 0xCD as [0x0C, 0x0D]
        let nibbles = [0x0A, 0x0B, 0x0C, 0x0D];
        let decoded = decode_sysex_nibbles(&nibbles);
        assert_eq!(decoded, vec![0xAB, 0xCD]);
    }

    #[test]
    fn decode_sysex_nibbles_round_trips_via_nybblize() {
        // nybblize is available via sysex module; verify inverse relationship
        let original: Vec<u8> = (0u8..=255).collect();
        let nibbles: Vec<u8> = original.iter()
            .flat_map(|&b| [(b >> 4) & 0x0F, b & 0x0F])
            .collect();
        let decoded = decode_sysex_nibbles(&nibbles);
        assert_eq!(decoded, original);
    }

    #[test]
    fn decode_sysex_nibbles_odd_length_truncates() {
        let nibbles = [0x01, 0x02, 0x03]; // 3 bytes → 1 pair + 1 orphan
        let decoded = decode_sysex_nibbles(&nibbles);
        assert_eq!(decoded, vec![0x12]); // only the complete pair
    }

    // ── allsequences_hardware_sysex_to_disk ────────────────────────────────────

    fn make_hw_sysex_payload(seq0_data: &[u8]) -> Vec<u8> {
        // Build a minimal hardware AllSequences decoded payload with one defined sequence.
        const PTR_TABLE_SIZE: usize = 240;
        const SYSEX_HEADER_SIZE: usize = 186;
        const HEADER_COUNT: usize = 60;
        const SYSEX_GLOBAL_SIZE: usize = 29;
        const EVENT_LEAD_ZEROS: usize = 12;
        const POOL_PREAMBLE: usize = 21;

        let ds = seq0_data.len();
        // Hardware pool layout:
        //   pool[0..12]          = EVENT_LEAD_ZEROS (leading marker)
        //   pool[12..21]         = 9 HW state bytes (POOL_PREAMBLE - EVENT_LEAD_ZEROS)
        //   pool[21..21+ds]      = sequence event data
        //   pool[21+ds..33+ds]   = EVENT_LEAD_ZEROS (trailing sentinel)
        // pool_size = 12 + 21 + ds = EVENT_LEAD_ZEROS + POOL_PREAMBLE + ds = 33 + ds
        // total_packed = pool_size - EVENT_LEAD_ZEROS = POOL_PREAMBLE + ds = 21 + ds
        let pool_size = EVENT_LEAD_ZEROS + POOL_PREAMBLE + ds;
        let total_packed = POOL_PREAMBLE + ds; // = pool_size - EVENT_LEAD_ZEROS

        let mut decoded = Vec::new();
        // ptr table (240 bytes): base addr at [0..4], seq0 offset at [4..8]
        decoded.extend_from_slice(&[0x00, 0x04, 0x90, 0x00]); // fake base RAM 0x49000
        decoded.extend_from_slice(&(POOL_PREAMBLE as u32).to_be_bytes()); // seq0 at pool[21]
        decoded.resize(PTR_TABLE_SIZE, 0); // remaining entries = 0 (undefined)

        // pool: leading EVENT_LEAD_ZEROS, then HW state, then seq data, then trailing sentinel
        decoded.resize(decoded.len() + EVENT_LEAD_ZEROS, 0); // 12 lead zeros
        decoded.resize(decoded.len() + 9, 0xBB);             // 9 HW state bytes (arbitrary)
        decoded.extend_from_slice(seq0_data);                 // seq 0 event data
        decoded.resize(decoded.len() + EVENT_LEAD_ZEROS, 0); // 12 trailing sentinel bytes

        // Verify pool_size matches
        assert_eq!(decoded.len() - PTR_TABLE_SIZE, pool_size);

        // 60 headers: slot 0 = defined (first byte not 0xFF), rest undefined
        let mut hdr0 = vec![0x00u8; SYSEX_HEADER_SIZE]; // defined
        // stamp total_packed-based ds into bytes 183-185
        hdr0[183] = ((ds >> 16) & 0xFF) as u8;
        hdr0[184] = ((ds >> 8)  & 0xFF) as u8;
        hdr0[185] = ( ds        & 0xFF) as u8;
        decoded.extend_from_slice(&hdr0);
        let undef_hdr = vec![0xFFu8; SYSEX_HEADER_SIZE];
        for _ in 1..HEADER_COUNT {
            decoded.extend_from_slice(&undef_hdr);
        }

        // global (29 bytes): write total_packed (= declared hardware value) at [10..14]
        // disk_global[2..6] = sysex_global[10..14] after stripping 8 internal bytes
        let mut global = vec![0u8; SYSEX_GLOBAL_SIZE];
        // declared = EVENT_LEAD_ZEROS + total_packed (hardware format)
        let hw_declared = (EVENT_LEAD_ZEROS + total_packed) as u32;
        global[10..14].copy_from_slice(&hw_declared.to_be_bytes());
        decoded.extend_from_slice(&global);

        decoded
    }

    fn nibble_encode(data: &[u8]) -> Vec<u8> {
        data.iter().flat_map(|&b| [(b >> 4) & 0x0F, b & 0x0F]).collect()
    }

    fn wrap_hw_sysex(payload: &[u8]) -> Vec<u8> {
        let mut raw = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, 0x0A];
        raw.extend_from_slice(&nibble_encode(payload));
        raw.push(0xF7);
        raw
    }

    #[test]
    fn hardware_sysex_missing_message_returns_error() {
        let raw = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, 0x03, 0xF7]; // 0x03 = AllPrograms
        let result = allsequences_hardware_sysex_to_disk(&raw, None);
        assert!(result.is_err(), "should fail: no 0x0A message");
    }

    #[test]
    fn hardware_sysex_missing_f7_returns_error() {
        let raw = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, 0x0A, 0x01, 0x02]; // no F7
        let result = allsequences_hardware_sysex_to_disk(&raw, None);
        assert!(result.is_err(), "should fail: no F7 terminator");
    }

    #[test]
    fn hardware_sysex_payload_too_short_returns_error() {
        let mut raw = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, 0x0A];
        raw.extend(std::iter::repeat(0x00).take(20)); // way too short
        raw.push(0xF7);
        let result = allsequences_hardware_sysex_to_disk(&raw, None);
        assert!(result.is_err(), "should fail: payload too short");
    }

    #[test]
    fn hardware_sysex_to_disk_basic_ds_and_declared() {
        let seq0_data: Vec<u8> = (0u8..100).collect(); // 100 bytes of event data
        let decoded_payload = make_hw_sysex_payload(&seq0_data);
        let raw = wrap_hw_sysex(&decoded_payload);

        let disk = allsequences_hardware_sysex_to_disk(&raw, None)
            .expect("should succeed");

        // disk[0] = first byte of slot 0 header; should be 0x00 (defined)
        assert_ne!(disk[0], 0xFF, "slot 0 header should be defined");

        // ds stamped into disk[183..186] for slot 0
        let ds_in_hdr = u32::from_be_bytes([0, disk[183], disk[184], disk[185]]) as usize;
        assert_eq!(ds_in_hdr, 100, "ds for slot 0 should be 100");

        // clean_declared at disk[11280+2..11280+6] = EVENT_LEAD_ZEROS + sum(ds) = 12 + 100 = 112
        let clean_declared = u32::from_be_bytes([disk[11282], disk[11283], disk[11284], disk[11285]]);
        assert_eq!(clean_declared, 112, "clean_declared should be EVENT_LEAD_ZEROS + ds = 112");
    }

    #[test]
    fn hardware_sysex_to_disk_round_trips() {
        let seq0_data: Vec<u8> = (0u8..200).collect();
        let decoded_payload = make_hw_sysex_payload(&seq0_data);
        let raw = wrap_hw_sysex(&decoded_payload);

        let disk = allsequences_hardware_sysex_to_disk(&raw, None)
            .expect("hardware → disk");

        // Round-trip: disk → HW SysEx → disk again; both disks must match.
        let payload = disk_to_allsequences(&disk, false)
            .expect("disk → allsequences hw sysex");
        let disk2 = allsequences_sysex_to_disk(&payload, None)
            .expect("allsequences → disk");

        assert_eq!(disk, disk2, "hardware-converted disk must survive round-trip");
    }

    #[test]
    fn hardware_sysex_to_disk_seq_data_preserved() {
        let seq0_data: Vec<u8> = (10u8..60).collect(); // 50 distinct bytes
        let decoded_payload = make_hw_sysex_payload(&seq0_data);
        let raw = wrap_hw_sysex(&decoded_payload);

        let disk = allsequences_hardware_sysex_to_disk(&raw, None)
            .expect("hardware → disk");

        // Seq data starts at disk[11776] (no-programs layout), block-padded to 512 bytes.
        // The first 50 bytes of disk[11776..] must match seq0_data.
        let seq_data_on_disk = &disk[11776..11776 + seq0_data.len()];
        assert_eq!(seq_data_on_disk, seq0_data.as_slice(), "seq 0 event data must be preserved");
    }

    #[test]
    fn hardware_sysex_multi_message_finds_allsequences() {
        // Build a file with two messages: AllPrograms (0x03) then AllSequences (0x0A)
        let seq0_data = vec![0x42u8; 30];
        let decoded_payload = make_hw_sysex_payload(&seq0_data);

        // Fake AllPrograms message (short)
        let mut raw = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, 0x03, 0x01, 0x02, 0xF7];
        // Then AllSequences
        raw.extend(wrap_hw_sysex(&decoded_payload));

        let disk = allsequences_hardware_sysex_to_disk(&raw, None)
            .expect("should find AllSequences in multi-message file");
        let ds = u32::from_be_bytes([0, disk[183], disk[184], disk[185]]);
        assert_eq!(ds, 30, "ds for seq 0 should be 30");
    }

    #[test]
    fn hardware_sysex_to_disk_against_real_4syx() {
        // Integration test: only runs if ~/Downloads/4.syx is present.
        let path = format!("{}/Downloads/4.syx", std::env::var("HOME").unwrap_or_default());
        let raw = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => return, // skip if file not available
        };

        let disk = allsequences_hardware_sysex_to_disk(&raw, None)
            .expect("4.syx must convert without error");

        // Basic sanity: disk is large enough to hold headers + global
        assert!(disk.len() > 11301, "output disk must exceed header+global minimum");

        // Round-trip must be byte-for-byte identical
        let payload = disk_to_allsequences(&disk, false).expect("disk→payload");
        let disk2 = allsequences_sysex_to_disk(&payload, None).expect("payload→disk");
        assert_eq!(disk, disk2, "4.syx round-trip must be identical");
    }

    // ── allsequences_sysex_to_disk (auto-detect) ───────────────────────────────

    #[test]
    fn autodetect_routes_library_sysex_via_allsequences_to_disk() {
        // Build a standard library-generated AllSequences SysEx (ptr_table[0] = 0).
        // Use a simple payload with two defined sequences.
        const DS: usize = 150;
        const HEADER_SIZE: usize = 186;
        const HEADER_COUNT: usize = 60;
        const PTR_TABLE: usize = 240;
        const EVENT_LEAD: usize = 12;
        const GLOBAL_SIZE: usize = 29;

        let mut hdr0 = [0u8; HEADER_SIZE];
        hdr0[183] = 0; hdr0[184] = 0; hdr0[185] = DS as u8;
        let mut hdr1 = [0u8; HEADER_SIZE];
        hdr1[183] = 0; hdr1[184] = 0; hdr1[185] = DS as u8;
        // Undefined headers: 0xFF except bytes 183..185 which are pre-zeroed.
        // allsequences_hardware_sysex_to_disk stamps ds=0 into those bytes, so
        // pre-zeroing ensures the round-trip is lossless from the first iteration.
        let mut undef = [0xFFu8; HEADER_SIZE];
        undef[183] = 0; undef[184] = 0; undef[185] = 0;
        let event_data: Vec<u8> = (0u8..100).cycle().take(DS * 2).collect();
        let declared = (EVENT_LEAD + DS * 2) as u32;
        let mut global = [0u8; GLOBAL_SIZE];
        global[10..14].copy_from_slice(&declared.to_be_bytes());

        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8; PTR_TABLE]); // ptr_table[0] = 0
        payload.extend_from_slice(&[0u8; EVENT_LEAD]);
        payload.extend_from_slice(&event_data);
        payload.extend_from_slice(&hdr0);
        payload.extend_from_slice(&hdr1);
        for _ in 2..HEADER_COUNT { payload.extend_from_slice(&undef); }
        payload.extend_from_slice(&global);

        // Wrap as a SysEx message (nibble-encode)
        let nibbles: Vec<u8> = payload.iter()
            .flat_map(|&b| [(b >> 4) & 0x0F, b & 0x0F])
            .collect();
        let mut raw = vec![0xF0u8, 0x0F, 0x05, 0x00, 0x00, 0x0A];
        raw.extend_from_slice(&nibbles);
        raw.push(0xF7);

        // Auto-detect should recognise ptr_table[0] == 0 → library path
        let disk = allsequences_sysex_to_disk(&raw, None).expect("autodetect library path");
        assert!(disk.len() > 11301);
        // Round-trip must survive
        let p2 = disk_to_allsequences(&disk, false).expect("disk→payload");
        let d2 = allsequences_sysex_to_disk(&p2, None).expect("payload→disk");
        assert_eq!(disk, d2);
    }

    #[test]
    fn autodetect_routes_hardware_sysex_via_hardware_path() {
        // Build a hardware SysEx (ptr_table[0] = non-zero base RAM addr).
        let seq0_data: Vec<u8> = (0u8..80).collect();
        let decoded_payload = make_hw_sysex_payload(&seq0_data);
        let raw = wrap_hw_sysex(&decoded_payload);

        // Auto-detect should recognise ptr_table[0] != 0 → hardware path
        let disk_auto = allsequences_sysex_to_disk(&raw, None)
            .expect("autodetect hardware path");
        let disk_hw = allsequences_hardware_sysex_to_disk(&raw, None)
            .expect("direct hardware path");
        assert_eq!(disk_auto, disk_hw, "auto-detect must produce identical output to direct hardware call");
    }

    #[test]
    fn autodetect_against_real_4syx() {
        let path = format!("{}/Downloads/4.syx", std::env::var("HOME").unwrap_or_default());
        let raw = match std::fs::read(&path) { Ok(b) => b, Err(_) => return };
        let disk = allsequences_sysex_to_disk(&raw, None).expect("autodetect 4.syx");
        let p = disk_to_allsequences(&disk, false).expect("disk→payload");
        let d2 = allsequences_sysex_to_disk(&p, None).expect("payload→disk");
        assert_eq!(disk, d2, "4.syx autodetect round-trip must be identical");
    }

    #[test]
    fn autodetect_against_real_shatterday() {
        let path = "/Volumes/Aux Brain/Music, canonical/SysEx Librarian/Shatterday/seq-DB final (all).syx";
        let raw = match std::fs::read(path) { Ok(b) => b, Err(_) => return };
        let disk = allsequences_sysex_to_disk(&raw, None).expect("autodetect shatterday");
        let p = disk_to_allsequences(&disk, false).expect("disk→payload");
        let d2 = allsequences_sysex_to_disk(&p, None).expect("payload→disk");
        assert_eq!(disk, d2, "shatterday autodetect round-trip must be identical");
    }

    #[test]
    fn hardware_sysex_to_disk_against_real_shatterday_seq_db() {
        // Integration test: only runs if the Shatterday seq-DB file is present.
        // Multi-message file: AllPrograms, AllPresets, button press, then AllSequences (0x0A).
        let path = "/Volumes/Aux Brain/Music, canonical/SysEx Librarian/Shatterday/seq-DB final (all).syx";
        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return,
        };

        let disk = allsequences_hardware_sysex_to_disk(&raw, None)
            .expect("seq-DB final (all).syx must convert without error");

        assert!(disk.len() > 11301, "output must exceed header+global minimum");

        // Round-trip must be byte-for-byte identical
        let payload = disk_to_allsequences(&disk, false).expect("disk→payload");
        let disk2 = allsequences_sysex_to_disk(&payload, None).expect("payload→disk");
        assert_eq!(disk, disk2, "seq-DB round-trip must be identical");
    }
}
