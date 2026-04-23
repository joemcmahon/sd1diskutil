// create-sixty-sequences-disk.cpp
// Reads an AllSequences SysEx file (and optionally an AllPrograms SysEx file),
// converts them to on-disk format, and writes a new SD-1 disk image.
//
// Usage:
//   create-sixty-sequences-disk <sequences.syx> <output.img>
//   create-sixty-sequences-disk <sequences.syx> <programs.syx> <output.img>
//
// The sequences file must contain a single AllSequences packet (type 0x0A).
// The programs file must contain a single AllPrograms packet (type 0x03);
// when present, the resulting disk file embeds the 60 programs alongside the
// sequences (SD1_FILE_SIXTY_SEQUENCES with programs_embedded = true).
//
// Build:
//   macOS:  c++ -std=c++17 -I/path/to/sd1ffi create-sixty-sequences-disk.cpp \
//               /path/to/libsd1disk.a -framework CoreFoundation -o create-sixty-sequences-disk
//   Linux:  c++ -std=c++17 -I/path/to/sd1ffi create-sixty-sequences-disk.cpp \
//               /path/to/libsd1disk.a -lpthread -ldl -o create-sixty-sequences-disk
//   MSVC:   cl /std:c++17 /I\path\to\sd1ffi create-sixty-sequences-disk.cpp \
//               \path\to\sd1disk.lib /Fe:create-sixty-sequences-disk.exe

#include "sd1disk.h"

#include <cstdio>
#include <cstdlib>
#include <vector>

// ── helpers ──────────────────────────────────────────────────────────────────

static std::vector<uint8_t> read_file(const char *path) {
    FILE *f = std::fopen(path, "rb");
    if (!f) return {};
    std::fseek(f, 0, SEEK_END);
    long len = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    std::vector<uint8_t> buf(static_cast<size_t>(len));
    std::fread(buf.data(), 1, buf.size(), f);
    std::fclose(f);
    return buf;
}

static Sd1SysExPacket *find_packet(Sd1SysExPacket **pkts, size_t count, uint8_t type) {
    for (size_t i = 0; i < count; ++i) {
        if (sd1_sysex_message_type(pkts[i]) == type) return pkts[i];
    }
    return nullptr;
}

// ── main ─────────────────────────────────────────────────────────────────────

int main(int argc, char *argv[]) {
    if (argc < 3 || argc > 4) {
        std::fprintf(stderr,
            "Usage:\n"
            "  %s <sequences.syx> <output.img>\n"
            "  %s <sequences.syx> <programs.syx> <output.img>\n",
            argv[0], argv[0]);
        return 1;
    }

    const char *seq_path  = argv[1];
    const char *prog_path = (argc == 4) ? argv[2] : nullptr;
    const char *out_path  = argv[argc - 1];

    // ── parse sequences ───────────────────────────────────────────────────────

    auto seq_syx = read_file(seq_path);
    if (seq_syx.empty()) {
        std::fprintf(stderr, "Error: could not read '%s'\n", seq_path);
        return 1;
    }

    int    seq_err   = 0;
    size_t seq_count = 0;
    Sd1SysExPacket **seq_pkts = sd1_sysex_parse_all(seq_syx.data(), seq_syx.size(),
                                                     &seq_count, &seq_err);
    if (!seq_pkts) {
        std::fprintf(stderr, "Error parsing '%s': %s\n", seq_path, sd1_error_message(seq_err));
        return 1;
    }

    Sd1SysExPacket *seq_pkt = find_packet(seq_pkts, seq_count, 0x0A); // AllSequences
    if (!seq_pkt) {
        std::fprintf(stderr, "Error: no AllSequences packet (type 0x0A) in '%s'\n", seq_path);
        sd1_sysex_packets_free(seq_pkts, seq_count);
        return 1;
    }

    const uint8_t *seq_payload = sd1_sysex_payload(seq_pkt);
    size_t         seq_len     = sd1_sysex_payload_len(seq_pkt);

    // ── optionally parse programs ─────────────────────────────────────────────

    uint8_t *interleaved_progs = nullptr;
    size_t   interleaved_len   = 0;

    if (prog_path) {
        auto prog_syx = read_file(prog_path);
        if (prog_syx.empty()) {
            std::fprintf(stderr, "Error: could not read '%s'\n", prog_path);
            sd1_sysex_packets_free(seq_pkts, seq_count);
            return 1;
        }

        int    prog_err   = 0;
        size_t prog_count = 0;
        Sd1SysExPacket **prog_pkts = sd1_sysex_parse_all(prog_syx.data(), prog_syx.size(),
                                                          &prog_count, &prog_err);
        if (!prog_pkts) {
            std::fprintf(stderr, "Error parsing '%s': %s\n", prog_path, sd1_error_message(prog_err));
            sd1_sysex_packets_free(seq_pkts, seq_count);
            return 1;
        }

        Sd1SysExPacket *prog_pkt = find_packet(prog_pkts, prog_count, 0x03); // AllPrograms
        if (!prog_pkt) {
            std::fprintf(stderr, "Warning: no AllPrograms packet (type 0x03) in '%s'; "
                                 "continuing without programs\n", prog_path);
        } else {
            // AllPrograms payload is 60 × 530 bytes in sequential order;
            // on-disk format requires them byte-interleaved.
            int rc = sd1_interleave_sixty_programs(
                sd1_sysex_payload(prog_pkt),
                sd1_sysex_payload_len(prog_pkt),
                &interleaved_progs, &interleaved_len);
            if (rc != SD1_OK) {
                std::fprintf(stderr, "Error interleaving programs: %s\n", sd1_error_message(rc));
                sd1_sysex_packets_free(prog_pkts, prog_count);
                sd1_sysex_packets_free(seq_pkts, seq_count);
                return 1;
            }
        }

        sd1_sysex_packets_free(prog_pkts, prog_count);
    }

    // ── convert to on-disk format ─────────────────────────────────────────────

    bool     has_progs = (interleaved_progs != nullptr);
    uint8_t *disk_data = nullptr;
    size_t   disk_len  = 0;

    int rc = sd1_allsequences_to_disk(
        seq_payload, seq_len,
        interleaved_progs, interleaved_len,
        &disk_data, &disk_len);

    sd1_sysex_packets_free(seq_pkts, seq_count);
    if (interleaved_progs) sd1_bytes_free(interleaved_progs, interleaved_len);

    if (rc != SD1_OK) {
        std::fprintf(stderr, "Error converting to disk format: %s\n", sd1_error_message(rc));
        return 1;
    }

    // ── create disk and write the file ────────────────────────────────────────

    Sd1DiskImage *img = sd1_disk_create();

    rc = sd1_disk_write(
        img,
        "MY-SEQS",
        SD1_FILE_SIXTY_SEQUENCES,
        has_progs,          // programs_embedded: controls the type_info byte
        disk_data, disk_len,
        /*overwrite=*/false);

    sd1_bytes_free(disk_data, disk_len);

    if (rc != SD1_OK) {
        std::fprintf(stderr, "Error writing file to disk image: %s\n", sd1_error_message(rc));
        sd1_disk_free(img);
        return 1;
    }

    rc = sd1_disk_save(img, out_path);
    sd1_disk_free(img);

    if (rc != SD1_OK) {
        std::fprintf(stderr, "Error saving '%s': %s\n", out_path, sd1_error_message(rc));
        return 1;
    }

    std::printf("Wrote %s  [%s]\n", out_path,
                has_progs ? "sixty sequences + programs" : "sixty sequences");
    return 0;
}
