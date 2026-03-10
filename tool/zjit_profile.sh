#!/bin/bash
# Profile Ruby with ZJIT and show HIR-annotated source in Firefox Profiler.
#
# Usage:
#   tool/zjit_profile.sh [options] -- ruby [ruby-args] script.rb
#   tool/zjit_profile.sh [options] -- ./build-dev/miniruby [ruby-args] script.rb
#
# Options:
#   --save FILE    Save profile JSON to FILE (default: /tmp/zjit-profile.json)
#   --no-open      Don't open the profiler UI
#   --help         Show this help
#
# Examples:
#   tool/zjit_profile.sh -- ruby my_script.rb
#   tool/zjit_profile.sh -- ./build-dev/miniruby --zjit-call-threshold=10 -e '1000.times { 1+1 }'
#   tool/zjit_profile.sh --save prof.json -- ruby benchmarks/railsbench/benchmark.rb
#
# The script automatically:
#   - Adds --zjit --zjit-perf flags to the ruby command
#   - Cleans up old jitdump/perf-map/hir files
#   - Runs samply record
#   - Shows where the HIR file is written
#
# After profiling, click on a zjit:: function in the Call Tree, then look at
# the Source tab to see HIR instructions with per-instruction sample counts.

set -e

SAVE_FILE="/tmp/zjit-profile.json"
SAMPLY_ARGS=()
OPEN=true

# Parse our options (before --)
while [[ $# -gt 0 ]]; do
    case "$1" in
        --save)
            SAVE_FILE="$2"
            shift 2
            ;;
        --no-open)
            OPEN=false
            shift
            ;;
        --help|-h)
            sed -n '2,/^$/s/^# \?//p' "$0"
            exit 0
            ;;
        --)
            shift
            break
            ;;
        *)
            echo "Unknown option: $1 (use -- to separate profiler options from ruby command)" >&2
            exit 1
            ;;
    esac
done

if [[ $# -eq 0 ]]; then
    echo "Usage: $0 [options] -- ruby [ruby-args] script.rb" >&2
    echo "Run '$0 --help' for more information." >&2
    exit 1
fi

# Check samply is installed
if ! command -v samply &>/dev/null; then
    echo "Error: samply not found. Install with: cargo install samply" >&2
    exit 1
fi

# Clean up old files
rm -f /tmp/jit-*.dump /tmp/perf-*.map /tmp/zjit-hir-*.hir 2>/dev/null

# Build the ruby command, injecting --zjit --zjit-perf
RUBY_CMD=("$1")
shift

# Check if --zjit is already in the args
HAS_ZJIT=false
HAS_PERF=false
for arg in "$@"; do
    [[ "$arg" == "--zjit" || "$arg" == --zjit-* ]] && HAS_ZJIT=true
    [[ "$arg" == "--zjit-perf" ]] && HAS_PERF=true
done

RUBY_ARGS=()
$HAS_ZJIT || RUBY_ARGS+=(--zjit)
$HAS_PERF || RUBY_ARGS+=(--zjit-perf)
RUBY_ARGS+=("$@")

if $OPEN; then
    SAMPLY_ARGS+=(-o "$SAVE_FILE")
else
    SAMPLY_ARGS+=(--no-open --save-only -o "$SAVE_FILE")
fi

echo "Recording profile..."
echo "  Command: ${RUBY_CMD[*]} ${RUBY_ARGS[*]}"
echo "  Profile: $SAVE_FILE"
echo ""

# Run samply. Afterward, copy jitdump/HIR files since samply may delete them.
# We run ruby once first to discover the PID pattern, but that's complex.
# Instead, snapshot all matching files right after samply finishes.
samply record "${SAMPLY_ARGS[@]}" -- "${RUBY_CMD[@]}" "${RUBY_ARGS[@]}"

# Preserve copies of jitdump/HIR files alongside the profile
SAVE_DIR=$(dirname "$SAVE_FILE")
for f in /tmp/jit-*.dump /tmp/zjit-hir-*.hir; do
    [ -f "$f" ] && cp "$f" "$SAVE_DIR/" 2>/dev/null
done

# Show where the HIR file ended up
HIR_FILES=(/tmp/zjit-hir-*.hir)
if [[ -f "${HIR_FILES[0]}" ]]; then
    echo ""
    echo "HIR source file: ${HIR_FILES[0]}"
    LINES=$(wc -l < "${HIR_FILES[0]}")
    FUNCS=$(grep -c '^fn ' "${HIR_FILES[0]}" 2>/dev/null || echo 0)
    echo "  $FUNCS functions, $LINES lines"
fi
