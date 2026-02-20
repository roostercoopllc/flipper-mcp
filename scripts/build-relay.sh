#!/usr/bin/env bash
# build-relay.sh — Build the relay server binary (native x86_64/arm64).
# The relay runs on any Linux/macOS machine or VPS. No ESP toolchain needed.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."

echo "Building relay server..."
cd "$REPO_ROOT"
cargo build --release -p flipper-mcp-relay

BIN="$REPO_ROOT/target/release/flipper-mcp-relay"
echo ""
echo "Build successful. Binary: $BIN"
echo ""
echo "Run with:"
echo "  $BIN --listen 0.0.0.0:9090"
echo ""
echo "Or install system-wide:"
echo "  sudo cp $BIN /usr/local/bin/flipper-mcp-relay"
