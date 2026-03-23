// crates/sd1cli/src/main.rs
use clap::{Parser, Subcommand};
use sd1disk::{
    DiskImage, SubDirectory, FileAllocationTable, Program, Preset, Sequence,
    validate_name, DirectoryEntry, FileType, MessageType, deinterleave_sixty_programs,
};
use sd1disk::sysex::SysExPacket;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "sd1cli",
    about = "Ensoniq SD-1 disk image utility",
    long_about = "\
Ensoniq SD-1 disk image utility

Manage Ensoniq SD-1 synthesizer disk images (.img). Supports reading, writing,
and extracting Programs, Presets, and Sequences in SysEx format.

SD-1 disks hold up to 156 files across 4 sub-directories. Each 512-byte block
stores raw synthesizer data. SysEx files use nybblized encoding for MIDI transfer.

Run `sd1cli <SUBCOMMAND> --help` for details on each command.",
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all files on a disk image
    #[command(long_about = "\
List all files stored on a disk image.

Prints a table with each file's name, type, size in blocks and bytes, and
directory slot number. Shows total file count and free block count at the end.

Supported file types: OneProgram, OnePreset, OneSequence, ThirtySequences,
SixtySequences, OperatingSystem.")]
    List {
        /// Path to the SD-1 disk image file
        image: PathBuf,
    },
    /// Show disk metadata: free blocks, FAT health
    #[command(long_about = "\
Inspect a disk image without modifying it.

Displays the disk path, OS-block free count, total block layout, and a
File Allocation Table (FAT) summary: free, used, and bad block counts.

Note: the OS-block free count may read 0 on hardware-written images; the FAT
count is always accurate.")]
    Inspect {
        /// Path to the SD-1 disk image file
        image: PathBuf,
    },
    /// Write a SysEx file to a disk image
    #[command(long_about = "\
Write a SysEx (.syx) file into a disk image.

The SysEx file must contain a OneProgram, OnePreset, SingleSequence, or
AllSequences message. The file is de-nybblized, stored in the disk image, and
the FAT is updated.

By default the file name is taken from the SysEx filename (stem only, up to 11
characters). Use --name to override. Use --dir to target a specific sub-directory
(1–4); otherwise the first directory with free slots is used.

Use --overwrite to replace an existing file with the same name.")]
    Write {
        /// Path to the SD-1 disk image file
        image: PathBuf,
        /// Path to the SysEx (.syx) file to write
        sysex: PathBuf,
        /// Override the file name stored on disk (max 11 characters, A–Z 0–9 space)
        #[arg(long, help = "Override stored file name (max 11 chars)")]
        name: Option<String>,
        /// Target sub-directory 1–4 (default: first directory with free space)
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4), help = "Target sub-directory 1–4")]
        dir: Option<u8>,
        /// Replace an existing file with the same name
        #[arg(long, help = "Overwrite if a file with this name already exists")]
        overwrite: bool,
    },
    /// Extract a file from a disk image as SysEx
    #[command(long_about = "\
Extract a named file from a disk image and save it as a SysEx (.syx) file.

The file is read from the disk image, nybblized, and wrapped in a SysEx
message. Output defaults to `<NAME>.syx` in the current directory.

Use --out to specify an alternate output path. Use --channel to set the MIDI
channel embedded in the SysEx header (default 0, i.e. channel 1).")]
    Extract {
        /// Path to the SD-1 disk image file
        image: PathBuf,
        /// Name of the file to extract (case-insensitive, max 11 characters)
        name: String,
        /// Output path for the extracted SysEx file (default: <NAME>.syx)
        #[arg(long, help = "Output file path (default: <NAME>.syx)")]
        out: Option<PathBuf>,
        /// MIDI channel to embed in the SysEx header (0 = channel 1)
        #[arg(long, default_value = "0", help = "MIDI channel (0–15, default 0)")]
        channel: u8,
    },
    /// Delete a file from a disk image
    #[command(long_about = "\
Delete a named file from a disk image.

Frees the file's FAT chain and removes its directory entry. The disk image is
saved after deletion. This operation cannot be undone.")]
    Delete {
        /// Path to the SD-1 disk image file
        image: PathBuf,
        /// Name of the file to delete (case-insensitive, max 11 characters)
        name: String,
    },
    /// Create a new blank disk image
    #[command(long_about = "\
Create a new blank SD-1 disk image.

Writes a 819,200-byte image (1600 × 512-byte blocks) pre-formatted with the
SD-1 OS structures intact. Blocks 0–22 are reserved; blocks 23–1599 are free.
The image can be written to a floppy disk with a tool such as `dd`.")]
    Create {
        /// Path where the new disk image will be written
        image: PathBuf,
    },
    /// Parse and display contents of a SysEx file
    #[command(name = "inspect-sysex", long_about = "\
Parse a SysEx (.syx) file and display its contents.

For AllPrograms: lists all 60 program names and their slot numbers.
For OneProgram: shows the program name and payload size.
For AllPresets: lists all preset names.
For other types: shows message type and payload size.")]
    InspectSysex {
        /// Path to the SysEx (.syx) file
        sysex: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> sd1disk::Result<()> {
    match cli.command {
        Command::List { image } => cmd_list(&image),
        Command::Inspect { image } => cmd_inspect(&image),
        Command::Write { image, sysex, name, dir, overwrite } =>
            cmd_write(&image, &sysex, name.as_deref(), dir, overwrite),
        Command::Extract { image, name, out, channel } =>
            cmd_extract(&image, &name, out.as_deref(), channel),
        Command::Delete { image, name } => cmd_delete(&image, &name),
        Command::Create { image } => cmd_create(&image),
        Command::InspectSysex { sysex } => cmd_inspect_sysex(&sysex),
    }
}

fn cmd_list(image_path: &Path) -> sd1disk::Result<()> {
    let img = DiskImage::open(image_path)?;
    println!("{:<12} {:<22} {:>6} {:>6} {:>4}",
        "NAME", "TYPE", "BLOCKS", "BYTES", "SLOT");
    println!("{}", "-".repeat(56));
    let mut total = 0usize;
    for dir_idx in 0..4u8 {
        let dir = SubDirectory::new(dir_idx);
        for entry in dir.entries(&img) {
            let type_str = format!("{:?}", entry.file_type);
            println!("{:<12} {:<22} {:>6} {:>6} {:>4}",
                entry.name_str(),
                type_str,
                entry.size_blocks,
                entry.size_bytes,
                entry.file_number,
            );
            total += 1;
        }
    }
    let free_count = (23u16..1600)
        .filter(|&b| sd1disk::FileAllocationTable::entry(&img, b) == sd1disk::FatEntry::Free)
        .count();
    println!("\n{} file(s), {} free blocks", total, free_count);
    Ok(())
}

fn cmd_inspect(image_path: &Path) -> sd1disk::Result<()> {
    let img = DiskImage::open(image_path)?;
    println!("Disk image: {}", image_path.display());
    println!("Free blocks: {}", img.free_blocks());
    println!("Total blocks: 1600 (23 reserved, 1577 usable)");

    let mut free = 0u32;
    let mut used = 0u32;
    let mut bad = 0u32;
    for b in 23u16..1600 {
        match FileAllocationTable::entry(&img, b) {
            sd1disk::FatEntry::Free => free += 1,
            sd1disk::FatEntry::BadBlock => bad += 1,
            _ => used += 1,
        }
    }
    println!("FAT: {} free, {} used, {} bad", free, used, bad);
    Ok(())
}

fn cmd_write(
    image_path: &Path,
    sysex_path: &Path,
    name_override: Option<&str>,
    dir_override: Option<u8>,
    overwrite: bool,
) -> sd1disk::Result<()> {
    let sysex_bytes = std::fs::read(sysex_path)?;
    let packet = SysExPacket::parse(&sysex_bytes)?;

    let (data, file_type) = match &packet.message_type {
        sd1disk::MessageType::OneProgram => {
            let prog = Program::from_sysex(&packet)?;
            (prog.to_bytes().to_vec(), FileType::OneProgram)
        }
        sd1disk::MessageType::AllPrograms => {
            (sd1disk::interleave_sixty_programs(&packet.payload)?, FileType::SixtyPrograms)
        }
        sd1disk::MessageType::OnePreset => {
            let preset = Preset::from_sysex(&packet)?;
            (preset.to_bytes().to_vec(), FileType::OnePreset)
        }
        sd1disk::MessageType::AllPresets => {
            (packet.payload.clone(), FileType::TwentyPresets)
        }
        sd1disk::MessageType::SingleSequence |
        sd1disk::MessageType::AllSequences => {
            let seq = Sequence::from_sysex(&packet)?;
            (seq.to_bytes().to_vec(), seq.file_type())
        }
        _other => {
            return Err(sd1disk::Error::InvalidSysEx("unsupported SysEx message type for write"));
        }
    };

    let resolved_name = if let Some(n) = name_override {
        n.to_uppercase()
    } else {
        sysex_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("UNNAMED")
            .to_uppercase()
    };

    let name_arr = validate_name(&resolved_name)?;

    let mut img = DiskImage::open(image_path)?;

    let target_dir_idx: u8 = if let Some(d) = dir_override {
        d - 1
    } else {
        (0..4u8)
            .find(|&i| SubDirectory::new(i).free_slots(&img) > 0)
            .ok_or(sd1disk::Error::DirectoryFull)?
    };
    let target_dir = SubDirectory::new(target_dir_idx);

    if let Some(existing) = target_dir.find(&img, &resolved_name) {
        if !overwrite {
            return Err(sd1disk::Error::FileExists(resolved_name));
        }
        FileAllocationTable::free_chain(&mut img, existing.first_block as u16);
        target_dir.remove(&mut img, &resolved_name)?;
    }

    let n_blocks = data.len().div_ceil(512) as u16;
    let blocks = FileAllocationTable::allocate(&mut img, n_blocks)?;

    for (i, &block_num) in blocks.iter().enumerate() {
        let start = i * 512;
        let end = (start + 512).min(data.len());
        let block = img.block_mut(block_num)?;
        block.fill(0);
        if end > start {
            block[..end - start].copy_from_slice(&data[start..end]);
        }
    }
    FileAllocationTable::set_chain(&mut img, &blocks);

    let entry = DirectoryEntry {
        type_info: 0x0F,
        file_type,
        name: name_arr,
        _reserved: 0,
        size_blocks: n_blocks,
        contiguous_blocks: n_blocks,
        first_block: blocks[0] as u32,
        file_number: 0,
        size_bytes: data.len() as u32,
    };
    target_dir.add(&mut img, entry)?;
    img.save(image_path)?;

    println!("Written: {} ({} bytes, {} block(s))", resolved_name, data.len(), n_blocks);
    Ok(())
}

fn cmd_extract(
    image_path: &Path,
    name: &str,
    out_path: Option<&Path>,
    channel: u8,
) -> sd1disk::Result<()> {
    let img = DiskImage::open(image_path)?;

    let entry = (0..4u8)
        .find_map(|i| SubDirectory::new(i).find(&img, name))
        .ok_or_else(|| sd1disk::Error::FileNotFound(name.to_string()))?;

    let chain = FileAllocationTable::chain(&img, entry.first_block as u16)?;
    let mut raw = Vec::new();
    for &b in &chain {
        raw.extend_from_slice(img.block(b)?);
    }
    raw.truncate(entry.size_bytes as usize);

    let sysex_bytes = match entry.file_type {
        FileType::OneProgram => {
            Program::from_bytes(&raw)?.to_sysex(channel).to_bytes(channel)
        }
        FileType::OnePreset => {
            Preset::from_bytes(&raw)?.to_sysex(channel).to_bytes(channel)
        }
        FileType::SixtyPrograms => {
            let payload = deinterleave_sixty_programs(&raw)?;
            SysExPacket { message_type: MessageType::AllPrograms, midi_channel: channel, model: 0, payload }
                .to_bytes(channel)
        }
        FileType::OneSequence | FileType::ThirtySequences | FileType::SixtySequences => {
            Sequence::from_bytes(&raw).to_sysex(channel).to_bytes(channel)
        }
        _ => return Err(sd1disk::Error::InvalidSysEx("unsupported file type for extract")),
    };

    let out = out_path.map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("{}.syx", name)));
    std::fs::write(&out, &sysex_bytes)?;
    println!("Extracted: {} -> {}", name, out.display());
    Ok(())
}

fn cmd_delete(image_path: &Path, name: &str) -> sd1disk::Result<()> {
    let mut img = DiskImage::open(image_path)?;

    let (dir_idx, entry) = (0..4u8)
        .find_map(|i| SubDirectory::new(i).find(&img, name).map(|e| (i, e)))
        .ok_or_else(|| sd1disk::Error::FileNotFound(name.to_string()))?;

    let chain = FileAllocationTable::chain(&img, entry.first_block as u16)?;
    let freed = chain.len() as u32;
    FileAllocationTable::free_chain(&mut img, entry.first_block as u16);
    SubDirectory::new(dir_idx).remove(&mut img, name)?;
    img.save(image_path)?;

    println!("Deleted: {} ({} block(s) freed)", name, freed);
    Ok(())
}

fn cmd_create(image_path: &Path) -> sd1disk::Result<()> {
    let img = DiskImage::create();
    img.save(image_path)?;
    println!("Created blank disk image: {}", image_path.display());
    Ok(())
}

fn cmd_inspect_sysex(sysex_path: &Path) -> sd1disk::Result<()> {
    let bytes = std::fs::read(sysex_path)?;
    let packets = SysExPacket::parse_all(&bytes)?;

    println!("File:    {}", sysex_path.display());
    if packets.len() > 1 {
        println!("Packets: {}", packets.len());
    }

    for (idx, packet) in packets.iter().enumerate() {
        let payload = &packet.payload;
        if packets.len() > 1 {
            println!();
            println!("--- Packet {}: {} ---", idx + 1, packet.message_type.display_name());
        }
        println!("Type:    {}", packet.message_type.display_name());
        println!("Channel: {}", packet.midi_channel);
        println!("Payload: {} bytes", payload.len());

        match &packet.message_type {
            sd1disk::MessageType::OneProgram => {
                let prog = Program::from_sysex(packet)?;
                println!("Name:    {}", prog.name());
            }
            sd1disk::MessageType::AllPrograms => {
                let prog_size = 530usize;
                let expected = 60 * prog_size;
                if payload.len() != expected {
                    println!("WARNING: expected {} bytes (60 × 530), got {}", expected, payload.len());
                    continue;
                }
                println!();
                println!("{:<4} {}", "SLOT", "NAME");
                println!("{}", "-".repeat(20));
                for i in 0..60 {
                    let name_bytes = &payload[i * prog_size + 498..i * prog_size + 509];
                    let name = name_bytes.iter()
                        .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '?' })
                        .collect::<String>();
                    let blank = name.trim().is_empty();
                    println!("{:<4} [{}]{}", i, name, if blank { "  <empty>" } else { "" });
                }
            }
            sd1disk::MessageType::OnePreset => {
                println!("(preset data, {} bytes)", payload.len());
            }
            sd1disk::MessageType::AllPresets => {
                let preset_size = 48usize;
                let count = payload.len() / preset_size;
                println!("Presets: {} × {} bytes", count, preset_size);
            }
            sd1disk::MessageType::SingleSequence |
            sd1disk::MessageType::AllSequences => {
                println!("(sequence data, {} bytes)", payload.len());
            }
            sd1disk::MessageType::TrackParameters => {
                println!("(track parameters, {} bytes)", payload.len());
            }
            sd1disk::MessageType::Command => {
                print_command_payload(payload);
            }
            sd1disk::MessageType::Error => {
                let code = payload.first().copied().unwrap_or(0xFF);
                let name = match code {
                    0x00 => "NAK",
                    0x01 => "INVALID PARAMETER NUMBER",
                    0x02 => "INVALID PARAMETER VALUE",
                    0x03 => "INVALID BUTTON NUMBER",
                    0x04 => "ACK",
                    _    => "unknown error code",
                };
                println!("Error code 0x{:02X}: {}", code, name);
            }
            sd1disk::MessageType::Unknown(b) => {
                println!("(unknown type 0x{:02X}, {} bytes)", b, payload.len());
            }
        }
    }
    Ok(())
}

fn print_command_payload(payload: &[u8]) {
    let cmd = payload.first().copied().unwrap_or(0xFF);
    let (name, extra) = match cmd {
        0x00 => ("VirtualButtons", None),
        0x01 => ("ParameterChange", None),
        0x02 => ("EditChangeStatus", None),
        0x03 => ("ESPMicrocodeLoad", None),
        0x04 => ("PokeByteToRAM", None),
        0x05 => ("SingleProgramDumpRequest", None),
        0x06 => ("SinglePresetDumpRequest", None),
        0x07 => ("TrackParameterDumpRequest", None),
        0x08 => ("DumpEverythingRequest", None),
        0x09 => ("InternalProgramBankDumpRequest", None),
        0x0A => ("InternalPresetBankDumpRequest", None),
        0x0B | 0x0C => {
            let name = if cmd == 0x0B { "SingleSequenceDump" } else { "AllSequenceMemoryDump" };
            let size = if payload.len() >= 5 {
                ((payload[1] as u32) << 24)
                    | ((payload[2] as u32) << 16)
                    | ((payload[3] as u32) << 8)
                    | (payload[4] as u32)
            } else {
                0
            };
            (name, Some(format!("sequence data size = {} bytes", size)))
        }
        0x0D => ("SingleSequenceDumpRequest", None),
        0x0E => ("AllSequenceDumpRequest", None),
        _    => ("unknown command", None),
    };
    match extra {
        Some(detail) => println!("Command 0x{:02X}: {} ({})", cmd, name, detail),
        None         => println!("Command 0x{:02X}: {}", cmd, name),
    }
}
