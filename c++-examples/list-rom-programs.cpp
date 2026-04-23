// list-rom-programs.cpp
// Prints all 120 SD-1 ROM program names, grouped by bank (ROM 0 and ROM 1).
//
// Build:
//   macOS:  c++ -std=c++17 -I/path/to/sd1ffi list-rom-programs.cpp \
//               /path/to/libsd1disk.a -framework CoreFoundation -o list-rom-programs
//   Linux:  c++ -std=c++17 -I/path/to/sd1ffi list-rom-programs.cpp \
//               /path/to/libsd1disk.a -lpthread -ldl -o list-rom-programs
//   MSVC:   cl /std:c++17 /I\path\to\sd1ffi list-rom-programs.cpp \
//               \path\to\sd1disk.lib /Fe:list-rom-programs.exe

#include "sd1disk.h"

#include <cstdio>

int main() {
    const char *const *rom  = sd1_rom_all_programs();  // 120 entries: ROM0[0-59], ROM1[60-119]
    const char *const *int0 = sd1_int0_programs();     // 60 entries: factory user bank

    std::printf("=== ROM 0 (%d programs) ===\n", 60);
    for (int i = 0; i < 60; ++i) {
        std::printf("  ROM0 %3d: %s\n", i, rom[i]);
    }

    std::printf("\n=== ROM 1 (%d programs) ===\n", 60);
    for (int i = 0; i < 60; ++i) {
        std::printf("  ROM1 %3d: %s\n", i, rom[60 + i]);
    }

    std::printf("\n=== INT0 factory user bank (%d programs) ===\n", 60);
    for (int i = 0; i < 60; ++i) {
        std::printf("  INT0 %3d: %s\n", i, int0[i]);
    }

    return 0;
}
