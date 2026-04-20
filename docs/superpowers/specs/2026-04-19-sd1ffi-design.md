# sd1ffi: C-compatible static library design

**Date:** 2026-04-19  
**Status:** approved  
**Purpose:** Expose the full `sd1disk` Rust library as a C-compatible static library with a C++ header, for integration into the Sojus SD-1 VST3 plugin.

---

## Architecture

```
crates/
  sd1disk/        ← unchanged, pure Rust library
  sd1ffi/         ← new; wraps sd1disk for C consumers
    Cargo.toml      (crate-type = ["staticlib"])
    build.rs        (runs cbindgen to emit sd1disk.h)
    cbindgen.toml   (language = "C", cpp_compat = true)
    src/
      lib.rs        (all extern "C" functions)
      error.rs      (sd1disk::Error → C error code mapping)
  sd1cli/         ← unchanged
```

### Build output

`cbindgen` reads `sd1ffi/src/lib.rs` at build time and generates `sd1disk.h` with `#ifdef __cplusplus extern "C"` guards — usable as both a C and C++ header.

### CI changes

- Switch Windows target from `x86_64-pc-windows-gnu` → `x86_64-pc-windows-msvc` (required for JUCE/MSVC compatibility; GNU `.a` and MSVC `.lib` are not interchangeable even with a C ABI).
- Each platform's release archive gains the static library (`.a` / `.lib`) and `sd1disk.h`.

### Delivery format (pre-built, no Rust toolchain required)

Sojus receives per-platform archives from GitHub Releases:
- `sd1ffi-aarch64-apple-darwin.tar.gz` → `libsd1disk.a` + `sd1disk.h`
- `sd1ffi-x86_64-apple-darwin.tar.gz`
- `sd1ffi-x86_64-unknown-linux-musl.tar.gz`
- `sd1ffi-x86_64-pc-windows-msvc.zip` → `sd1disk.lib` + `sd1disk.h`

---

## Memory model

| Category | Ownership rule |
|---|---|
| `Sd1DiskImage*`, `Sd1SysExPacket*`, `Sd1Program*`, `Sd1Preset*`, `Sd1Sequence*` | Rust allocates; caller must call the matching `_free` function |
| `Sd1DirectoryEntry*` arrays from `sd1_disk_list` / `sd1_block1_entries` | Rust allocates; caller calls `sd1_entries_free(ptr, count)` |
| `uint8_t*` byte buffers from `_extract`, `_to_bytes`, conversion functions | Rust allocates; caller calls `sd1_bytes_free(ptr, len)` |
| `uint16_t*` block arrays from `sd1_fat_chain` / `sd1_fat_allocate` | Rust allocates; caller calls `sd1_u16_array_free(ptr, count)` |
| `Sd1SysExPacket**` arrays from `sd1_sysex_parse_all` | Rust allocates; caller calls `sd1_sysex_packets_free(ptr, count)` |
| `const char*` from `sd1_error_message` | Static string — never free |
| Caller-provided `char` output buffers (name functions, `sd1_validate_name`) | Caller owns; Rust writes into them |

**Rationale:** Mismatched allocators crash on Windows. Rust allocated it, Rust must free it.

**Thread safety:** `Sd1DiskImage` and other opaque handles are not thread-safe. Callers must not share a handle across threads without external locking. Disk I/O is not expected on the audio thread.

---

## API surface

### Error codes

```c
typedef enum Sd1Error {
    SD1_OK                   =  0,
    SD1_ERR_INVALID_IMAGE    = -1,
    SD1_ERR_INVALID_SYSEX    = -2,
    SD1_ERR_WRONG_MSG_TYPE   = -3,
    SD1_ERR_FILE_NOT_FOUND   = -4,
    SD1_ERR_FILE_EXISTS      = -5,
    SD1_ERR_DISK_FULL        = -6,
    SD1_ERR_DIRECTORY_FULL   = -7,
    SD1_ERR_BLOCK_OOB        = -8,
    SD1_ERR_INVALID_TYPE     = -9,
    SD1_ERR_CORRUPT_FAT      = -10,
    SD1_ERR_BAD_BLOCK        = -11,
    SD1_ERR_INVALID_NAME     = -12,
    SD1_ERR_INVALID_HFE      = -13,
    SD1_ERR_HFE_CRC          = -14,
    SD1_ERR_HFE_MISSING_SEC  = -15,
    SD1_ERR_IO               = -16,
} Sd1Error;

const char* sd1_error_message(int code);  // static string, never free
```

### POD structs and enums

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
    uint8_t  type_info;
    uint8_t  file_type;          // Sd1FileType value
    char     name[12];           // null-terminated, max 11 chars
    uint16_t size_blocks;
    uint16_t contiguous_blocks;
    uint32_t first_block;
    uint8_t  file_number;
    uint32_t size_bytes;
} Sd1DirectoryEntry;

typedef struct {
    uint8_t  entry_type;         // 0=Free, 1=EOF, 2=BadBlock, 3=Next
    uint16_t next_block;         // valid only when entry_type == 3
} Sd1FatEntry;
```

### Opaque handle declarations

```c
typedef struct Sd1DiskImage  Sd1DiskImage;
typedef struct Sd1SysExPacket Sd1SysExPacket;
typedef struct Sd1Program    Sd1Program;
typedef struct Sd1Preset     Sd1Preset;
typedef struct Sd1Sequence   Sd1Sequence;
```

### DiskImage lifecycle

```c
Sd1DiskImage* sd1_disk_open(const char* path, int* err_out);
Sd1DiskImage* sd1_disk_create(void);
int           sd1_disk_save(const Sd1DiskImage*, const char* path);
void          sd1_disk_free(Sd1DiskImage*);
uint32_t      sd1_disk_free_blocks(const Sd1DiskImage*);
```

### Directory operations

```c
// Standard subdirectories (subdir_index 0–3)
Sd1DirectoryEntry* sd1_disk_list(const Sd1DiskImage*, uint8_t subdir, size_t* count_out);
int                sd1_disk_find(const Sd1DiskImage*, const char* name, Sd1DirectoryEntry* out);
int                sd1_disk_extract(const Sd1DiskImage*, const char* name,
                                    uint8_t** data_out, size_t* len_out);
int                sd1_disk_write(Sd1DiskImage*, const char* name,
                                  uint8_t file_type, bool programs_embedded,
                                  const uint8_t* data, size_t len, bool overwrite);
int                sd1_disk_delete(Sd1DiskImage*, const char* name);

// VST3 block-1 directory (Sojus-specific layout)
Sd1DirectoryEntry* sd1_block1_entries(const Sd1DiskImage*, size_t* count_out);
int                sd1_block1_find(const Sd1DiskImage*, const char* name, Sd1DirectoryEntry* out);

// Utilities
uint8_t sd1_next_file_number(const Sd1DiskImage*, uint8_t file_type);
uint8_t sd1_file_type_info(uint8_t file_type, bool programs_embedded);
int     sd1_validate_name(const char* name, uint8_t out[11]);
```

### HFE

```c
Sd1DiskImage* sd1_read_hfe(const char* path, int* err_out);
int           sd1_write_hfe(const Sd1DiskImage*, const char* path);
```

### FAT direct access

```c
void     sd1_fat_entry(const Sd1DiskImage*, uint16_t block, Sd1FatEntry* out);
int      sd1_fat_chain(const Sd1DiskImage*, uint16_t start,
                        uint16_t** blocks_out, size_t* count_out);
int      sd1_fat_allocate(Sd1DiskImage*, uint16_t n,
                           uint16_t** blocks_out, size_t* count_out);
void     sd1_fat_free_chain(Sd1DiskImage*, uint16_t start);
void     sd1_fat_set_chain(Sd1DiskImage*, const uint16_t* blocks, size_t count);
uint32_t sd1_fat_count_free(const Sd1DiskImage*);
```

### SysEx

```c
Sd1SysExPacket*  sd1_sysex_parse(const uint8_t* data, size_t len, int* err_out);
Sd1SysExPacket** sd1_sysex_parse_all(const uint8_t* data, size_t len,
                                      size_t* count_out, int* err_out);
uint8_t          sd1_sysex_message_type(const Sd1SysExPacket*);
uint8_t          sd1_sysex_midi_channel(const Sd1SysExPacket*);
uint8_t          sd1_sysex_model(const Sd1SysExPacket*);
const uint8_t*   sd1_sysex_payload(const Sd1SysExPacket*);
size_t           sd1_sysex_payload_len(const Sd1SysExPacket*);
uint8_t*         sd1_sysex_to_bytes(const Sd1SysExPacket*, uint8_t channel, size_t* len_out);
void             sd1_sysex_free(Sd1SysExPacket*);
void             sd1_sysex_packets_free(Sd1SysExPacket**, size_t count);
```

### Typed wrappers: Program, Preset, Sequence

```c
// Program
Sd1Program*     sd1_program_from_sysex(const Sd1SysExPacket*, int* err_out);
Sd1Program*     sd1_program_from_bytes(const uint8_t*, size_t, int* err_out);
const uint8_t*  sd1_program_bytes(const Sd1Program*, size_t* len_out);
void            sd1_program_name(const Sd1Program*, char* out, size_t out_len);
Sd1SysExPacket* sd1_program_to_sysex(const Sd1Program*, uint8_t channel);
uint8_t         sd1_program_file_type(const Sd1Program*);
void            sd1_program_free(Sd1Program*);

// Preset
Sd1Preset*      sd1_preset_from_sysex(const Sd1SysExPacket*, int* err_out);
Sd1Preset*      sd1_preset_from_bytes(const uint8_t*, size_t, int* err_out);
const uint8_t*  sd1_preset_bytes(const Sd1Preset*, size_t* len_out);
Sd1SysExPacket* sd1_preset_to_sysex(const Sd1Preset*, uint8_t channel);
uint8_t         sd1_preset_file_type(const Sd1Preset*);
void            sd1_preset_free(Sd1Preset*);

// Sequence
Sd1Sequence*    sd1_sequence_from_sysex(const Sd1SysExPacket*, int* err_out);
Sd1Sequence*    sd1_sequence_from_bytes(const uint8_t*, size_t);
const uint8_t*  sd1_sequence_bytes(const Sd1Sequence*, size_t* len_out);
Sd1SysExPacket* sd1_sequence_to_sysex(const Sd1Sequence*, uint8_t channel);
uint8_t         sd1_sequence_file_type(const Sd1Sequence*);
void            sd1_sequence_free(Sd1Sequence*);
```

### Type conversion functions

```c
int sd1_allsequences_to_disk(const uint8_t* payload, size_t payload_len,
                              const uint8_t* interleaved_progs, size_t progs_len,
                              uint8_t** out, size_t* out_len);
int sd1_disk_to_allsequences(const uint8_t* disk, size_t len, bool has_programs,
                              uint8_t** out, size_t* out_len);
int sd1_interleave_sixty_programs(const uint8_t* payload, size_t len,
                                   uint8_t** out, size_t* out_len);
int sd1_deinterleave_sixty_programs(const uint8_t* data, size_t len,
                                     uint8_t** out, size_t* out_len);
```

### Program utilities

```c
// out must be at least 12 bytes
void sd1_program_name_from_slot(const uint8_t* slot, size_t slot_len,
                                 char* out, size_t out_len);
// out must be at least 32 bytes (longest label: "ROM0[enc=127]=<11-char name>")
void sd1_decode_b10(uint8_t b10, const char** disk_programs, size_t count,
                    char* out, size_t out_len);

extern const char* const SD1_INT0_PROGRAMS[60];
extern const char* const SD1_ROM_ALL_PROGRAMS[120];
```

### Memory management

```c
void sd1_entries_free(Sd1DirectoryEntry*, size_t count);
void sd1_bytes_free(uint8_t*, size_t len);
void sd1_u16_array_free(uint16_t*, size_t count);
```

---

## Error handling convention

- Functions that return a pointer return `NULL` on failure; `*err_out` is set to a negative `Sd1Error` value.
- Functions that return `int` return `SD1_OK` (0) on success or a negative `Sd1Error` value on failure.
- `sd1_error_message(code)` returns a static human-readable string for any error code.
- `err_out` pointer parameters may be `NULL` if the caller doesn't need the code.

---

## Testing

- Unit tests live in `sd1ffi/src/lib.rs` as `#[test]` functions, exercising the FFI boundary (round-trips through opaque handles, null/error paths).
- A small `tests/c_smoke_test.c` compiled as part of CI validates that the generated header compiles cleanly as C and C++.
