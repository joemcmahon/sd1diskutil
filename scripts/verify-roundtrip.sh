#!/usr/bin/env bash
# Verify round-trip extract+write for every SysEx-capable file type on a disk image.
# Skips: SequencerOs, SixPrograms, ThirtyPrograms (no SysEx message type exists for these).
#
# Strategy: extract each file to SysEx, write it to a fresh blank disk, extract it back,
# compare the two SysEx files byte-for-byte.
#
# Usage: ./scripts/verify-roundtrip.sh <disk.img>
set -euo pipefail

DISK="${1:?Usage: $0 <disk.img>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLI="$SCRIPT_DIR/../target/debug/sd1cli"
TMPDIR="$(mktemp -d)"
BLANK_DISK="$TMPDIR/blank.img"
PASS=0
FAIL=0
SKIP=0

trap 'rm -rf "$TMPDIR"' EXIT

"$CLI" create "$BLANK_DISK"

echo "Reference disk: $DISK"
echo ""

# cmd_list format: NAME is {:<12}, TYPE is {:<22}, then BLOCKS BYTES SLOT
# Use fixed-width cut: name=cols 1-12, type=cols 14-35 (1-indexed).
# Skip header lines (NAME and ----).
# Process substitution avoids a subshell so PASS/FAIL/SKIP counters survive the loop.

while IFS= read -r line; do
    NAME="$(echo "$line" | cut -c1-12 | sed 's/[[:space:]]*$//')"
    TYPE="$(echo "$line" | cut -c14-35 | sed 's/[[:space:]]*$//')"

    [ -z "$NAME" ] && continue
    [ -z "$TYPE" ] && continue

    case "$TYPE" in
        SequencerOs|SixPrograms|ThirtyPrograms)
            printf "SKIP  %-12s  (%s — no SysEx type)\n" "$NAME" "$TYPE"
            SKIP=$((SKIP+1))
            continue
            ;;
    esac

    SYX="$TMPDIR/orig.syx"
    SYX2="$TMPDIR/back.syx"

    # Extract from reference disk
    if ! "$CLI" extract "$DISK" "$NAME" --out "$SYX" 2>&1; then
        printf "FAIL  %-12s  (%s) — extract failed\n" "$NAME" "$TYPE"
        FAIL=$((FAIL+1))
        continue
    fi

    # Write to blank disk (truncate name to 8 chars + TST suffix to stay within 11)
    WNAME="${NAME:0:8}TST"
    if ! "$CLI" write "$BLANK_DISK" "$SYX" --name "$WNAME" 2>&1; then
        printf "FAIL  %-12s  (%s) — write failed\n" "$NAME" "$TYPE"
        FAIL=$((FAIL+1))
        continue
    fi

    # Extract back from blank disk
    if ! "$CLI" extract "$BLANK_DISK" "$WNAME" --out "$SYX2" 2>&1; then
        printf "FAIL  %-12s  (%s) — re-extract failed\n" "$NAME" "$TYPE"
        FAIL=$((FAIL+1))
        "$CLI" delete "$BLANK_DISK" "$WNAME" 2>/dev/null || true
        continue
    fi

    if cmp -s "$SYX" "$SYX2"; then
        printf "PASS  %-12s  (%s)\n" "$NAME" "$TYPE"
        PASS=$((PASS+1))
    else
        printf "DIFF  %-12s  (%s) — SysEx differs after round-trip\n" "$NAME" "$TYPE"
        FAIL=$((FAIL+1))
    fi

    "$CLI" delete "$BLANK_DISK" "$WNAME" 2>/dev/null || true
done < <("$CLI" list "$DISK" | grep -v '^NAME' | grep -v '^---' | grep -v '^$' | grep -v 'file(s)')

echo ""
echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"
[ "$FAIL" -eq 0 ]
