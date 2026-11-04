#!/usr/bin/env python3
"""Check tools/extract_intact_sequences.py against `sd1cli extract`, slot by slot.

For every ThirtySequences and SixtySequences file on a disk image:

  * intact: every sequence the Rust library finds must be extracted, byte-identical.
  * damaged: with one block of the file zeroed, the tool may extract fewer sequences,
    but everything it does extract must still be byte-identical. A recovery tool
    must never produce wrong output.

Usage: scripts/verify-extractor.py <disk.img> [--tool PATH] [--cli PATH]
"""
import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BLOCK = 512


def denybblize(n: bytes) -> bytes:
    return bytes((n[i] << 4) | n[i + 1] for i in range(0, len(n), 2))


def sequence_files(cli, image):
    """Names of the sequence files on the image (sd1cli list: name is columns 1-12)."""
    out = subprocess.run([cli, "list", image], capture_output=True, text=True, check=True).stdout
    names = []
    for line in out.splitlines():
        if line[13:35].strip() in ("ThirtySequences", "SixtySequences"):
            names.append(line[:12].rstrip())
    return names


def reference_slots(cli, image, name, tmp):
    """Per-slot event bytes according to the Rust library (AllSequences pointer table)."""
    syx = tmp / "ref.syx"
    subprocess.run([cli, "extract", image, name, "--no-programs", "--out", str(syx)],
                   capture_output=True, check=True)
    payload = denybblize(syx.read_bytes()[6:-1])
    size = len(payload) - 29 - 60 * 186 - 252
    events = payload[252:252 + size]
    ptr = [int.from_bytes(payload[i * 4:i * 4 + 4], "big") for i in range(60)] + [size]
    return {s: events[ptr[s]:ptr[s + 1]] for s in range(60) if ptr[s + 1] > ptr[s]}


def tool_slots(tool, image, name, tmp):
    """Per-slot event bytes the extractor produced."""
    out = tmp / "out"
    out.mkdir(exist_ok=True)
    for old in out.glob("*.syx"):
        old.unlink()
    subprocess.run([sys.executable, "-I", tool, image, name, "--outdir", str(out)],
                   capture_output=True, text=True)
    return {int(f.stem.rsplit("slot", 1)[1]): denybblize(f.read_bytes()[6:-1])
            for f in out.glob("*.syx")}


def last_block_of(img: bytes, name: str):
    """Disk block holding the end of the file, found through the directory and FAT."""
    for n in range(4):
        base = (15 + 2 * n) * BLOCK
        for slot in range(39):
            e = img[base + slot * 26:base + slot * 26 + 26]
            if 0x0A <= e[1] <= 0x16 and e[2:13].decode("ascii", "replace").rstrip() == name:
                block = int.from_bytes(e[18:22], "big")
                for _ in range(int.from_bytes(e[14:16], "big") - 1):
                    off = (5 + block // 170) * BLOCK + (block % 170) * 3
                    nxt = int.from_bytes(img[off:off + 3], "big")
                    if nxt < 23:
                        break
                    block = nxt
                return block
    raise SystemExit(f"{name!r} not found in directory")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("image")
    ap.add_argument("--tool", default=str(ROOT / "tools/extract_intact_sequences.py"))
    ap.add_argument("--cli", default=str(ROOT / "target/debug/sd1cli"))
    args = ap.parse_args()

    failures = total = 0
    img = Path(args.image).read_bytes()
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        for name in sequence_files(args.cli, args.image):
            ref = reference_slots(args.cli, args.image, name, tmp)

            got = tool_slots(args.tool, args.image, name, tmp)
            wrong = sorted(s for s in got if got[s] != ref.get(s))
            missing = sorted(set(ref) - set(got))

            # Zero the file's last block: the tail of its data (or embedded programs).
            block = last_block_of(img, name)
            damaged = bytearray(img)
            damaged[block * BLOCK:(block + 1) * BLOCK] = bytes(BLOCK)
            dmg_path = tmp / "damaged.img"
            dmg_path.write_bytes(damaged)
            dmg = tool_slots(args.tool, str(dmg_path), name, tmp)
            dmg_wrong = sorted(s for s in dmg if dmg[s] != ref.get(s))

            ok = not (wrong or missing or dmg_wrong)
            total += 1
            failures += not ok
            print(f"{'PASS' if ok else 'FAIL'}  {name:12} {len(got):2}/{len(ref):2} intact"
                  f" | damaged {len(dmg):2}/{len(ref):2}"
                  + (f" | wrong {wrong[:6]}" if wrong else "")
                  + (f" | missing {missing[:6]}" if missing else "")
                  + (f" | wrong when damaged {dmg_wrong[:6]}" if dmg_wrong else ""))

    print(f"\nResults: {total - failures} passed, {failures} failed")
    if total == 0:
        print("no sequence files found on this image")
        return 1
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
