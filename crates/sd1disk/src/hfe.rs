// crates/sd1disk/src/hfe.rs
//
// HFE v1 file read/write support for Ensoniq SD-1 disk images.
//
// Format reference: docs/superpowers/specs/2026-03-30-hfe-support-design.md
// Verified against: Ensoniq.hfe (real hardware-written SD-1 disk, 2026-03-30)

use std::path::Path;
use crate::{DiskImage, Error, Result};

// ── constants ─────────────────────────────────────────────────────────────────

const HFE_SIGNATURE: &[u8; 8] = b"HXCPICFE";
const HFE_REVISION: u8 = 0;
const NUM_TRACKS: usize = 80;
const NUM_SIDES: usize = 2;
const SECTORS_PER_TRACK: usize = 10;
const SECTOR_SIZE: usize = 512;

// Each side of a track is exactly 12,522 bytes of MFM bitstream.
const SIDE_LEN: usize = 12_522;
// Both sides interleaved = 25,044 bytes per track.
const TRACK_DATA_LEN: usize = SIDE_LEN * 2;
// HFE uses 512-byte blocks; 49 blocks holds 25,044 bytes (last 44 bytes unused).
const TRACK_BLOCK_STRIDE: usize = 49;

// Track layout constants (bytes in decoded data stream, before MFM encoding)
const GAP4A_COUNT: usize = 80;  // 0x4E bytes before first sync
const SYNC1_COUNT: usize = 12;  // 0x00 bytes (sync before gap1)
const GAP1_COUNT: usize = 50;   // 0x4E bytes after first sync

// Per-sector layout constants
const SYNC_COUNT: usize = 12;   // 0x00 bytes before A1* marks
const A1_COUNT: usize = 3;      // A1* sync marks
const GAP2_COUNT: usize = 22;   // 0x4E bytes between IDAM and DAM
// Gap 3 is dynamic: computed to pad total to SIDE_LEN exactly

// Fixed-structure sizes in decoded bytes
const IDAM_PAYLOAD: usize = 1 + 4 + 2; // FE + track + side + sec + size + crc×2
const DAM_PAYLOAD: usize = 1 + SECTOR_SIZE + 2; // FB + data[512] + crc×2

// ── CRC16-CCITT ───────────────────────────────────────────────────────────────

fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        let mut x = (crc >> 8) ^ byte as u16;
        x ^= x >> 4;
        crc = (crc << 8) ^ (x << 12) ^ (x << 5) ^ x;
    }
    crc
}

// ── MFM encode helpers ────────────────────────────────────────────────────────

/// Encode one data byte to 2 MFM-encoded HFE bytes (LSB-first bit storage).
///
/// For each data bit `d` (MSB first): clock `c = !(prev_bit | d)`.
/// Emit (c, d) in time order; pack 8 time-bits per output byte, bit-0 = oldest.
fn encode_byte(byte: u8, prev_bit: &mut u8) -> [u8; 2] {
    let mut out = [0u8; 2];
    for i in 0..8usize {
        let d = (byte >> (7 - i)) & 1;
        let c = if (*prev_bit == 0) && (d == 0) { 1u8 } else { 0u8 };
        let pos_c = 2 * i;
        let pos_d = 2 * i + 1;
        out[pos_c / 8] |= c << (pos_c % 8);
        out[pos_d / 8] |= d << (pos_d % 8);
        *prev_bit = d;
    }
    out
}

/// Emit the special A1* sync mark (0x4489, missing clock bit) as raw HFE bytes.
/// Callers must set `prev_bit = 1` after emitting a sync mark.
fn encode_a1_sync() -> [u8; 2] {
    [0x22, 0x91]
}

/// Encode a slice of bytes, appending to `out`. Updates `prev_bit` across all bytes.
fn encode_bytes(src: &[u8], prev_bit: &mut u8, out: &mut Vec<u8>) {
    for &b in src {
        let pair = encode_byte(b, prev_bit);
        out.extend_from_slice(&pair);
    }
}

// ── MFM decode helpers ────────────────────────────────────────────────────────

/// Convert an HFE byte (LSB-first) to 8 time-ordered bits.
fn hfe_to_bits(byte: u8) -> [u8; 8] {
    let mut bits = [0u8; 8];
    for i in 0..8 {
        bits[i] = (byte >> i) & 1;
    }
    bits
}

/// Extract data bits from MFM bit pairs.
///
/// Data bits are at odd positions (1, 3, 5, 7, ...) in the time-ordered stream.
/// Given a pair of HFE bytes (16 time-bits), the data byte has bits at positions
/// 1, 3, 5, 7, 9, 11, 13, 15 (0-indexed in the 16-bit stream).
fn decode_mfm_byte(pair: [u8; 2]) -> u8 {
    let bits0 = hfe_to_bits(pair[0]);
    let bits1 = hfe_to_bits(pair[1]);
    let all = [
        bits0[0], bits0[1], bits0[2], bits0[3],
        bits0[4], bits0[5], bits0[6], bits0[7],
        bits1[0], bits1[1], bits1[2], bits1[3],
        bits1[4], bits1[5], bits1[6], bits1[7],
    ];
    // Data bits at odd positions: 1,3,5,7,9,11,13,15 → byte MSB first
    let mut byte = 0u8;
    for i in 0..8 {
        byte |= all[2 * i + 1] << (7 - i);
    }
    byte
}

/// The A1* sync pattern in time-ordered bits (LSB-first HFE bytes [0x22, 0x91]).
/// 0x22 = 0b00100010, 0x91 = 0b10010001
const A1_PATTERN: [u8; 16] = [
    // 0x22 LSB-first: bits 0..7
    0, 1, 0, 0, 0, 1, 0, 0,
    // 0x91 LSB-first: bits 0..7
    1, 0, 0, 0, 1, 0, 0, 1,
];

/// Search `stream` for the A1* sync mark pattern, starting at `start_byte`.
/// Returns the byte offset of the first byte of the match, or None.
fn find_sync(stream: &[u8], start_byte: usize) -> Option<usize> {
    if stream.len() < 2 {
        return None;
    }
    'outer: for i in start_byte..stream.len().saturating_sub(1) {
        let b0 = hfe_to_bits(stream[i]);
        let b1 = hfe_to_bits(stream[i + 1]);
        let bits = [
            b0[0], b0[1], b0[2], b0[3], b0[4], b0[5], b0[6], b0[7],
            b1[0], b1[1], b1[2], b1[3], b1[4], b1[5], b1[6], b1[7],
        ];
        for j in 0..16 {
            if bits[j] != A1_PATTERN[j] {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

// ── track interleave/de-interleave ────────────────────────────────────────────

/// Interleave side 0 and side 1 into HFE track storage.
///
/// HFE stores tracks with side 0 and side 1 data interleaved in 256-byte chunks:
/// [side0 chunk 0][side1 chunk 0][side0 chunk 1][side1 chunk 1]...
fn interleave_sides(side0: &[u8], side1: &[u8]) -> Vec<u8> {
    debug_assert_eq!(side0.len(), SIDE_LEN);
    debug_assert_eq!(side1.len(), SIDE_LEN);
    let chunk = 256usize;
    let chunks = SIDE_LEN.div_ceil(chunk);
    let mut out = Vec::with_capacity(chunks * 2 * chunk);
    for i in 0..chunks {
        let start = i * chunk;
        let end0 = (start + chunk).min(side0.len());
        let end1 = (start + chunk).min(side1.len());
        let s0_chunk = &side0[start..end0];
        let s1_chunk = &side1[start..end1];
        out.extend_from_slice(s0_chunk);
        if s0_chunk.len() < chunk {
            out.extend(std::iter::repeat(0x4E).take(chunk - s0_chunk.len()));
        }
        out.extend_from_slice(s1_chunk);
        if s1_chunk.len() < chunk {
            out.extend(std::iter::repeat(0x4E).take(chunk - s1_chunk.len()));
        }
    }
    out
}

/// Extract one side's bitstream from HFE track data.
///
/// `side` is 0 or 1. Each 512-byte block contains a 256-byte side-0 chunk
/// followed by a 256-byte side-1 chunk. The last super-chunk may be partial
/// (when TRACK_DATA_LEN is not a multiple of 512), so we read whatever bytes
/// are available even if fewer than 256.
fn extract_side(raw: &[u8], side: u8, track_len: usize) -> Vec<u8> {
    let chunk = 256usize;
    let mut out = Vec::with_capacity(track_len);
    let offset = side as usize * chunk;
    let mut pos = offset;
    while pos < raw.len() && out.len() < track_len {
        // Take up to `chunk` bytes from raw, limited by what's available and what we need
        let available = raw.len() - pos;
        let needed = track_len - out.len();
        let take = chunk.min(available).min(needed);
        if take == 0 {
            break;
        }
        out.extend_from_slice(&raw[pos..pos + take]);
        pos += chunk * 2; // advance past both sides' chunks
    }
    out.truncate(track_len);
    out
}

// ── track encode ──────────────────────────────────────────────────────────────

/// Encode one side of one track into exactly SIDE_LEN (12,522) MFM bytes.
///
/// Layout (per spec):
///   Gap4a:  80 × 0x4E
///   Sync:   12 × 0x00
///   Gap1:   50 × 0x4E
///   × 10 sectors:
///     Sync: 12 × 0x00
///     3× A1* sync mark
///     IDAM: 0xFE + track + side + sector + 0x02 + CRC16(2)
///     Gap2: 22 × 0x4E
///     Sync: 12 × 0x00
///     3× A1* sync mark
///     DAM:  0xFB + data[512] + CRC16(2)
///     Gap3: ~75 × 0x4E  [dynamic, pads to exactly SIDE_LEN]
fn encode_track_side(img: &DiskImage, track: u8, side: u8) -> Vec<u8> {
    // Compute fixed encoded size to determine gap3 size.
    // Each decoded byte → 2 HFE bytes. A1* marks → 2 bytes each (already raw).
    //
    // Per sector fixed content (encoded bytes, before gap3):
    //   sync(12×2=24) + 3×A1*(3×2=6) + IDAM(7×2=14) + gap2(22×2=44)
    //   + sync(12×2=24) + 3×A1*(3×2=6) + DAM(515×2=1030) = 1148 encoded bytes
    // Preamble: gap4a(80×2=160) + sync(12×2=24) + gap1(50×2=100) = 284 encoded bytes
    // Total fixed = 284 + 10×1148 = 11764 encoded bytes
    // Remaining for gap3s: 12522 − 11764 = 758 encoded bytes across 10 sectors
    // Base gap3: 758/10 = 75 encoded bytes per sector (last sector absorbs remainder)

    let preamble_decoded = GAP4A_COUNT + SYNC1_COUNT + GAP1_COUNT; // 142
    // Per sector: sync(12) + 3×A1(each=2raw) + idam(7) + gap2(22) + sync(12) + 3×A1 + dam(515)
    // In encoded bytes: (12+22+12)*2 each side, A1*=2raw bytes each, IDAM=7*2=14, DAM=515*2=1030
    let per_sector_fixed_encoded =
        SYNC_COUNT * 2          // sync before IDAM
        + A1_COUNT * 2          // 3 A1* marks (2 bytes each)
        + IDAM_PAYLOAD * 2      // IDAM marker+fields+CRC
        + GAP2_COUNT * 2        // gap2
        + SYNC_COUNT * 2        // sync before DAM
        + A1_COUNT * 2          // 3 A1* marks
        + DAM_PAYLOAD * 2;      // DAM marker+data+CRC
    let total_fixed = preamble_decoded * 2 + SECTORS_PER_TRACK * per_sector_fixed_encoded;
    let remaining_encoded = SIDE_LEN.saturating_sub(total_fixed);
    // Each gap3 byte (0x4E) encodes to 2 HFE bytes. Work in decoded byte counts.
    // remaining_encoded must be even (all other structures are byte-pairs); assert this.
    debug_assert_eq!(remaining_encoded % 2, 0, "remaining encoded bytes must be even");
    let gap3_decoded_total = remaining_encoded / 2;
    let gap3_per_sector = gap3_decoded_total / SECTORS_PER_TRACK;
    let gap3_extra = gap3_decoded_total % SECTORS_PER_TRACK; // first N sectors get one extra

    let mut out = Vec::with_capacity(SIDE_LEN);
    let mut prev = 0u8;

    // Preamble
    encode_bytes(&vec![0x4E; GAP4A_COUNT], &mut prev, &mut out);
    encode_bytes(&vec![0x00; SYNC1_COUNT], &mut prev, &mut out);
    encode_bytes(&vec![0x4E; GAP1_COUNT], &mut prev, &mut out);

    for sector in 0..SECTORS_PER_TRACK as u8 {
        // Sync before IDAM
        encode_bytes(&vec![0x00; SYNC_COUNT], &mut prev, &mut out);

        // 3× A1* sync marks
        for _ in 0..A1_COUNT {
            out.extend_from_slice(&encode_a1_sync());
        }
        prev = 1;

        // IDAM: 0xFE track side sector size_code
        let idam_data = [0xA1u8, 0xA1, 0xA1, 0xFE, track, side, sector, 0x02];
        let crc = crc16_ccitt(&idam_data);
        let idam_payload = [0xFEu8, track, side, sector, 0x02,
                            (crc >> 8) as u8, (crc & 0xFF) as u8];
        encode_bytes(&idam_payload, &mut prev, &mut out);

        // Gap 2
        encode_bytes(&vec![0x4E; GAP2_COUNT], &mut prev, &mut out);

        // Sync before DAM
        encode_bytes(&vec![0x00; SYNC_COUNT], &mut prev, &mut out);

        // 3× A1* sync marks
        for _ in 0..A1_COUNT {
            out.extend_from_slice(&encode_a1_sync());
        }
        prev = 1;

        // DAM: 0xFB + sector data + CRC
        let block = (track as u16) * 20 + (side as u16) * 10 + (sector as u16);
        let sector_data = img.block(block).unwrap_or(&[0u8; 512][..]);
        let mut dam_crc_input = vec![0xA1u8, 0xA1, 0xA1, 0xFB];
        dam_crc_input.extend_from_slice(sector_data);
        let dam_crc = crc16_ccitt(&dam_crc_input);

        encode_bytes(&[0xFBu8], &mut prev, &mut out);
        encode_bytes(sector_data, &mut prev, &mut out);
        encode_bytes(&[(dam_crc >> 8) as u8, (dam_crc & 0xFF) as u8], &mut prev, &mut out);

        // Gap 3 (dynamic padding): gap3_per_sector is in decoded bytes;
        // each decoded 0x4E byte encodes to exactly 2 HFE bytes.
        let this_gap3_decoded = gap3_per_sector
            + if (sector as usize) < gap3_extra { 1 } else { 0 };
        encode_bytes(&vec![0x4E; this_gap3_decoded], &mut prev, &mut out);
    }

    // Pad or truncate to exactly SIDE_LEN
    if out.len() < SIDE_LEN {
        let filler_count = SIDE_LEN - out.len();
        // Encode 0x4E gap bytes for remaining space
        let filler_decoded = filler_count / 2;
        encode_bytes(&vec![0x4E; filler_decoded], &mut prev, &mut out);
        if out.len() < SIDE_LEN {
            out.push(0x49); // partial 0x4E if still short
        }
    }
    out.truncate(SIDE_LEN);
    out
}

// ── track decode ──────────────────────────────────────────────────────────────

/// Decode one side's bitstream into 10 sectors (each 512 bytes).
///
/// Returns an array of 10 Option<[u8; 512]>; each None means the sector was
/// not found. Fails with HfeCrcMismatch if a sector's CRC is wrong.
/// Fails with HfeMissingSector if any sector 0–9 is absent.
fn decode_track_side(
    raw: &[u8],
    track: u8,
    side: u8,
) -> Result<[[u8; SECTOR_SIZE]; SECTORS_PER_TRACK]> {
    let mut sectors: [Option<[u8; SECTOR_SIZE]>; SECTORS_PER_TRACK] =
        [None, None, None, None, None, None, None, None, None, None];

    let mut pos = 0usize;
    // We need to find all 10 sectors. Each sector has 2 A1* marks (IDAM + DAM).
    // We scan for each triple-A1* and determine if it's an IDAM or DAM by the marker byte.
    while pos < raw.len() {
        // Find next A1* mark
        let Some(sync_pos) = find_sync(raw, pos) else { break };

        // Skip consecutive A1* marks to find the first non-A1* byte after them
        let mut after_syncs = sync_pos;
        while after_syncs + 1 < raw.len() && find_sync(raw, after_syncs) == Some(after_syncs) {
            after_syncs += 2; // each A1* is 2 bytes
        }

        // Need at least one data byte after syncs
        if after_syncs + 2 > raw.len() {
            break;
        }

        // Read marker byte
        let marker = decode_mfm_byte([raw[after_syncs], raw[after_syncs + 1]]);

        match marker {
            0xFE => {
                // IDAM: track(1) + side(1) + sector(1) + size_code(1) + CRC(2) = 6 bytes
                let field_start = after_syncs + 2;
                if field_start + 12 > raw.len() {
                    pos = after_syncs + 2;
                    continue;
                }
                // Decode 4 field bytes + 2 CRC bytes = 6 bytes → 12 raw bytes
                let mut fields = [0u8; 6];
                for i in 0..6 {
                    fields[i] = decode_mfm_byte([
                        raw[field_start + i * 2],
                        raw[field_start + i * 2 + 1],
                    ]);
                }
                let idam_track = fields[0];
                let idam_side  = fields[1];
                let idam_sec   = fields[2];
                // fields[3] = size code (0x02 = 512)
                let stored_crc = ((fields[4] as u16) << 8) | (fields[5] as u16);

                // Verify CRC
                let crc_input = [0xA1u8, 0xA1, 0xA1, 0xFE,
                                  idam_track, idam_side, idam_sec, 0x02];
                let calc_crc = crc16_ccitt(&crc_input);
                if calc_crc != stored_crc {
                    return Err(Error::HfeCrcMismatch { track, side, sector: idam_sec });
                }

                // Validate sector number
                if idam_sec as usize >= SECTORS_PER_TRACK {
                    pos = after_syncs + 2;
                    continue;
                }

                // Now find the DAM (3× A1* + 0xFB + 512 bytes + 2 CRC)
                let dam_search_start = field_start + 12;
                let Some(dam_sync) = find_sync(raw, dam_search_start) else {
                    pos = after_syncs + 2;
                    continue;
                };
                let mut dam_after = dam_sync;
                while dam_after + 1 < raw.len() && find_sync(raw, dam_after) == Some(dam_after) {
                    dam_after += 2;
                }
                if dam_after + 2 > raw.len() {
                    pos = after_syncs + 2;
                    continue;
                }
                let dam_marker = decode_mfm_byte([raw[dam_after], raw[dam_after + 1]]);
                if dam_marker != 0xFB {
                    pos = after_syncs + 2;
                    continue;
                }

                // DAM data: 512 bytes + 2 CRC bytes = 514 bytes → 1028 raw bytes
                let data_start = dam_after + 2;
                if data_start + (SECTOR_SIZE + 2) * 2 > raw.len() {
                    pos = after_syncs + 2;
                    continue;
                }
                let mut data = [0u8; SECTOR_SIZE];
                for i in 0..SECTOR_SIZE {
                    data[i] = decode_mfm_byte([
                        raw[data_start + i * 2],
                        raw[data_start + i * 2 + 1],
                    ]);
                }
                let crc_offset = data_start + SECTOR_SIZE * 2;
                let dam_crc_hi = decode_mfm_byte([raw[crc_offset], raw[crc_offset + 1]]);
                let dam_crc_lo = decode_mfm_byte([raw[crc_offset + 2], raw[crc_offset + 3]]);
                let stored_dam_crc = ((dam_crc_hi as u16) << 8) | (dam_crc_lo as u16);

                let mut dam_crc_input = vec![0xA1u8, 0xA1, 0xA1, 0xFB];
                dam_crc_input.extend_from_slice(&data);
                let calc_dam_crc = crc16_ccitt(&dam_crc_input);
                if calc_dam_crc != stored_dam_crc {
                    return Err(Error::HfeCrcMismatch { track, side, sector: idam_sec });
                }

                sectors[idam_sec as usize] = Some(data);
                pos = crc_offset + 4;
            }
            _ => {
                // Not an IDAM marker we recognise; advance past this sync
                pos = after_syncs + 2;
            }
        }
    }

    // Verify all 10 sectors were found
    let mut result = [[0u8; SECTOR_SIZE]; SECTORS_PER_TRACK];
    for (i, opt) in sectors.iter().enumerate() {
        match opt {
            Some(data) => result[i] = *data,
            None => return Err(Error::HfeMissingSector {
                track, side, sector: i as u8,
            }),
        }
    }
    Ok(result)
}

// ── public API ────────────────────────────────────────────────────────────────

/// Read an HFE v1 file and return a DiskImage.
pub fn read_hfe(path: &Path) -> Result<DiskImage> {
    let data = std::fs::read(path)?;

    // Verify signature and revision
    if data.len() < 512 {
        return Err(Error::InvalidHfe("file too short"));
    }
    if &data[0..8] != HFE_SIGNATURE {
        return Err(Error::InvalidHfe("bad signature (not HXCPICFE)"));
    }
    if data[8] != HFE_REVISION {
        return Err(Error::InvalidHfe("unsupported HFE revision (only revision 0 supported)"));
    }

    // Parse header
    let num_tracks = data[9] as usize;
    let _num_sides  = data[10] as usize;
    // track_list_block at offset 18 (u16 LE)
    let track_list_block = u16::from_le_bytes([data[18], data[19]]) as usize;
    let track_list_offset = track_list_block * 512;

    if track_list_offset + num_tracks * 4 > data.len() {
        return Err(Error::InvalidHfe("track list extends beyond file"));
    }

    let mut img = DiskImage::create();

    for track in 0..num_tracks {
        let tl_offset = track_list_offset + track * 4;
        let block_offset = u16::from_le_bytes([data[tl_offset], data[tl_offset + 1]]) as usize;
        let byte_length  = u16::from_le_bytes([data[tl_offset + 2], data[tl_offset + 3]]) as usize;

        let raw_start = block_offset * 512;
        let raw_end   = raw_start + byte_length;
        if raw_end > data.len() {
            return Err(Error::InvalidHfe("track data extends beyond file"));
        }
        let raw = &data[raw_start..raw_end];

        for side in 0..NUM_SIDES as u8 {
            let side_stream = extract_side(raw, side, byte_length / 2);
            let sectors = decode_track_side(&side_stream, track as u8, side)?;

            for sector in 0..SECTORS_PER_TRACK {
                let block = (track as u16) * 20 + (side as u16) * 10 + (sector as u16);
                let blk = img.block_mut(block)?;
                blk.copy_from_slice(&sectors[sector]);
            }
        }
    }

    Ok(img)
}

/// Write a DiskImage as an HFE v1 file. Uses atomic write (tmp → rename).
pub fn write_hfe(image: &DiskImage, path: &Path) -> Result<()> {
    let mut hfe: Vec<u8> = Vec::new();

    // ── Header block (512 bytes) ──────────────────────────────────────────────
    let mut header = vec![0u8; 512];
    header[0..8].copy_from_slice(HFE_SIGNATURE);
    header[8]  = HFE_REVISION;           // format revision
    header[9]  = NUM_TRACKS as u8;        // num tracks
    header[10] = NUM_SIDES as u8;         // num sides
    header[11] = 0x00;                    // track encoding: ISOIBM_MFM
    // bit rate: 250 kbps → stored as 250 in u16 LE at offset 12
    header[12] = 0xFA;                    // 250 = 0x00FA LE → [0xFA, 0x00]
    header[13] = 0x00;
    // RPM at offset 14: 0
    header[14] = 0x00;
    header[15] = 0x00;
    // interface mode at offset 16: 7 = GENERIC_SHUGART_DD
    header[16] = 0x07;
    header[17] = 0xFF; // dnu (do not use) — HFE v1 spec requires 0xFF
    // track list block at offset 18: block 1
    header[18] = 0x01;
    header[19] = 0x00;
    // write_allowed at 20: 0xFF
    header[20] = 0xFF;
    // single_step at 21: 0xFF
    header[21] = 0xFF;
    // track0s0_altencoding, track0s0_encoding at 22,23: 0xFF
    header[22] = 0xFF;
    header[23] = 0xFF;
    // track0s1_altencoding, track0s1_encoding at 24,25: 0xFF
    header[24] = 0xFF;
    header[25] = 0xFF;
    hfe.extend_from_slice(&header);

    // ── Track lookup table (block 1, 512 bytes) ───────────────────────────────
    // Each track occupies TRACK_BLOCK_STRIDE blocks (49) of 512 bytes = 25,088 bytes,
    // but only TRACK_DATA_LEN (25,044) bytes are used.
    // Track 0 starts at block 2 (after header + track list).
    let track_list_block_start: usize = 2; // first track data block
    let mut track_list = vec![0u8; 512];
    for t in 0..NUM_TRACKS {
        let block_offset = track_list_block_start + t * TRACK_BLOCK_STRIDE;
        let offset = t * 4;
        track_list[offset]     = (block_offset & 0xFF) as u8;
        track_list[offset + 1] = (block_offset >> 8) as u8;
        track_list[offset + 2] = (TRACK_DATA_LEN & 0xFF) as u8;
        track_list[offset + 3] = (TRACK_DATA_LEN >> 8) as u8;
    }
    hfe.extend_from_slice(&track_list);

    // ── Track data ────────────────────────────────────────────────────────────
    for track in 0..NUM_TRACKS as u8 {
        let side0 = encode_track_side(image, track, 0);
        let side1 = encode_track_side(image, track, 1);
        let interleaved = interleave_sides(&side0, &side1);
        // Pad to TRACK_BLOCK_STRIDE × 512 bytes
        let padded_len = TRACK_BLOCK_STRIDE * 512;
        hfe.extend_from_slice(&interleaved);
        let padding = padded_len - interleaved.len();
        hfe.extend(std::iter::repeat(0x4Eu8).take(padding));
    }

    // Atomic write
    let tmp = path.with_extension("hfe.tmp");
    std::fs::write(&tmp, &hfe)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_known_vector() {
        // CRC16-CCITT of "123456789" = 0x29B1
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn encode_byte_gap_byte() {
        // 0x4E with prev_bit=0 encodes to [0x49, 0x2A]
        assert_eq!(encode_byte(0x4E, &mut 0), [0x49, 0x2A]);
    }

    #[test]
    fn encode_byte_sync_zero() {
        // 0x00 with prev_bit=0 encodes to [0x55, 0x55]
        assert_eq!(encode_byte(0x00, &mut 0), [0x55, 0x55]);
    }

    #[test]
    fn encode_byte_updates_prev_bit() {
        // 0x01 has last data bit = 1; prev_bit should be 1 afterwards
        let mut prev = 0u8;
        encode_byte(0x01, &mut prev);
        assert_eq!(prev, 1);
    }

    #[test]
    fn encode_a1_sync_is_correct() {
        assert_eq!(encode_a1_sync(), [0x22, 0x91]);
    }

    #[test]
    fn decode_encode_round_trips_byte() {
        // Any non-sync byte should survive encode → decode
        for b in 0x00u8..=0xFEu8 {
            let mut prev = 0u8;
            let pair = encode_byte(b, &mut prev);
            let decoded = decode_mfm_byte(pair);
            assert_eq!(decoded, b, "round trip failed for byte 0x{:02X}", b);
        }
    }

    #[test]
    fn find_sync_locates_a1_pattern() {
        // Build a stream with two gap bytes then an A1* mark
        let gap = encode_byte(0x4E, &mut 0);
        let a1  = encode_a1_sync();
        let mut stream = vec![];
        stream.extend_from_slice(&gap);
        stream.extend_from_slice(&gap);
        stream.extend_from_slice(&a1);
        stream.extend_from_slice(&gap);
        assert_eq!(find_sync(&stream, 0), Some(4));
    }

    #[test]
    fn extract_side_recovers_side0() {
        // Build a minimal interleaved block: side0=[0xAA; 256], side1=[0xBB; 256]
        let mut raw = vec![0u8; 512];
        raw[0..256].fill(0xAA);
        raw[256..512].fill(0xBB);
        let s0 = extract_side(&raw, 0, 256);
        let s1 = extract_side(&raw, 1, 256);
        assert!(s0.iter().all(|&b| b == 0xAA), "side0 should be 0xAA");
        assert!(s1.iter().all(|&b| b == 0xBB), "side1 should be 0xBB");
    }

    #[test]
    fn encode_track_side_is_correct_length() {
        let img = DiskImage::create();
        let side = encode_track_side(&img, 0, 0);
        assert_eq!(side.len(), SIDE_LEN,
            "encoded track side should be exactly {} bytes", SIDE_LEN);
    }

    #[test]
    fn round_trip_blank_image() {
        let img = DiskImage::create();
        let tmp = std::env::temp_dir().join("sd1_hfe_roundtrip.hfe");
        write_hfe(&img, &tmp).expect("write_hfe failed");
        let img2 = read_hfe(&tmp).expect("read_hfe failed");
        assert_eq!(img.data, img2.data, "blank image should survive HFE round trip");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn crc_mismatch_returns_error() {
        let img = DiskImage::create();
        let tmp = std::env::temp_dir().join("sd1_hfe_crctest.hfe");
        write_hfe(&img, &tmp).expect("write_hfe failed");

        // Find the first sector's DAM data payload in the encoded stream and corrupt a byte.
        // Layout (encoded bytes, side 0 of track 0):
        //   Preamble: (80+12+50)*2 = 284 bytes
        //   Sector 0: sync(24) + 3×A1(6) + IDAM(14) + gap2(44) + sync(24) + 3×A1(6) = 118 bytes
        //             then DAM marker(2) + data starts at offset 284+118+2 = 404
        // Track data starts at block 2 (offset 2*512 = 1024). Side 0 is in first 256-byte
        // chunks of interleaved data; data at offset 1024+0, 1024+512, 1024+1024, ...
        // The DAM data is well within the first 256-byte chunk of side0 is at 1024..1280.
        // The DAM data payload starts at encoded byte 404 of the side0 stream, which maps to
        // interleaved offset: chunk 404/256=1 → interleaved byte 512 + (404%256)=512+148=660
        // Physical offset in file: 1024 + 660 = 1684. Corrupt a byte well inside the DAM data.
        let mut raw = std::fs::read(&tmp).unwrap();
        // Corrupt a byte in the middle of the first sector's DAM data payload.
        // Encoded side0 byte 500 is well inside the first sector's 512-byte data block.
        // byte 500 → interleaved chunk index 500/256=1, byte within chunk 500%256=244
        // interleaved offset = 1*512 + 244 = 756. File offset = 1024 + 756 = 1780.
        let corrupt_offset = 1024 + 756;
        raw[corrupt_offset] ^= 0xFF;
        std::fs::write(&tmp, &raw).unwrap();

        let result = read_hfe(&tmp);
        assert!(result.is_err(), "corrupted HFE should return an error");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn block_sentinel_survives_round_trip() {
        let mut img = DiskImage::create();
        // Write a sentinel pattern to block 42
        let sentinel = [0xDE, 0xAD, 0xBE, 0xEF];
        img.block_mut(42).unwrap()[0..4].copy_from_slice(&sentinel);

        let tmp = std::env::temp_dir().join("sd1_hfe_sentinel.hfe");
        write_hfe(&img, &tmp).expect("write_hfe failed");
        let img2 = read_hfe(&tmp).expect("read_hfe failed");
        assert_eq!(&img2.block(42).unwrap()[0..4], &sentinel,
            "sentinel at block 42 should survive HFE round trip");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn reserved_blocks_survive_round_trip() {
        let img = DiskImage::create();
        let tmp = std::env::temp_dir().join("sd1_hfe_reserved.hfe");
        write_hfe(&img, &tmp).expect("write_hfe failed");
        let img2 = read_hfe(&tmp).expect("read_hfe failed");
        // OS data blocks 0–22 should be identical
        for b in 0u16..23 {
            assert_eq!(img.block(b).unwrap(), img2.block(b).unwrap(),
                "block {} should survive HFE round trip", b);
        }
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn sector_numbering_is_zero_based() {
        let img = DiskImage::create();
        // Encode track 0 side 0 and scan for IDAMs; all sector fields should be 0–9.
        let side = encode_track_side(&img, 0, 0);
        let mut pos = 0;
        let mut found_sectors = std::collections::BTreeSet::new();
        while pos < side.len() {
            let Some(sync) = find_sync(&side, pos) else { break };
            let mut after = sync;
            while after + 1 < side.len() && find_sync(&side, after) == Some(after) {
                after += 2;
            }
            if after + 2 > side.len() { break; }
            let marker = decode_mfm_byte([side[after], side[after + 1]]);
            if marker == 0xFE && after + 14 <= side.len() {
                // sector field is at offset 2+2 (skip marker, skip track, skip side)
                let sec = decode_mfm_byte([side[after + 6], side[after + 7]]);
                found_sectors.insert(sec);
            }
            pos = after + 2;
        }
        assert_eq!(found_sectors.len(), 10, "should find 10 sector IDAMs");
        assert_eq!(*found_sectors.iter().next().unwrap(), 0, "first sector should be 0");
        assert_eq!(*found_sectors.iter().last().unwrap(), 9, "last sector should be 9");
    }
}
