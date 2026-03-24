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
/// SD-1 on-disk SixtyPrograms format, which byte-interleaves even- and odd-indexed programs.
///
/// File byte layout: even bytes = programs 0,2,4,...,58 concatenated;
///                   odd bytes  = programs 1,3,5,...,59 concatenated.
pub fn interleave_sixty_programs(payload: &[u8]) -> Result<Vec<u8>> {
    let expected = SIXTY_PROGRAMS_COUNT * PROGRAM_SIZE;
    if payload.len() != expected {
        return Err(Error::InvalidSysEx("AllPrograms payload must be exactly 60 × 530 bytes"));
    }
    let even_data: Vec<u8> = (0..30)
        .flat_map(|k| &payload[k * 2 * PROGRAM_SIZE..(k * 2 + 1) * PROGRAM_SIZE])
        .copied()
        .collect();
    let odd_data: Vec<u8> = (0..30)
        .flat_map(|k| &payload[(k * 2 + 1) * PROGRAM_SIZE..(k * 2 + 2) * PROGRAM_SIZE])
        .copied()
        .collect();
    let mut result = vec![0u8; expected];
    for i in 0..15900 {
        result[2 * i]     = even_data[i];
        result[2 * i + 1] = odd_data[i];
    }
    Ok(result)
}

/// Convert an AllSequences SysEx payload to the SD-1 on-disk SixtySequences (No Programs) format.
///
/// SysEx AllSequences payload layout:
///   [0..240]            – 60 × 4-byte internal memory pointer table (SD-1 private; not written to disk)
///   [240..-(21+11280)]  – sequence event data (track offset tables + track data for all defined seqs)
///                          The first 12 bytes are an SD-1-internal header; actual data starts at +12.
///   [-(21+11280)..-21]  – 60 × 188-byte sequence headers
///   [-21..]             – 21-byte global section
///                          [0..2]  current selected sequence number (BE u16)
///                          [2..6]  sum of all sequence data sizes + 0xFC (BE u32)
///                          [6..21] global sequencer information (15 bytes)
///
/// On-disk SixtySequences (No Programs) layout:
///   [0..11280]          – 60 × 188-byte sequence headers
///   [11280..11282]      – current selected sequence number
///   [11282..11286]      – sum of all sequence data sizes + 0xFC
///   [11286..11301]      – global sequencer information
///   [11301..11776]      – zeros (475 bytes)
///   [11776..]           – sequence event data (seq_data_len bytes)
/// Convert an AllSequences SysEx payload to the on-disk SixtySequences format.
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
    const HEADER_SIZE: usize = 188;
    const HEADER_COUNT: usize = 60;
    const GLOBAL_SIZE: usize = 21;
    const HEADERS_TOTAL: usize = HEADER_SIZE * HEADER_COUNT; // 11280
    const MIN_PAYLOAD: usize = PTR_TABLE_SIZE + HEADERS_TOTAL + GLOBAL_SIZE;
    const GLOBAL_DISK_START: usize = HEADERS_TOTAL; // 11280
    const GLOBAL_DISK_END: usize = GLOBAL_DISK_START + GLOBAL_SIZE; // 11301
    const EVENT_LEAD_ZEROS: usize = 12;
    // Layout constants for the 60-programs variant
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

    let global_sec = &payload[payload.len() - GLOBAL_SIZE..];
    let headers_start = payload.len() - GLOBAL_SIZE - HEADERS_TOTAL;
    let headers_sec = &payload[headers_start..payload.len() - GLOBAL_SIZE];
    let event_data = &payload[PTR_TABLE_SIZE..headers_start];

    if event_data.len() < EVENT_LEAD_ZEROS {
        return Err(Error::InvalidSysEx("AllSequences payload: event data section too short"));
    }

    // Global section bytes 2–5 (BE u32) = sum of all seq data sizes + 0xFC.
    // seq_data_len is the UNPADDED sum; on disk each sequence is padded to a 512-byte block.
    let size_sum = u32::from_be_bytes([global_sec[2], global_sec[3], global_sec[4], global_sec[5]]);
    let seq_data_len = (size_sum as usize).saturating_sub(0xFC);

    let event_start = EVENT_LEAD_ZEROS;
    if event_data.len() < event_start + seq_data_len {
        return Err(Error::InvalidSysEx("AllSequences payload: event data too short for declared seq_data_len"));
    }
    let actual_event_data = &event_data[event_start..event_start + seq_data_len];

    // Compute on-disk padded size: each defined sequence rounded up to 512-byte block.
    const BLOCK_SIZE: usize = 512;
    let padded_total: usize = (0..HEADER_COUNT)
        .filter_map(|slot| {
            let hdr = &headers_sec[slot * HEADER_SIZE..(slot + 1) * HEADER_SIZE];
            if hdr[0] == 0xFF { return None; }  // undefined slot
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

    out[..HEADERS_TOTAL].copy_from_slice(headers_sec);
    out[GLOBAL_DISK_START..GLOBAL_DISK_END].copy_from_slice(global_sec);

    if let Some(progs) = interleaved_programs {
        // Embed programs at 11776; zeros at 43576..44032 are already zeroed.
        out[PROGRAMS_DISK_OFFSET..PROGRAMS_DISK_OFFSET + PROGRAMS_SIZE].copy_from_slice(progs);
    }

    // Write each defined sequence's data at its block-padded position.
    // SysEx event data is packed (no padding); disk format pads each to 512 bytes.
    let mut in_pos = 0usize;
    let mut out_pos = seq_data_offset;
    for slot in 0..HEADER_COUNT {
        let hdr = &headers_sec[slot * HEADER_SIZE..(slot + 1) * HEADER_SIZE];
        if hdr[0] == 0xFF { continue; }
        let ds = u32::from_be_bytes([0, hdr[183], hdr[184], hdr[185]]) as usize;
        out[out_pos..out_pos + ds].copy_from_slice(&actual_event_data[in_pos..in_pos + ds]);
        in_pos += ds;
        out_pos += (ds + BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE;
    }

    Ok(out)
}

/// Reverse of `interleave_sixty_programs`: convert on-disk SixtyPrograms data back
/// to the AllPrograms SysEx payload order (programs 0,1,2,...,59 in sequence).
pub fn deinterleave_sixty_programs(data: &[u8]) -> Result<Vec<u8>> {
    let expected = SIXTY_PROGRAMS_COUNT * PROGRAM_SIZE;
    if data.len() != expected {
        return Err(Error::InvalidSysEx("SixtyPrograms disk data must be exactly 60 × 530 bytes"));
    }
    let even_data: Vec<u8> = data.iter().step_by(2).copied().collect();
    let odd_data:  Vec<u8> = data.iter().skip(1).step_by(2).copied().collect();
    let mut result = vec![0u8; expected];
    for k in 0..30 {
        let dst_even = k * 2 * PROGRAM_SIZE;
        let dst_odd  = (k * 2 + 1) * PROGRAM_SIZE;
        result[dst_even..dst_even + PROGRAM_SIZE]
            .copy_from_slice(&even_data[k * PROGRAM_SIZE..(k + 1) * PROGRAM_SIZE]);
        result[dst_odd..dst_odd + PROGRAM_SIZE]
            .copy_from_slice(&odd_data[k * PROGRAM_SIZE..(k + 1) * PROGRAM_SIZE]);
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
        const HEADER_SIZE: usize = 188;
        const HEADERS_TOTAL: usize = HEADER_COUNT * HEADER_SIZE;
        const SEQ_DATA_LEN: usize = 170;  // one sequence, 170 unpadded bytes
        const GLOBAL_SIZE: usize = 21;
        const EVENT_LEAD: usize = 12;

        // size_sum = SEQ_DATA_LEN + 0xFC
        let size_sum: u32 = SEQ_DATA_LEN as u32 + 0xFC;

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
        global[2..6].copy_from_slice(&size_sum.to_be_bytes());
        payload.extend_from_slice(&global);

        let disk = allsequences_to_disk(&payload, None).unwrap();

        // File size = 11776 + 512 (170 bytes padded to one 512-byte block)
        assert_eq!(disk.len(), 11776 + 512);
        // Headers at [0..11280]
        assert_eq!(&disk[..HEADERS_TOTAL], headers.as_slice());
        // Global at [11280..11301]
        assert_eq!(&disk[11280..11301], &global[..]);
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
    fn program_to_sysex_round_trips() {
        let pkt = make_program_sysex(b"SYSEXRTRIP ");
        let prog = Program::from_sysex(&pkt).unwrap();
        let rebuilt_pkt = prog.to_sysex(0);
        let reparsed = Program::from_sysex(&rebuilt_pkt).unwrap();
        assert_eq!(reparsed.to_bytes(), prog.to_bytes());
    }
}
