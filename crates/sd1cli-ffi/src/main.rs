//! sd1cli-ffi: SD-1 disk utility that routes all operations through the sd1ffi C API.
//! Produces byte-identical output to sd1cli; any difference indicates a bug in the FFI layer.

use clap::{Parser, Subcommand};
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use sd1ffi::{
    Sd1DirectoryEntry,
    sd1_disk_create, sd1_disk_open, sd1_disk_save, sd1_disk_free, sd1_disk_free_blocks,
    sd1_disk_list, sd1_disk_find, sd1_disk_extract, sd1_disk_write, sd1_disk_delete,
    sd1_entries_free, sd1_bytes_free,
    sd1_sysex_parse_all, sd1_sysex_message_type, sd1_sysex_payload, sd1_sysex_payload_len,
    sd1_sysex_to_bytes, sd1_sysex_packets_free, sd1_sysex_free,
    sd1_program_from_sysex, sd1_program_from_bytes, sd1_program_bytes,
    sd1_program_to_sysex, sd1_program_file_type, sd1_program_free,
    sd1_preset_from_sysex, sd1_preset_from_bytes, sd1_preset_bytes,
    sd1_preset_to_sysex, sd1_preset_file_type, sd1_preset_free,
    sd1_sequence_from_sysex, sd1_sequence_from_bytes, sd1_sequence_bytes,
    sd1_sequence_to_sysex, sd1_sequence_file_type, sd1_sequence_free,
    sd1_allsequences_to_disk, sd1_disk_to_allsequences, sd1_disk_to_thirty_sequences,
    sd1_interleave_sixty_programs, sd1_deinterleave_sixty_programs,
    sd1_error_message,
};
use sd1disk::sysex::SysExPacket;

// ─── File-type byte constants (from sd1disk.h) ──────────────────────────────
const SD1_FILE_ONE_PROGRAM:      u8 = 0x0A;
const SD1_FILE_SIXTY_PROGRAMS:   u8 = 0x0D;
const SD1_FILE_ONE_PRESET:       u8 = 0x0E;
const SD1_FILE_TWENTY_PRESETS:   u8 = 0x10;
const SD1_FILE_ONE_SEQUENCE:     u8 = 0x11;
const SD1_FILE_THIRTY_SEQUENCES: u8 = 0x12;
const SD1_FILE_SIXTY_SEQUENCES:  u8 = 0x13;

// ─── SysEx message-type bytes ────────────────────────────────────────────────
const MSG_ONE_PROGRAM:     u8 = 0x02;
const MSG_ALL_PROGRAMS:    u8 = 0x03;
const MSG_ONE_PRESET:      u8 = 0x04;
const MSG_ALL_PRESETS:     u8 = 0x05;
const MSG_SINGLE_SEQUENCE: u8 = 0x09;
const MSG_ALL_SEQUENCES:   u8 = 0x0A;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn err_msg(code: i32) -> String {
    let ptr = sd1_error_message(code);
    unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| CString::new("?").unwrap())
}

fn path_cstr(p: &Path) -> CString {
    cstr(p.to_str().unwrap_or("?"))
}

/// Copy bytes from a heap-allocated FFI buffer to Vec, then free the buffer.
unsafe fn take_bytes(ptr: *mut u8, len: usize) -> Vec<u8> {
    let v = std::slice::from_raw_parts(ptr, len).to_vec();
    sd1_bytes_free(ptr, len);
    v
}

/// Build SD-1 SysEx: F0 0F 05 00 <channel> <type_byte> [nybblized payload] F7.
/// Matches sd1disk::sysex::SysExPacket::to_bytes with model=0.
fn wrap_sysex(channel: u8, msg_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(7 + payload.len() * 2);
    out.extend_from_slice(&[0xF0, 0x0F, 0x05, 0x00, channel, msg_type]);
    for &b in payload {
        out.push((b >> 4) & 0x0F);
        out.push(b & 0x0F);
    }
    out.push(0xF7);
    out
}

fn file_type_name(byte: u8) -> &'static str {
    match byte {
        0x0A => "OneProgram",
        0x0B => "SixPrograms",
        0x0C => "ThirtyPrograms",
        0x0D => "SixtyPrograms",
        0x0E => "OnePreset",
        0x0F => "TenPresets",
        0x10 => "TwentyPresets",
        0x11 => "OneSequence",
        0x12 => "ThirtySequences",
        0x13 => "SixtySequences",
        0x14 => "SystemExclusive",
        0x15 => "SystemSetup",
        0x16 => "OperatingSystem",
        _    => "Unknown",
    }
}

fn entry_name(e: &Sd1DirectoryEntry) -> &str {
    unsafe { CStr::from_ptr(e.name.as_ptr()) }.to_str().unwrap_or("?")
}

// ─── CLI definition ──────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "sd1cli-ffi", about = "Ensoniq SD-1 disk utility (FFI code path)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all files on a disk image
    List { image: PathBuf },

    /// Extract a file as SysEx
    Extract {
        image: PathBuf,
        name: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short, long, default_value = "0")]
        channel: u8,
    },

    /// Write a SysEx file to disk image
    Write {
        image: PathBuf,
        sysex: PathBuf,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(long)]
        overwrite: bool,
    },

    /// Delete a file from disk image
    Delete {
        image: PathBuf,
        name: String,
    },

    /// Create a blank disk image
    Create { image: PathBuf },
}

// ─── Command implementations ─────────────────────────────────────────────────

fn cmd_list(image_path: &Path) -> Result<(), String> {
    let path = path_cstr(image_path);
    let mut open_err = 0i32;
    let img = sd1_disk_open(path.as_ptr(), &mut open_err);
    if img.is_null() {
        return Err(format!("open failed: {}", err_msg(open_err)));
    }

    println!("{:<12} {:<22} {:>6} {:>6} {:>4}", "NAME", "TYPE", "BLOCKS", "BYTES", "SLOT");
    println!("{}", "-".repeat(56));

    let mut total = 0usize;
    for subdir in 0u8..4 {
        let mut count = 0usize;
        let eptr = sd1_disk_list(img, subdir, &mut count);
        if eptr.is_null() || count == 0 { continue; }
        let entries = unsafe { std::slice::from_raw_parts(eptr, count) };
        for e in entries {
            println!("{:<12} {:<22} {:>6} {:>6} {:>4}",
                entry_name(e), file_type_name(e.file_type),
                e.size_blocks, e.size_bytes, e.file_number);
            total += 1;
        }
        sd1_entries_free(eptr, count);
    }

    let free = sd1_disk_free_blocks(img);
    println!("\n{} file(s), {} free blocks", total, free);
    sd1_disk_free(img);
    Ok(())
}

fn cmd_extract(image_path: &Path, name: &str, out_path: Option<&Path>, channel: u8) -> Result<(), String> {
    let path   = path_cstr(image_path);
    let name_c = cstr(name);

    let mut open_err = 0i32;
    let img = sd1_disk_open(path.as_ptr(), &mut open_err);
    if img.is_null() {
        return Err(format!("open failed: {}", err_msg(open_err)));
    }

    let mut entry = std::mem::MaybeUninit::<Sd1DirectoryEntry>::uninit();
    let find_rc = sd1_disk_find(img, name_c.as_ptr(), entry.as_mut_ptr());
    if find_rc != 0 {
        sd1_disk_free(img);
        return Err(format!("{}: {}", name, err_msg(find_rc)));
    }
    let entry = unsafe { entry.assume_init() };

    let mut raw_ptr: *mut u8 = ptr::null_mut();
    let mut raw_len: usize = 0;
    let rc = sd1_disk_extract(img, name_c.as_ptr(), &mut raw_ptr, &mut raw_len);
    sd1_disk_free(img);
    if rc != 0 {
        return Err(format!("extract failed: {}", err_msg(rc)));
    }

    let sysex_bytes: Vec<u8> = match entry.file_type {
        SD1_FILE_ONE_PROGRAM => {
            let prog = sd1_program_from_bytes(raw_ptr, raw_len, ptr::null_mut());
            sd1_bytes_free(raw_ptr, raw_len);
            if prog.is_null() { return Err("program parse failed".into()); }
            let pkt = sd1_program_to_sysex(prog, channel);
            sd1_program_free(prog);
            let mut slen = 0usize;
            let sptr = sd1_sysex_to_bytes(pkt, channel, &mut slen);
            sd1_sysex_free(pkt);
            if sptr.is_null() { return Err("sysex serialise failed".into()); }
            unsafe { take_bytes(sptr, slen) }
        }
        SD1_FILE_ONE_PRESET => {
            let preset = sd1_preset_from_bytes(raw_ptr, raw_len, ptr::null_mut());
            sd1_bytes_free(raw_ptr, raw_len);
            if preset.is_null() { return Err("preset parse failed".into()); }
            let pkt = sd1_preset_to_sysex(preset, channel);
            sd1_preset_free(preset);
            let mut slen = 0usize;
            let sptr = sd1_sysex_to_bytes(pkt, channel, &mut slen);
            sd1_sysex_free(pkt);
            if sptr.is_null() { return Err("sysex serialise failed".into()); }
            unsafe { take_bytes(sptr, slen) }
        }
        SD1_FILE_TWENTY_PRESETS => {
            let payload = unsafe { std::slice::from_raw_parts(raw_ptr, raw_len) };
            let out = wrap_sysex(channel, MSG_ALL_PRESETS, payload);
            sd1_bytes_free(raw_ptr, raw_len);
            out
        }
        SD1_FILE_SIXTY_PROGRAMS => {
            let mut dp: *mut u8 = ptr::null_mut();
            let mut dl: usize = 0;
            let rc = sd1_deinterleave_sixty_programs(raw_ptr, raw_len, &mut dp, &mut dl);
            sd1_bytes_free(raw_ptr, raw_len);
            if rc != 0 { return Err(format!("deinterleave failed: {}", err_msg(rc))); }
            let payload = unsafe { std::slice::from_raw_parts(dp, dl) };
            let out = wrap_sysex(channel, MSG_ALL_PROGRAMS, payload);
            sd1_bytes_free(dp, dl);
            out
        }
        SD1_FILE_ONE_SEQUENCE => {
            let seq = sd1_sequence_from_bytes(raw_ptr, raw_len);
            sd1_bytes_free(raw_ptr, raw_len);
            if seq.is_null() { return Err("sequence parse failed".into()); }
            let pkt = sd1_sequence_to_sysex(seq, channel);
            sd1_sequence_free(seq);
            let mut slen = 0usize;
            let sptr = sd1_sysex_to_bytes(pkt, channel, &mut slen);
            sd1_sysex_free(pkt);
            if sptr.is_null() { return Err("sysex serialise failed".into()); }
            unsafe { take_bytes(sptr, slen) }
        }
        SD1_FILE_THIRTY_SEQUENCES => {
            let mut pp: *mut u8 = ptr::null_mut();
            let mut pl: usize = 0;
            let rc = sd1_disk_to_thirty_sequences(raw_ptr, raw_len, &mut pp, &mut pl);
            sd1_bytes_free(raw_ptr, raw_len);
            if rc != 0 { return Err(format!("thirty_seq failed: {}", err_msg(rc))); }
            let payload = unsafe { std::slice::from_raw_parts(pp, pl) };
            let out = wrap_sysex(channel, MSG_ALL_SEQUENCES, payload);
            sd1_bytes_free(pp, pl);
            out
        }
        SD1_FILE_SIXTY_SEQUENCES => {
            let has_progs = entry.type_info & 0x20 != 0;
            let mut pp: *mut u8 = ptr::null_mut();
            let mut pl: usize = 0;
            let mut rc = sd1_disk_to_allsequences(raw_ptr, raw_len, has_progs, &mut pp, &mut pl);
            if rc != 0 && has_progs {
                rc = sd1_disk_to_allsequences(raw_ptr, raw_len, false, &mut pp, &mut pl);
            }
            sd1_bytes_free(raw_ptr, raw_len);
            if rc != 0 { return Err(format!("allseq failed: {}", err_msg(rc))); }
            let payload = unsafe { std::slice::from_raw_parts(pp, pl) };
            let out = wrap_sysex(channel, MSG_ALL_SEQUENCES, payload);
            sd1_bytes_free(pp, pl);
            out
        }
        other => {
            sd1_bytes_free(raw_ptr, raw_len);
            return Err(format!("unsupported file type 0x{:02X}", other));
        }
    };

    let dest = out_path.map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("{}.syx", name)));
    std::fs::write(&dest, &sysex_bytes).map_err(|e| e.to_string())?;
    println!("Extracted: {} -> {}", name, dest.display());
    Ok(())
}

fn cmd_write(image_path: &Path, sysex_path: &Path, name_override: Option<&str>, overwrite: bool) -> Result<(), String> {
    let raw_syx = std::fs::read(sysex_path).map_err(|e| e.to_string())?;

    let mut pkt_count = 0usize;
    let mut parse_err = 0i32;
    let pkts = sd1_sysex_parse_all(raw_syx.as_ptr(), raw_syx.len(), &mut pkt_count, &mut parse_err);
    if pkts.is_null() || pkt_count == 0 {
        return Err(format!("SysEx parse failed: {}", err_msg(parse_err)));
    }
    let pkt_slice = unsafe { std::slice::from_raw_parts(pkts, pkt_count) };

    let base_name = if let Some(n) = name_override {
        n.to_uppercase()
    } else {
        sysex_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("UNNAMED")
            .to_uppercase()
    };

    let writable: Vec<*mut SysExPacket> = pkt_slice.iter().copied()
        .filter(|&p| { let t = sd1_sysex_message_type(p); t != 0x00 && t != 0x01 })
        .collect();

    if writable.is_empty() {
        sd1_sysex_packets_free(pkts, pkt_count);
        return Err("no writable packets in SysEx file".into());
    }

    let has_all_progs = writable.iter().any(|&p| sd1_sysex_message_type(p) == MSG_ALL_PROGRAMS);
    let has_all_seqs  = writable.iter().any(|&p| sd1_sysex_message_type(p) == MSG_ALL_SEQUENCES);
    let embed_programs = has_all_progs && has_all_seqs;

    let interleaved_progs: Option<Vec<u8>> = if embed_programs {
        let prog_pkt = writable.iter().copied()
            .find(|&p| sd1_sysex_message_type(p) == MSG_ALL_PROGRAMS).unwrap();
        let pl = sd1_sysex_payload(prog_pkt);
        let pl_len = sd1_sysex_payload_len(prog_pkt);
        let mut ip: *mut u8 = ptr::null_mut();
        let mut il: usize = 0;
        let rc = sd1_interleave_sixty_programs(pl, pl_len, &mut ip, &mut il);
        if rc != 0 {
            sd1_sysex_packets_free(pkts, pkt_count);
            return Err(format!("interleave failed: {}", err_msg(rc)));
        }
        let v = unsafe { take_bytes(ip, il) };
        Some(v)
    } else {
        None
    };

    let effective_count = if embed_programs { writable.len() - 1 } else { writable.len() };
    let multi = effective_count > 1;

    let path = path_cstr(image_path);
    let mut open_err = 0i32;
    let img = sd1_disk_open(path.as_ptr(), &mut open_err);
    if img.is_null() {
        sd1_sysex_packets_free(pkts, pkt_count);
        return Err(format!("open failed: {}", err_msg(open_err)));
    }

    for &pkt in &writable {
        let msg_type = sd1_sysex_message_type(pkt);
        if embed_programs && msg_type == MSG_ALL_PROGRAMS { continue; }

        let pl_ptr = sd1_sysex_payload(pkt);
        let pl_len = sd1_sysex_payload_len(pkt);

        let (disk_bytes, file_type_byte): (Vec<u8>, u8) = match msg_type {
            MSG_ONE_PROGRAM => {
                let prog = sd1_program_from_sysex(pkt, ptr::null_mut());
                if prog.is_null() {
                    sd1_disk_free(img); sd1_sysex_packets_free(pkts, pkt_count);
                    return Err("program parse failed".into());
                }
                let ft = sd1_program_file_type(prog);
                let mut bl = 0usize;
                let bp = sd1_program_bytes(prog, &mut bl);
                sd1_program_free(prog);
                let v = unsafe { take_bytes(bp, bl) };
                (v, ft)
            }
            MSG_ALL_PROGRAMS => {
                let mut ip: *mut u8 = ptr::null_mut();
                let mut il: usize = 0;
                let rc = sd1_interleave_sixty_programs(pl_ptr, pl_len, &mut ip, &mut il);
                if rc != 0 {
                    sd1_disk_free(img); sd1_sysex_packets_free(pkts, pkt_count);
                    return Err(format!("interleave failed: {}", err_msg(rc)));
                }
                let v = unsafe { take_bytes(ip, il) };
                (v, SD1_FILE_SIXTY_PROGRAMS)
            }
            MSG_ONE_PRESET => {
                let preset = sd1_preset_from_sysex(pkt, ptr::null_mut());
                if preset.is_null() {
                    sd1_disk_free(img); sd1_sysex_packets_free(pkts, pkt_count);
                    return Err("preset parse failed".into());
                }
                let ft = sd1_preset_file_type(preset);
                let mut bl = 0usize;
                let bp = sd1_preset_bytes(preset, &mut bl);
                sd1_preset_free(preset);
                let v = unsafe { take_bytes(bp, bl) };
                (v, ft)
            }
            MSG_ALL_PRESETS => {
                let v = unsafe { std::slice::from_raw_parts(pl_ptr, pl_len).to_vec() };
                (v, SD1_FILE_TWENTY_PRESETS)
            }
            MSG_SINGLE_SEQUENCE => {
                let seq = sd1_sequence_from_sysex(pkt, ptr::null_mut());
                if seq.is_null() {
                    sd1_disk_free(img); sd1_sysex_packets_free(pkts, pkt_count);
                    return Err("sequence parse failed".into());
                }
                let ft = sd1_sequence_file_type(seq);
                let mut bl = 0usize;
                let bp = sd1_sequence_bytes(seq, &mut bl);
                sd1_sequence_free(seq);
                let v = unsafe { take_bytes(bp, bl) };
                (v, ft)
            }
            MSG_ALL_SEQUENCES => {
                let ip_ptr = interleaved_progs.as_deref().map(|v| v.as_ptr()).unwrap_or(ptr::null());
                let ip_len = interleaved_progs.as_ref().map(|v| v.len()).unwrap_or(0);
                let mut op: *mut u8 = ptr::null_mut();
                let mut ol: usize = 0;
                let rc = sd1_allsequences_to_disk(pl_ptr, pl_len, ip_ptr, ip_len, &mut op, &mut ol);
                if rc != 0 {
                    sd1_disk_free(img); sd1_sysex_packets_free(pkts, pkt_count);
                    return Err(format!("allseq_to_disk failed: {}", err_msg(rc)));
                }
                let v = unsafe { take_bytes(op, ol) };
                (v, SD1_FILE_SIXTY_SEQUENCES)
            }
            other => {
                sd1_disk_free(img); sd1_sysex_packets_free(pkts, pkt_count);
                return Err(format!("unsupported message type 0x{:02X}", other));
            }
        };

        let file_name = if multi {
            let prefix = &base_name[..base_name.len().min(8)];
            match msg_type {
                MSG_ALL_PRESETS | MSG_ONE_PRESET          => format!("{}PST", prefix),
                MSG_ALL_SEQUENCES | MSG_SINGLE_SEQUENCE   => format!("{}SEQ", prefix),
                _ => base_name[..base_name.len().min(11)].to_string(),
            }
        } else {
            base_name[..base_name.len().min(11)].to_string()
        };

        let name_c = cstr(&file_name);
        let programs_embedded = embed_programs && msg_type == MSG_ALL_SEQUENCES;
        let rc = sd1_disk_write(
            img, name_c.as_ptr(), file_type_byte, programs_embedded,
            disk_bytes.as_ptr(), disk_bytes.len(), overwrite,
        );
        if rc != 0 {
            sd1_disk_free(img); sd1_sysex_packets_free(pkts, pkt_count);
            return Err(format!("write '{}' failed: {}", file_name, err_msg(rc)));
        }
        println!("Written: {} ({} bytes, {} block(s))",
            file_name, disk_bytes.len(), disk_bytes.len().div_ceil(512));
    }

    let rc = sd1_disk_save(img, path.as_ptr());
    sd1_disk_free(img);
    sd1_sysex_packets_free(pkts, pkt_count);
    if rc != 0 {
        return Err(format!("save failed: {}", err_msg(rc)));
    }
    Ok(())
}

fn cmd_delete(image_path: &Path, name: &str) -> Result<(), String> {
    let path   = path_cstr(image_path);
    let name_c = cstr(name);
    let mut open_err = 0i32;
    let img = sd1_disk_open(path.as_ptr(), &mut open_err);
    if img.is_null() {
        return Err(format!("open failed: {}", err_msg(open_err)));
    }
    let rc = sd1_disk_delete(img, name_c.as_ptr());
    if rc != 0 {
        sd1_disk_free(img);
        return Err(format!("delete failed: {}", err_msg(rc)));
    }
    let rc = sd1_disk_save(img, path.as_ptr());
    sd1_disk_free(img);
    if rc != 0 {
        return Err(format!("save failed: {}", err_msg(rc)));
    }
    println!("Deleted: {}", name);
    Ok(())
}

fn cmd_create(image_path: &Path) -> Result<(), String> {
    let path = path_cstr(image_path);
    let img = sd1_disk_create();
    if img.is_null() {
        return Err("create failed".into());
    }
    let rc = sd1_disk_save(img, path.as_ptr());
    sd1_disk_free(img);
    if rc != 0 {
        return Err(format!("save failed: {}", err_msg(rc)));
    }
    println!("Created: {}", image_path.display());
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::List   { image }                           => cmd_list(&image),
        Command::Extract { image, name, output, channel }  => cmd_extract(&image, &name, output.as_deref(), channel),
        Command::Write  { image, sysex, name, overwrite }  => cmd_write(&image, &sysex, name.as_deref(), overwrite),
        Command::Delete { image, name }                     => cmd_delete(&image, &name),
        Command::Create { image }                           => cmd_create(&image),
    };
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
