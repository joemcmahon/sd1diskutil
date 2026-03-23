// crates/sd1disk/src/types.rs
use std::borrow::Cow;
use crate::{Error, FileType, Result};
use crate::sysex::{MessageType, SysExPacket};

const PROGRAM_NAME_OFFSET: usize = 498;
const PROGRAM_NAME_LEN: usize = 11;
const PROGRAM_SIZE: usize = 530;
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
            payload: self.0.clone(),
        }
    }

    pub fn file_type(&self) -> FileType {
        FileType::OneSequence
    }
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
            payload,
        }
    }

    fn make_preset_sysex() -> SysExPacket {
        SysExPacket {
            message_type: MessageType::OnePreset,
            midi_channel: 0,
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
            payload: vec![0u8; 530],
        };
        assert!(matches!(Program::from_sysex(&pkt), Err(crate::Error::WrongMessageType { .. })));
    }

    #[test]
    fn program_wrong_size_returns_error() {
        let pkt = SysExPacket {
            message_type: MessageType::OneProgram,
            midi_channel: 0,
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
    fn program_to_sysex_round_trips() {
        let pkt = make_program_sysex(b"SYSEXRTRIP ");
        let prog = Program::from_sysex(&pkt).unwrap();
        let rebuilt_pkt = prog.to_sysex(0);
        let reparsed = Program::from_sysex(&rebuilt_pkt).unwrap();
        assert_eq!(reparsed.to_bytes(), prog.to_bytes());
    }
}
