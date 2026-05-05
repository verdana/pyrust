#!/bin/bash
# One-way sync: WSL -> Windows
# Usage: ./scripts/sync-to-windows.sh

set -e

WSL_ROOT="/home/verdana/workspace/pyrust"
WIN_ROOT="/mnt/c/Users/Verdana/Desktop/pyrust"

echo "Syncing WSL -> Windows..."

# TSF crate source
cp "$WSL_ROOT/crates/tsf/src/"*.rs "$WIN_ROOT/crates/tsf/src/"
echo "  tsf/src/*.rs"

# engine-core
cp "$WSL_ROOT/crates/engine-core/src/lib.rs" "$WIN_ROOT/crates/engine-core/src/"
echo "  engine-core/src/lib.rs"

# CLAUDE.md
cp "$WSL_ROOT/CLAUDE.md" "$WIN_ROOT/"
echo "  CLAUDE.md"

# build-tsf.ps1
cp "$WSL_ROOT/scripts/build-tsf.ps1" "$WIN_ROOT/scripts/build-tsf.ps1"
echo "  scripts/build-tsf.ps1"

echo "Done."
