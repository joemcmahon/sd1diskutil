// crates/sd1cli/src/main.rs
use clap::{Parser, Subcommand};
use sd1disk::{
    DiskImage, SubDirectory, FileAllocationTable, Program, Preset, Sequence,
    validate_name, DirectoryEntry, FileType,
};
use sd1disk::sysex::SysExPacket;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "sd1disk", about = "Ensoniq SD-1 disk image utility")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all files on a disk image
    List {
        image: PathBuf,
    },
    /// Show disk metadata: free blocks, FAT health
    Inspect {
        image: PathBuf,
    },
    /// Write a SysEx file to a disk image
    Write {
        image: PathBuf,
        sysex: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
        dir: Option<u8>,
        #[arg(long)]
        overwrite: bool,
    },
    /// Extract a file from a disk image as SysEx
    Extract {
        image: PathBuf,
        name: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "0")]
        channel: u8,
    },
    /// Delete a file from a disk image
    Delete {
        image: PathBuf,
        name: String,
    },
    /// Create a new blank disk image
    Create {
        image: PathBuf,
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
    println!("\n{} file(s), {} free blocks", total, img.free_blocks());
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
        sd1disk::MessageType::OnePreset => {
            let preset = Preset::from_sysex(&packet)?;
            (preset.to_bytes().to_vec(), FileType::OnePreset)
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
        n.to_string()
    } else {
        sysex_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("UNNAMED")
            .to_string()
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
        let old_chain = FileAllocationTable::chain(&img, existing.first_block as u16)?;
        let freed = old_chain.len() as u32;
        FileAllocationTable::free_chain(&mut img, existing.first_block as u16);
        target_dir.remove(&mut img, &resolved_name)?;
        img.set_free_blocks(img.free_blocks() + freed);
    }

    let n_blocks = ((data.len() + 511) / 512) as u16;
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
        type_info: 0,
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
    img.set_free_blocks(img.free_blocks() - n_blocks as u32);
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
    img.set_free_blocks(img.free_blocks() + freed);
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
