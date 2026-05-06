#!/usr/bin/env bash
# Compare sd1cli (native Rust) vs sd1cli-ffi (via C FFI) for every file on a disk.
# Both CLIs extract each file to SysEx; outputs must be byte-for-byte identical.
#
# Usage: ./scripts/test-equivalence.sh <disk.img>
set -euo pipefail

DISK="${1:?Usage: $0 <disk.img>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLI="$SCRIPT_DIR/../target/debug/sd1cli"
CLI_FFI="$SCRIPT_DIR/../target/debug/sd1cli-ffi"
TMPDIR="$(mktemp -d)"
PASS=0
FAIL=0
SKIP=0

trap 'rm -rf "$TMPDIR"' EXIT

for bin in "$CLI" "$CLI_FFI"; do
    if [ ! -x "$bin" ]; then
        echo "Missing binary: $bin  (run: cargo build)" >&2
        exit 1
    fi
done

echo "Disk:    $DISK"
echo "Native:  $CLI"
echo "FFI:     $CLI_FFI"
echo ""

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

    SYX_NATIVE="$TMPDIR/${NAME}.native.syx"
    SYX_FFI="$TMPDIR/${NAME}.ffi.syx"

    if ! "$CLI" extract "$DISK" "$NAME" --out "$SYX_NATIVE" 2>/dev/null; then
        printf "FAIL  %-12s  (%s) — native extract failed\n" "$NAME" "$TYPE"
        FAIL=$((FAIL+1))
        continue
    fi

    if ! "$CLI_FFI" extract "$DISK" "$NAME" --output "$SYX_FFI" 2>/dev/null; then
        printf "FAIL  %-12s  (%s) — ffi extract failed\n" "$NAME" "$TYPE"
        FAIL=$((FAIL+1))
        continue
    fi

    if cmp -s "$SYX_NATIVE" "$SYX_FFI"; then
        printf "PASS  %-12s  (%s)\n" "$NAME" "$TYPE"
        PASS=$((PASS+1))
    else
        printf "DIFF  %-12s  (%s) — native vs ffi differ\n" "$NAME" "$TYPE"
        echo "  native: $(wc -c < "$SYX_NATIVE") bytes"
        echo "  ffi:    $(wc -c < "$SYX_FFI") bytes"
        xxd "$SYX_NATIVE" | head -4 | sed 's/^/  native: /'
        xxd "$SYX_FFI"    | head -4 | sed 's/^/  ffi:    /'
        FAIL=$((FAIL+1))
    fi

done < <("$CLI" list "$DISK" | grep -v '^NAME' | grep -v '^---' | grep -v '^$' | grep -v 'file(s)')

echo ""
echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"
[ "$FAIL" -eq 0 ]
