// crates/sd1disk/src/sysex.rs
use crate::{Error, Result};

const SYSEX_START: u8 = 0xF0;
const ENSONIQ_CODE: u8 = 0x0F;
const VFX_FAMILY: u8 = 0x05;
const SYSEX_END: u8 = 0xF7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    Command,
    Error,
    OneProgram,
    AllPrograms,
    OnePreset,
    AllPresets,
    SingleSequence,
    AllSequences,
    TrackParameters,
    Unknown(u8),
}

impl MessageType {
    fn from_byte(b: u8) -> Self {
        match b {
            0x00 => MessageType::Command,
            0x01 => MessageType::Error,
            0x02 => MessageType::OneProgram,
            0x03 => MessageType::AllPrograms,
            0x04 => MessageType::OnePreset,
            0x05 => MessageType::AllPresets,
            0x09 => MessageType::SingleSequence,
            0x0A => MessageType::AllSequences,
            0x0B => MessageType::TrackParameters,
            other => MessageType::Unknown(other),
        }
    }

    fn to_byte(&self) -> u8 {
        match self {
            MessageType::Command         => 0x00,
            MessageType::Error           => 0x01,
            MessageType::OneProgram      => 0x02,
            MessageType::AllPrograms     => 0x03,
            MessageType::OnePreset       => 0x04,
            MessageType::AllPresets      => 0x05,
            MessageType::SingleSequence  => 0x09,
            MessageType::AllSequences    => 0x0A,
            MessageType::TrackParameters => 0x0B,
            MessageType::Unknown(b)      => *b,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            MessageType::Command         => "Command",
            MessageType::Error           => "Error",
            MessageType::OneProgram      => "OneProgram",
            MessageType::AllPrograms     => "AllPrograms",
            MessageType::OnePreset       => "OnePreset",
            MessageType::AllPresets      => "AllPresets",
            MessageType::SingleSequence  => "SingleSequence",
            MessageType::AllSequences    => "AllSequences",
            MessageType::TrackParameters => "TrackParameters",
            MessageType::Unknown(_)      => "Unknown",
        }
    }
}

pub struct SysExPacket {
    pub message_type: MessageType,
    pub midi_channel: u8,
    pub model: u8,
    pub payload: Vec<u8>,
}

impl SysExPacket {
    #[allow(clippy::manual_is_multiple_of)]
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 {
            return Err(Error::InvalidSysEx("packet too short"));
        }
        if bytes[0] != SYSEX_START {
            return Err(Error::InvalidSysEx("missing F0 start byte"));
        }
        if *bytes.last().unwrap() != SYSEX_END {
            return Err(Error::InvalidSysEx("missing F7 end byte"));
        }
        if bytes[1] != ENSONIQ_CODE {
            return Err(Error::InvalidSysEx("not an Ensoniq packet (expected 0F)"));
        }
        if bytes[2] != VFX_FAMILY {
            return Err(Error::InvalidSysEx("not a VFX family packet (expected 05)"));
        }
        let model = bytes[3];
        let midi_channel = bytes[4];
        let message_type = MessageType::from_byte(bytes[5]);
        let nybbles = &bytes[6..bytes.len() - 1];
        if nybbles.len() % 2 != 0 {
            return Err(Error::InvalidSysEx("odd number of nybble bytes"));
        }
        let payload = denybblize(nybbles);
        Ok(SysExPacket { message_type, midi_channel, model, payload })
    }

    /// Parse a byte stream that may contain one or more concatenated SysEx packets.
    /// Splits on F0/F7 boundaries and returns all valid packets.
    pub fn parse_all(bytes: &[u8]) -> Result<Vec<SysExPacket>> {
        let mut packets = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != SYSEX_START {
                i += 1;
                continue;
            }
            let end = match bytes[i..].iter().position(|&b| b == SYSEX_END) {
                Some(pos) => i + pos,
                None => return Err(Error::InvalidSysEx("unterminated SysEx packet")),
            };
            packets.push(SysExPacket::parse(&bytes[i..=end])?);
            i = end + 1;
        }
        if packets.is_empty() {
            return Err(Error::InvalidSysEx("no SysEx packets found"));
        }
        Ok(packets)
    }

    pub fn to_bytes(&self, channel: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(7 + self.payload.len() * 2);
        out.push(SYSEX_START);
        out.push(ENSONIQ_CODE);
        out.push(VFX_FAMILY);
        out.push(self.model);
        out.push(channel);
        out.push(self.message_type.to_byte());
        out.extend(nybblize(&self.payload));
        out.push(SYSEX_END);
        out
    }
}

pub fn nybblize(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for &b in data {
        out.push((b >> 4) & 0x0F);
        out.push(b & 0x0F);
    }
    out
}

pub fn denybblize(nybbles: &[u8]) -> Vec<u8> {
    nybbles.chunks(2).map(|pair| (pair[0] << 4) | pair[1]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sysex(msg_type: u8, payload_bytes: &[u8]) -> Vec<u8> {
        let mut pkt = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, msg_type];
        for &b in payload_bytes {
            pkt.push((b >> 4) & 0x0F);
            pkt.push(b & 0x0F);
        }
        pkt.push(0xF7);
        pkt
    }

    #[test]
    fn parse_one_program_header() {
        let payload = vec![0xABu8; 530];
        let sysex = make_sysex(0x02, &payload);
        let packet = SysExPacket::parse(&sysex).unwrap();
        assert_eq!(packet.message_type, MessageType::OneProgram);
        assert_eq!(packet.midi_channel, 0);
        assert_eq!(packet.payload, payload);
    }

    #[test]
    fn parse_one_preset_header() {
        let payload = vec![0x55u8; 48];
        let sysex = make_sysex(0x04, &payload);
        let packet = SysExPacket::parse(&sysex).unwrap();
        assert_eq!(packet.message_type, MessageType::OnePreset);
        assert_eq!(packet.payload.len(), 48);
        assert_eq!(packet.payload, payload);
    }

    #[test]
    fn denybblize_is_inverse_of_nybblize() {
        let original = (0u8..=255).collect::<Vec<_>>();
        let nybblized = nybblize(&original);
        let recovered = denybblize(&nybblized);
        assert_eq!(recovered, original);
    }

    #[test]
    fn to_bytes_round_trips() {
        let payload = vec![0x42u8; 530];
        let sysex = make_sysex(0x02, &payload);
        let packet = SysExPacket::parse(&sysex).unwrap();
        let rebuilt = packet.to_bytes(0x00);
        assert_eq!(rebuilt, sysex);
    }

    #[test]
    fn wrong_manufacturer_code_returns_error() {
        let mut bad = vec![0xF0, 0x41, 0x05, 0x00, 0x00, 0x02];
        bad.extend_from_slice(&[0x00; 10]);
        bad.push(0xF7);
        assert!(SysExPacket::parse(&bad).is_err());
    }

    #[test]
    fn missing_f7_tail_returns_error() {
        let mut bad = vec![0xF0, 0x0F, 0x05, 0x00, 0x00, 0x02];
        bad.extend_from_slice(&[0x00; 10]);
        assert!(SysExPacket::parse(&bad).is_err());
    }

    #[test]
    fn midi_channel_is_parsed() {
        let payload = vec![0u8; 48];
        let mut pkt = vec![0xF0, 0x0F, 0x05, 0x00, 0x09, 0x04];
        for &b in &payload {
            pkt.push((b >> 4) & 0x0F);
            pkt.push(b & 0x0F);
        }
        pkt.push(0xF7);
        let packet = SysExPacket::parse(&pkt).unwrap();
        assert_eq!(packet.midi_channel, 9);
    }
}
