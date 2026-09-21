# SD-1 Disk Recovery Utilities

## tools/scp_to_img.py
- Converts a Greaseweazle SCP flux image to a raw 819,200-byte Ensoniq SD-1 disk image
- Parses 3 revolutions per track for maximum sector recovery
- Usage: `python3 tools/scp_to_img.py <input.scp> <output.img> [--report]`
- `--report`: print coverage summary without writing
- Recovered 1178/1600 sectors (73.6%) from sd1_muzak3.scp

## tools/merge_scp_hfe.py
- Merges sectors from both a SCP and an HFE (same Greaseweazle session) into one image
- HFE sometimes decodes sectors the SCP flux decoder misses, and vice versa
- Usage: `python3 tools/merge_scp_hfe.py <input.scp> <input.hfe> <output.img> [--report]`
- Recovered 1287/1600 sectors (80.4%) from the MUZAK3 pair

## tools/triage_sequences.py
- Analyses a (possibly partial) disk image for salvageable sequence data
- Works on both SixtySequences and OneSequence file types
- For SixtySequences: walks the 60 slot headers in order, tracking the event pool cursor, and reports which slots are fully intact
- Usage: `python3 tools/triage_sequences.py <image.img>`

## tools/extract_intact_sequences.py
- Extracts only fully-intact sequences from a damaged SixtySequences file
- Writes each intact slot as a standalone OneSequence SysEx (.syx) loadable by SD-1 hardware
- Usage: `python3 tools/extract_intact_sequences.py <image.img> <file_name> [--channel N] [--outdir DIR]`

## Typical workflow for a damaged disk
```
# 1. Best single-source recovery
python3 tools/scp_to_img.py disk.scp recovered.img

# 2. Merge with HFE for extra sectors
python3 tools/merge_scp_hfe.py disk.scp disk.hfe merged.img

# 3. Assess what's salvageable
python3 tools/triage_sequences.py merged.img

# 4. Extract intact sequences
python3 tools/extract_intact_sequences.py merged.img 'FILE NAME' --outdir ./extracted
```

## Format notes
- SCP: raw flux transition timing (25ns ticks, big-endian u16), 3 revolutions per track
- HFE v1: MFM bitstream, LSB-first bit storage, 256-byte interleaved side chunks
- SD-1 image: 819,200 bytes, 1600 blocks × 512 bytes, 80 tracks × 2 sides × 10 sectors
- Block addressing: block = track×20 + side×10 + sector (0-based)
