/*
 * Smoke test: verify sd1disk.h compiles as C and C++.
 * This file is compiled by CI; it does not execute — only confirms the header is valid.
 */
#include "../sd1disk.h"
#include <stddef.h>
#include <stdint.h>

/* Verify struct field access compiles */
void check_struct_layout(void) {
    Sd1DirectoryEntry e;
    (void)e.type_info;
    (void)e.file_type;
    (void)e.name[0];
    (void)e.size_blocks;
    (void)e.contiguous_blocks;
    (void)e.first_block;
    (void)e.file_number;
    (void)e.size_bytes;

    Sd1FatEntry f;
    (void)f.entry_type;
    (void)f.next_block;
}
