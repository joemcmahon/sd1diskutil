---
date: 2026-03-23T02:52:16Z
session_name: general
researcher: Claude
git_commit: 5ad7ad4
branch: main
repository: sd1diskutil
topic: "SD-1 Disk Image Utility — Design and Implementation Plan"
tags: [rust, ensoniq, sd-1, sysex, disk-image, design, implementation-plan]
status: complete
last_updated: 2026-03-22
last_updated_by: Claude
type: implementation_strategy
root_span_id: ""
turn_span_id: ""
---

# Handoff: SD-1 Disk Utility — Design complete, ready for subagent implementation

## Task(s)

**COMPLETED: Brainstorming and design**
- Researched and read both reference documents (SD-1 SysEx spec, Giebler Ensoniq disk format doc)
- Conducted full brainstorming session to determine language, architecture, and operations scope
- Produced a reviewed and approved design spec
- Produced a reviewed and approved implementation plan
- Committed all artifacts to `main`

**NEXT: Implement the plan using superpowers:subagent-driven-development**
- The user chose Subagent-Driven execution (Option 1)
- The plan has 13 tasks; none have been started yet
- Start from Task 1 (Workspace Bootstrap)

## Critical References

- **Design spec:** `docs/superpowers/specs/2026-03-22-sd1diskutil-design.md`
- **Implementation plan:** `docs/superpowers/plans/2026-03-22-sd1diskutil.md`
- **Reference docs:** `SD1-SYSEX.pdf` and `ensoniq_floppy_diskette_formats.pdf` (both at repo root)

## Recent changes

- `docs/superpowers/specs/2026-03-22-sd1diskutil-design.md` — Full design spec (created this session)
- `docs/superpowers/plans/2026-03-22-sd1diskutil.md` — Full implementation plan with TDD steps (created this session)
- `blank_image.img` — Known-good blank SD-1 disk image (committed as reference/template)
- `disk_with_everything.img` — SD-1 disk image with files (committed as integration test fixture)

## Learnings

**Disk format (critical for implementation):**
- SD-1 shares its disk format with VFX-SD. 80 tracks × 2 heads × 10 sectors × 512 bytes = 1,600 blocks
- Block formula: `Block = ((Track × 2) + Head) × 10 + Sector`
- **All multi-byte fields are big-endian** (68000-family CPU) — use `from_be_bytes()` everywhere
- FAT: blocks 5–14, 170 entries × 3 bytes per block. Raw 24-bit value determines type (0=Free, 1=EOF, 2=Bad, other=Next block)
- Sub-directories: blocks 15–22 (4 dirs × 2 blocks each), 39 entries × 26 bytes each = 156 files max
- Directory entry layout is 1-indexed in Giebler doc; plan uses 0-indexed. Byte 13 (0-indexed) is `_reserved`, always zero
- OS block (block 2): bytes 0–3 = free block count (big-endian u32); bytes 28–29 = "OS"

**SysEx format:**
- Nybblized: each byte → `0000HHHH 0000LLLL` (hi nybble first)
- Header: `F0 0F 05 00 [chan] [msg_type]`, tail: `F7`
- One Program = 530 bytes internal / 1060 MIDI bytes; One Preset = 48 bytes / 96 MIDI bytes
- Program name is at bytes 498–508 of the 530-byte payload

**Rust architecture decisions:**
- `FileAllocationTable` and `SubDirectory` are **stateless handles** (no lifetime parameters) — they take `&DiskImage` or `&mut DiskImage` as method arguments. This avoids Rust borrow-checker conflicts when FAT and directory mutations are interleaved in the same operation.
- `DiskImage::save()` writes to a temp file then renames — atomic write, no partial corruption
- `DiskImage::create()` uses `include_bytes!("../../../blank_image.img")` — embeds known-good image at compile time
- `block()` returns `Result<&[u8]>` (not infallible) — error if n >= 1600
- `allocate()` and `free_chain()` do NOT update the OS block free count — caller must call `image.set_free_blocks()` explicitly
- Name matching in `find()` is **case-sensitive**
- `name()` methods return `Cow<'_, str>` via `from_utf8_lossy()` — never panics on non-UTF-8 disk names
- `validate_name()` is public and enforced inside `SubDirectory::add()` regardless of call site

**No existing library support:** Checked thoroughly — no Rust/Python/Go library handles SD-1/VFX-SD disk images. The EPS/ASR tools (EnsoniqFS, EnDiskEx) target a different Ensoniq product family.

**Swift bridge path:** Mozilla UniFFI generates Swift bindings from Rust. `DiskImage`, `Program`, `Preset`, `Sequence`, and `Error` are the intended UniFFI surface. `FileAllocationTable` and `SubDirectory` are internal and not part of the bridge surface. `Error` must implement `std::error::Error` + `Display` for UniFFI error bridging (already in the plan).

## Post-Mortem

### What Worked
- **Embedding blank_image.img via `include_bytes!`**: Rather than constructing a blank image from spec (risky), the plan embeds the known-good hardware-verified blank image. This eliminates an entire class of initialization bugs.
- **Stateless handles for FAT and SubDirectory**: The key insight that solves the Rust dual-borrow problem. Making them stateless (no `&self` lifetime tied to `DiskImage`) means FAT ops and directory ops can be interleaved freely in the WRITE pipeline.
- **Spec review loop**: The automated reviewer caught 15 real issues in the first pass including a critical dual-borrow design flaw and a missing `free_chain()` call in the --overwrite path. All were resolved before the plan was written.
- **TDD task structure**: Each task in the plan has write-test → run-fail → implement → run-pass → commit structure, which is especially important for a Rust newcomer.

### What Failed
- Nothing failed in this design/planning session. The spec review found issues that were corrected before implementation.

### Key Decisions
- **Language: Rust** — No library support exists in any language; user chose Rust for the learning experience and Swift interop via UniFFI.
- **Architecture: Library + thin CLI** — `sd1disk` crate (pure library) + `sd1cli` crate (clap binary). Library has no CLI concerns; CLI has no disk logic.
- **Stateless FAT/SubDirectory handles** — Alternatives (borrowing views with lifetimes) would have caused borrow-checker conflicts in the WRITE pipeline where both need `&mut DiskImage` simultaneously.
- **String type for WrongMessageType fields** — Spec used `MessageType` enum fields; plan uses `String` (via `.display_name()`). Self-consistent and avoids making `MessageType` Clone, which adds complexity for no real benefit.
- **OS block free count is caller's responsibility** — `allocate()` and `free_chain()` don't update it automatically, forcing the operation-level code to do it explicitly. This makes the obligation visible at the call site.

## Artifacts

- `docs/superpowers/specs/2026-03-22-sd1diskutil-design.md` — Approved design spec
- `docs/superpowers/plans/2026-03-22-sd1diskutil.md` — Approved implementation plan (13 tasks, TDD)
- `blank_image.img` — Blank SD-1 disk image (template for `DiskImage::create()`)
- `disk_with_everything.img` — SD-1 disk with files (integration test fixture for `disk_with_everything.img`-based tests in Tasks 8–11)
- `SD1-SYSEX.pdf` — Ensoniq SD-1 MIDI SysEx Specification v3.11
- `ensoniq_floppy_diskette_formats.pdf` — Giebler Ensoniq Floppy Diskette Formats (disk layout reference)

## Action Items & Next Steps

1. **Invoke `superpowers:subagent-driven-development`** to execute the plan task by task
2. Start with **Task 1: Workspace Bootstrap** (`docs/superpowers/plans/2026-03-22-sd1diskutil.md`)
3. Each task: subagent implements → review → next task
4. After Task 7 (all library modules done), run `cargo test -p sd1disk` — all unit tests should pass before proceeding to integration tests
5. After Task 11 (all operations tested), proceed to Task 12 (CLI)
6. After Task 13 (final verification), smoke test with a real `.syx` file if the user has one

**Known gaps to address post-v0.1 (documented in plan's "Known Gaps" section):**
- AllPrograms SysEx (60 programs): write cmd only handles OneProgram
- Preset name extraction: no `name()` on `Preset` yet (no obvious name field in spec)
- UniFFI Swift bindings
- `--dir` flag validation when specified directory is full

## Other Notes

**File locations summary:**
- All source will live under `crates/sd1disk/src/` and `crates/sd1cli/src/`
- Integration tests: `crates/sd1disk/tests/operations_tests.rs`
- The `blank_image.img` path in `include_bytes!` is `"../../../blank_image.img"` relative to `crates/sd1disk/src/image.rs` (3 levels up to workspace root)

**CLI binary name:** The binary is named `sd1cli` (from the package name), NOT `sd1disk`. Built at `./target/release/sd1cli`.

**Test commands:**
- Library only: `cargo test -p sd1disk`
- CLI only: `cargo test -p sd1cli`
- All: `cargo test`
- Specific test: `cargo test -p sd1disk fat`

**FAT byte offset formula** (useful for debugging):
```
fat_block_index = block_number / 170
offset_in_block = (block_number % 170) * 3
byte_in_image   = (5 + fat_block_index) * 512 + offset_in_block
```

**SubDirectory byte offset formula:**
```
base = (15 + dir_index * 2) * 512
entry_offset = base + slot_index * 26
```
