# sd1disk C/C++ API Reference

`sd1disk` is a static library for reading and writing Ensoniq SD-1/VFXsd disk images. It is written in Rust and exposes a C-compatible ABI suitable for use from C or C++ (including JUCE projects).

---

## Setup

### Linking (CMake example)

```cmake
target_link_libraries(MyPlugin PRIVATE
    ${CMAKE_SOURCE_DIR}/libs/sd1disk.a   # or sd1disk.lib on Windows
)
target_include_directories(MyPlugin PRIVATE
    ${CMAKE_SOURCE_DIR}/libs
)
```

### Include

```cpp
#include "sd1disk.h"
```

The header is valid C and C++. All declarations are wrapped in `extern "C"` when included from C++.

---

## Error handling

Every fallible function returns `int` (0 = success) or a pointer (`NULL` = failure).

```cpp
int err = 0;
Sd1DiskImage* img = sd1_disk_open("/path/to/disk.img", &err);
if (!img) {
    fprintf(stderr, "failed to open: %s\n", sd1_error_message(err));
}
```

The `err_out` pointer may be `NULL` if you don't need the specific code.

### Error codes

| Code | Meaning |
|---|---|
| `SD1_OK` (0) | Success |
| `SD1_ERR_INVALID_IMAGE` | File is not a valid 819,200-byte SD-1 image |
| `SD1_ERR_INVALID_SYSEX` | Bad SysEx structure |
| `SD1_ERR_WRONG_MSG_TYPE` | SysEx message type mismatch |
| `SD1_ERR_FILE_NOT_FOUND` | No file with that name on the disk |
| `SD1_ERR_FILE_EXISTS` | File already exists; use `overwrite = true` |
| `SD1_ERR_DISK_FULL` | Not enough free blocks |
| `SD1_ERR_DIRECTORY_FULL` | All 156 directory slots are used |
| `SD1_ERR_BLOCK_OOB` | Block number out of range (must be 0–1599) |
| `SD1_ERR_INVALID_TYPE` | Unrecognised file type byte |
| `SD1_ERR_CORRUPT_FAT` | FAT chain contains a cycle or bad reference |
| `SD1_ERR_BAD_BLOCK` | Bad-block marker found in file chain |
| `SD1_ERR_INVALID_NAME` | Name is empty or longer than 11 bytes |
| `SD1_ERR_INVALID_HFE` | HFE file header is bad or unsupported |
| `SD1_ERR_HFE_CRC` | CRC mismatch decoding an HFE sector |
| `SD1_ERR_HFE_MISSING_SEC` | Sector not found in HFE track data |
| `SD1_ERR_IO` | Underlying I/O error |

```c
const char* sd1_error_message(int code);
```
Returns a static human-readable string. Never free this pointer.

---

## Memory management

The library manages its own heap. Always free resources using the matching `_free` function — mixing allocators crashes on Windows.

| Resource | How to free |
|---|---|
| `Sd1DiskImage*` | `sd1_disk_free(img)` |
| `Sd1SysExPacket*` | `sd1_sysex_free(pkt)` |
| `Sd1SysExPacket**` array | `sd1_sysex_packets_free(pkts, count)` |
| `Sd1Program*` | `sd1_program_free(prog)` |
| `Sd1Preset*` | `sd1_preset_free(preset)` |
| `Sd1Sequence*` | `sd1_sequence_free(seq)` |
| `Sd1DirectoryEntry*` array | `sd1_entries_free(entries, count)` |
| `uint8_t*` byte buffer | `sd1_bytes_free(buf, len)` |
| `uint16_t*` block array | `sd1_u16_array_free(blocks, count)` |
| `const char*` from `sd1_error_message` | **Do not free** (static string) |

---

## DiskImage

A disk image is an 819,200-byte representation of a single Ensoniq SD-1 floppy (1600 blocks × 512 bytes). The `Sd1DiskImage` handle is opaque.

### Open / create / save

```c
Sd1DiskImage* sd1_disk_open(const char* path, int* err_out);
```
Load an existing `.img` file. Returns `NULL` on failure.

```c
Sd1DiskImage* sd1_disk_create(void);
```
Create a blank, pre-formatted disk image from the embedded factory template. Always succeeds.

```c
int sd1_disk_save(const Sd1DiskImage* img, const char* path);
```
Write the disk image to `path` atomically (write to temp file, then rename). Returns `SD1_OK` or an error code.

```c
void sd1_disk_free(Sd1DiskImage* img);
```
Release all resources held by the image.

```c
uint32_t sd1_disk_free_blocks(const Sd1DiskImage* img);
```
Return the number of free data blocks (0–1577).

### Example

```cpp
int err = 0;
Sd1DiskImage* img = sd1_disk_open("sequencer-os.img", &err);
if (!img) { /* handle error */ }

printf("free blocks: %u\n", sd1_disk_free_blocks(img));

sd1_disk_save(img, "sequencer-os-modified.img");
sd1_disk_free(img);
```

---

## Directory

Each disk has four subdirectories (indices 0–3), each holding up to 39 entries. Some Sojus-created disks also use a secondary directory layout at block 1 (see Block-1 directory below).

### Data types

```c
typedef enum Sd1FileType {
    SD1_FILE_ONE_PROGRAM      = 0x0A,
    SD1_FILE_SIX_PROGRAMS     = 0x0B,
    SD1_FILE_THIRTY_PROGRAMS  = 0x0C,
    SD1_FILE_SIXTY_PROGRAMS   = 0x0D,
    SD1_FILE_ONE_PRESET       = 0x0E,
    SD1_FILE_TEN_PRESETS      = 0x0F,
    SD1_FILE_TWENTY_PRESETS   = 0x10,
    SD1_FILE_ONE_SEQUENCE     = 0x11,
    SD1_FILE_THIRTY_SEQUENCES = 0x12,
    SD1_FILE_SIXTY_SEQUENCES  = 0x13,
    SD1_FILE_SYSTEM_EXCLUSIVE = 0x14,
    SD1_FILE_SYSTEM_SETUP     = 0x15,
    SD1_FILE_SEQUENCER_OS     = 0x16,
} Sd1FileType;

typedef struct {
    uint8_t  type_info;           // 0x00 normally; 0x20 = SixtySequences+Programs
    uint8_t  file_type;           // Sd1FileType value
    char     name[12];            // null-terminated, max 11 chars
    uint16_t size_blocks;
    uint16_t contiguous_blocks;
    uint32_t first_block;
    uint8_t  file_number;
    uint32_t size_bytes;
} Sd1DirectoryEntry;
```

### Listing and searching

```c
Sd1DirectoryEntry* sd1_disk_list(const Sd1DiskImage* img, uint8_t subdir, size_t* count_out);
```
Return all entries in subdirectory `subdir` (0–3). Caller frees with `sd1_entries_free(entries, count)`.

```c
int sd1_disk_find(const Sd1DiskImage* img, const char* name, Sd1DirectoryEntry* entry_out);
```
Search all four subdirectories for a file named `name` (case-sensitive). Writes the entry into `entry_out` and returns `SD1_OK`, or returns `SD1_ERR_FILE_NOT_FOUND`.

### Extracting files

```c
int sd1_disk_extract(const Sd1DiskImage* img, const char* name,
                     uint8_t** data_out, size_t* len_out);
```
Extract the raw file data for `name`. Allocates a buffer; caller frees with `sd1_bytes_free(*data_out, *len_out)`.

> **Note for sequence files:** For `SD1_FILE_THIRTY_SEQUENCES` and `SD1_FILE_SIXTY_SEQUENCES`, the returned buffer length is the full block-aligned size of the FAT chain, which may be larger than `Sd1DirectoryEntry.size_bytes`. Hardware-written files store `size_bytes` as the unpadded logical size, but sequence event data is block-padded on disk and the decoder functions (`sd1_disk_to_thirty_sequences`, `sd1_disk_to_allsequences`) require the full block-aligned data. Pass the entire returned buffer directly to those functions.

### Writing files

```c
int sd1_disk_write(Sd1DiskImage* img, const char* name,
                   uint8_t file_type, bool programs_embedded,
                   const uint8_t* data, size_t len, bool overwrite);
```
Write `data` to the disk under `name`. If a file with that name already exists and `overwrite` is false, returns `SD1_ERR_FILE_EXISTS`.

### Deleting files

```c
int sd1_disk_delete(Sd1DiskImage* img, const char* name);
```

### Block-1 directory (Sojus VST3 layout)

Disks created by the Sojus VST3 plugin store their directory at a non-standard location in block 1. Use these functions to read that layout:

```c
Sd1DirectoryEntry* sd1_block1_entries(const Sd1DiskImage* img, size_t* count_out);
int                sd1_block1_find(const Sd1DiskImage* img, const char* name,
                                   Sd1DirectoryEntry* entry_out);
```

On hardware-formatted disks these return empty results and `SD1_ERR_FILE_NOT_FOUND` respectively.

### Utilities

```c
uint8_t sd1_next_file_number(const Sd1DiskImage* img, uint8_t file_type);
```
Return the next `file_number` to assign for entries of `file_type`. Counts all existing entries of that type across all four subdirectories.

```c
uint8_t sd1_file_type_info(uint8_t file_type, bool programs_embedded);
```
Return the `type_info` byte for a directory entry. Only `SD1_FILE_SIXTY_SEQUENCES` with `programs_embedded = true` returns `0x20`; everything else returns `0x00`.

```c
int sd1_validate_name(const char* name, uint8_t out[11]);
```
Validate that `name` is 1–11 bytes and write it space-padded into `out`. Returns `SD1_OK` or `SD1_ERR_INVALID_NAME`.

### Example

```cpp
size_t count = 0;
Sd1DirectoryEntry* entries = sd1_disk_list(img, 0, &count);
for (size_t i = 0; i < count; i++) {
    printf("%-11s  %u bytes\n", entries[i].name, entries[i].size_bytes);
}
sd1_entries_free(entries, count);

// Extract a file
uint8_t* data = nullptr;
size_t len = 0;
if (sd1_disk_extract(img, "MY-PATCH", &data, &len) == SD1_OK) {
    // use data[0..len]
    sd1_bytes_free(data, len);
}
```

---

## HFE

HFE v1 is the flux image format used by the HxC floppy emulator.

```c
Sd1DiskImage* sd1_read_hfe(const char* path, int* err_out);
int           sd1_write_hfe(const Sd1DiskImage* img, const char* path);
```

`sd1_read_hfe` decodes the HFE flux data and returns a standard `Sd1DiskImage*` handle — use all the same functions on it. Free with `sd1_disk_free`.

---

## FAT (file allocation table)

Low-level access to the on-disk FAT. Most callers don't need these; they're provided for diagnostic tools and advanced use.

```c
typedef struct {
    uint8_t  entry_type;   // 0=Free, 1=EOF, 2=BadBlock, 3=Next
    uint16_t next_block;   // valid only when entry_type == 3
} Sd1FatEntry;

void     sd1_fat_entry(const Sd1DiskImage*, uint16_t block, Sd1FatEntry* out);
int      sd1_fat_chain(const Sd1DiskImage*, uint16_t start,
                        uint16_t** blocks_out, size_t* count_out);
int      sd1_fat_allocate(Sd1DiskImage*, uint16_t n,
                           uint16_t** blocks_out, size_t* count_out);
void     sd1_fat_free_chain(Sd1DiskImage*, uint16_t start);
void     sd1_fat_set_chain(Sd1DiskImage*, const uint16_t* blocks, size_t count);
uint32_t sd1_fat_count_free(const Sd1DiskImage*);
```

`sd1_fat_chain` and `sd1_fat_allocate` allocate a `uint16_t` array; free with `sd1_u16_array_free(blocks, count)`.

`sd1_fat_allocate` prefers a contiguous run of blocks; falls back to scattered allocation if no run is available.

---

## SysEx

Ensoniq SysEx packets carry programs, presets, and sequences over MIDI. The SD-1 uses a nybblized encoding (each byte split into two 4-bit nybbles) inside a standard `F0 … F7` wrapper.

### Message types

| Value | Meaning |
|---|---|
| 0x00 | Command |
| 0x01 | Error |
| 0x02 | OneProgram |
| 0x03 | AllPrograms |
| 0x04 | OnePreset |
| 0x05 | AllPresets |
| 0x09 | SingleSequence |
| 0x0A | AllSequences |
| 0x0B | TrackParameters |
| other | Unknown |

### Parsing

```c
Sd1SysExPacket* sd1_sysex_parse(const uint8_t* data, size_t len, int* err_out);
```
Parse a single Ensoniq SysEx packet. Returns `NULL` on error.

```c
Sd1SysExPacket** sd1_sysex_parse_all(const uint8_t* data, size_t len,
                                      size_t* count_out, int* err_out);
```
Parse a byte stream containing one or more concatenated SysEx packets (e.g. a `.syx` file). Returns an array of `count` handles. Free each packet and then the array:

```cpp
size_t count = 0;
int err = 0;
Sd1SysExPacket** pkts = sd1_sysex_parse_all(data, len, &count, &err);
// use pkts[0..count]
sd1_sysex_packets_free(pkts, count);
```

### Accessors

```c
uint8_t        sd1_sysex_message_type(const Sd1SysExPacket*);
uint8_t        sd1_sysex_midi_channel(const Sd1SysExPacket*);
uint8_t        sd1_sysex_model(const Sd1SysExPacket*);
const uint8_t* sd1_sysex_payload(const Sd1SysExPacket*);
size_t         sd1_sysex_payload_len(const Sd1SysExPacket*);
```

`sd1_sysex_payload` returns a pointer into the packet's internal buffer — valid until the packet is freed, no separate free needed.

### Serialization

```c
uint8_t* sd1_sysex_to_bytes(const Sd1SysExPacket*, uint8_t channel, size_t* len_out);
```
Serialize the packet to wire bytes (with nybblization and `F0`/`F7` framing). Caller frees with `sd1_bytes_free`.

### Freeing

```c
void sd1_sysex_free(Sd1SysExPacket*);
void sd1_sysex_packets_free(Sd1SysExPacket**, size_t count);
```

---

## Typed wrappers: Program, Preset, Sequence

These wrappers validate payload size and file type, and provide typed access to the structured data inside a SysEx packet or raw byte buffer.

### Program (530 bytes, message type OneProgram)

```c
Sd1Program*     sd1_program_from_sysex(const Sd1SysExPacket*, int* err_out);
Sd1Program*     sd1_program_from_bytes(const uint8_t*, size_t, int* err_out);
const uint8_t*  sd1_program_bytes(const Sd1Program*, size_t* len_out);
void            sd1_program_name(const Sd1Program*, char* out, size_t out_len);
Sd1SysExPacket* sd1_program_to_sysex(const Sd1Program*, uint8_t channel);
uint8_t         sd1_program_file_type(const Sd1Program*);
void            sd1_program_free(Sd1Program*);
```

`sd1_program_name` writes the null-terminated program name into `out` (provide at least 12 bytes). The SD-1 strips MSB mute flags automatically.

`sd1_program_bytes` returns a pointer into the program's internal buffer — valid until the program is freed.

`sd1_program_to_sysex` allocates a new `Sd1SysExPacket*`; free with `sd1_sysex_free`.

### Preset (48 bytes, message type OnePreset)

```c
Sd1Preset*      sd1_preset_from_sysex(const Sd1SysExPacket*, int* err_out);
Sd1Preset*      sd1_preset_from_bytes(const uint8_t*, size_t, int* err_out);
const uint8_t*  sd1_preset_bytes(const Sd1Preset*, size_t* len_out);
Sd1SysExPacket* sd1_preset_to_sysex(const Sd1Preset*, uint8_t channel);
uint8_t         sd1_preset_file_type(const Sd1Preset*);
void            sd1_preset_free(Sd1Preset*);
```

### Sequence (variable length)

```c
Sd1Sequence*    sd1_sequence_from_sysex(const Sd1SysExPacket*, int* err_out);
Sd1Sequence*    sd1_sequence_from_bytes(const uint8_t*, size_t);
const uint8_t*  sd1_sequence_bytes(const Sd1Sequence*, size_t* len_out);
Sd1SysExPacket* sd1_sequence_to_sysex(const Sd1Sequence*, uint8_t channel);
uint8_t         sd1_sequence_file_type(const Sd1Sequence*);
void            sd1_sequence_free(Sd1Sequence*);
```

`sd1_sequence_from_bytes` always succeeds (no fixed size constraint); it has no `err_out` parameter.

---

## Type conversions (disk ↔ SysEx format)

These functions convert between the SD-1 on-disk binary layout and the SysEx payload format. They allocate output buffers; free with `sd1_bytes_free`.

```c
int sd1_allsequences_to_disk(const uint8_t* payload, size_t payload_len,
                              const uint8_t* interleaved_progs, size_t progs_len,
                              uint8_t** out, size_t* out_len);
```
Convert an AllSequences SysEx payload to the on-disk SixtySequences format. Pass `interleaved_progs = NULL, progs_len = 0` for the no-programs layout; pass 31,800 bytes of interleaved program data (see `sd1_interleave_sixty_programs`) for the SixtySequences+Programs layout.

```c
int sd1_disk_to_allsequences(const uint8_t* disk, size_t len, bool has_programs,
                              uint8_t** out, size_t* out_len);
```
Reverse of the above. Set `has_programs = true` if the on-disk data includes embedded programs.

```c
int sd1_disk_to_thirty_sequences(const uint8_t* disk, size_t len,
                                  uint8_t** out, size_t* out_len);
```
Convert on-disk ThirtySequences (file type `0x12`) data to a 60-slot AllSequences SysEx payload. Slots 0–29 are populated from the disk; slots 30–59 are set to undefined (`0xFF` headers). Any programs embedded after the sequence data are not included in the output. Unlike SixtySequences, ThirtySequences always stores sequence data at a fixed offset (6144 bytes), so no `has_programs` flag is needed.

If the on-disk global section has an all-zeros `size_sum` (a known quirk of some real hardware files), the correct value is synthesized from the sequence header data so the resulting payload is always valid.

```c
int sd1_thirty_sequences_to_disk(const uint8_t* payload, size_t payload_len,
                                  const uint8_t* interleaved_progs, size_t progs_len,
                                  uint8_t** out, size_t* out_len);
```
Convert an AllSequences SysEx payload to on-disk ThirtySequences format. Only slots 0–29 are written; slots 30–59 are ignored. Pass `interleaved_progs = NULL, progs_len = 0` for the no-programs layout; pass exactly 31,800 bytes (`60 × 530`) of interleaved program data to embed programs after the sequence data.

> **Layout note:** ThirtySequences differs from SixtySequences in two ways: sequence data always starts at offset 6144 (independent of whether programs are present), and when programs are embedded they appear *after* the sequence data rather than before it.

```c
int sd1_interleave_sixty_programs(const uint8_t* payload, size_t len,
                                   uint8_t** out, size_t* out_len);
int sd1_deinterleave_sixty_programs(const uint8_t* data, size_t len,
                                     uint8_t** out, size_t* out_len);
```
Convert between the AllPrograms SysEx payload (60 programs in order, 60 × 530 = 31,800 bytes) and the on-disk SixtyPrograms interleaved format. The interleaved format byte-interleaves programs 0–29 (even bytes) with programs 30–59 (odd bytes) so the hardware can address each program by index.

---

## Program utilities

```c
void sd1_program_name_from_slot(const uint8_t* slot, size_t slot_len,
                                 char* out, size_t out_len);
```
Extract the null-terminated program name from a raw 530-byte program slot. Masks the MSB mute flags on each name byte. Provide at least 12 bytes for `out`.

```c
void sd1_decode_b10(uint8_t b10, const char** disk_programs, size_t count,
                    char* out, size_t out_len);
```
Decode a track program assignment byte (`b10`) to a human-readable label. Provide at least 32 bytes for `out`. `disk_programs` is an optional array of `count` RAM program name strings; pass `NULL, 0` to fall back to the factory INT0 bank.

| b10 value | Meaning |
|---|---|
| 0x00–0x3B | RAM slot; label is `RAM[n]=<name>` |
| 0x7F | No program change on sequence recall |
| 0x80–0xFE | ROM program; label is `ROM0[enc=n]=<name>` or `ROM1[enc=n]=<name>` |
| 0xFF | Track inactive |

### Built-in program name tables

```c
extern const char* const SD1_INT0_PROGRAMS[60];     // factory INT0 user bank
extern const char* const SD1_ROM_ALL_PROGRAMS[120];  // ROM0 (0–59) + ROM1 (60–119)
```

---

## Thread safety

Opaque handles (`Sd1DiskImage*`, etc.) are **not thread-safe**. Do not share a handle across threads without external locking. Disk image I/O is not intended for the audio thread.
