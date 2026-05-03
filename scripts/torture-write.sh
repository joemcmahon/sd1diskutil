#!/usr/bin/env bash
# Torture test: write every SysEx file in a directory tree to SD-1 disk images.
# When a disk fills, start a new one. Report results.
#
# Usage: ./scripts/torture-write.sh <sysex-dir> [output-dir]
set -euo pipefail

SYSEX_DIR="${1:?Usage: $0 <sysex-dir> [output-dir]}"
OUTPUT_DIR="${2:-torture-out}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLI="$SCRIPT_DIR/../target/release/sd1cli"

mkdir -p "$OUTPUT_DIR"

DISK_IDX=1
CURRENT_DISK="$OUTPUT_DIR/disk-$(printf '%04d' $DISK_IDX).img"
"$CLI" create "$CURRENT_DISK"

WRITTEN=0
SKIPPED=0
FAILED=0
DISK_COUNT=1
TOTAL=0

log_file="$OUTPUT_DIR/results.log"
: > "$log_file"

# Derive a valid SD-1 disk name from a file path.
# Rules: uppercase, keep A-Z 0-9 space, strip the rest, truncate to 11.
derive_name() {
    local stem
    stem="$(basename "$1")"
    stem="${stem%.[Ss][Yy][Xx]}"             # strip .syx / .SYX
    stem="${stem//[^A-Za-z0-9 ]/ }"          # replace invalid chars with space
    stem="$(echo "$stem" | tr '[:lower:]' '[:upper:]')"  # uppercase
    stem="$(echo "$stem" | tr -s ' ')"       # collapse runs of spaces
    stem="${stem## }"                         # ltrim
    stem="${stem%% }"                         # rtrim
    stem="${stem:0:11}"                       # truncate to 11
    stem="${stem%% }"                         # rtrim again after truncate
    if [ -z "$stem" ]; then
        stem="FILE$(printf '%07d' "$TOTAL")"
    fi
    echo "$stem"
}

new_disk() {
    DISK_IDX=$((DISK_IDX + 1))
    DISK_COUNT=$((DISK_COUNT + 1))
    CURRENT_DISK="$OUTPUT_DIR/disk-$(printf '%04d' $DISK_IDX).img"
    "$CLI" create "$CURRENT_DISK"
    echo "  [new disk: $(basename "$CURRENT_DISK")]"
}

while IFS= read -r -d '' syx; do
    TOTAL=$((TOTAL + 1))
    NAME="$(derive_name "$syx")"

    result="$(
        "$CLI" write "$CURRENT_DISK" "$syx" --name "$NAME" --overwrite 2>&1
    )" && rc=0 || rc=$?

    if [ $rc -eq 0 ]; then
        WRITTEN=$((WRITTEN + 1))
        printf "OK    %s\n" "$NAME" >> "$log_file"
        continue
    fi

    # Disk full or directory full: start new disk and retry once
    if echo "$result" | grep -qE "Disk full|Directory full"; then
        new_disk
        result2="$(
            "$CLI" write "$CURRENT_DISK" "$syx" --name "$NAME" --overwrite 2>&1
        )" && rc2=0 || rc2=$?
        if [ $rc2 -eq 0 ]; then
            WRITTEN=$((WRITTEN + 1))
            printf "OK    %s  (new disk)\n" "$NAME" >> "$log_file"
            continue
        fi
        result="$result2"
        rc=$rc2
    fi

    # Unsupported type or parse error
    if echo "$result" | grep -qE "unsupported|Invalid|WrongMessage|not.*SD-1|manufacturer"; then
        SKIPPED=$((SKIPPED + 1))
        printf "SKIP  %s  -- %s\n" "$NAME" "$result" >> "$log_file"
    else
        FAILED=$((FAILED + 1))
        printf "FAIL  %s  -- %s\n" "$NAME" "$result" >> "$log_file"
    fi

done < <(find "$SYSEX_DIR" \( -name "*.syx" -o -name "*.SYX" \) -print0 | sort -z)

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Total files:   $TOTAL"
echo "Written:       $WRITTEN"
echo "Skipped:       $SKIPPED  (unsupported SysEx type)"
echo "Failed:        $FAILED   (parse/other errors)"
echo "Disk images:   $DISK_COUNT"
echo "Output dir:    $OUTPUT_DIR"
echo "Full log:      $log_file"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Show any failures for diagnosis
if [ "$FAILED" -gt 0 ]; then
    echo ""
    echo "Failures:"
    grep "^FAIL" "$log_file" | head -30
fi
