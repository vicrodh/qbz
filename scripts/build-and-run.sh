#!/bin/bash
# Build QBZ in release mode and run the binary
# Usage: ./scripts/build-and-run.sh [--skip-build]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BINARY_PATH="$PROJECT_DIR/src-tauri/target/release/qbz-nix"

cd "$PROJECT_DIR"

# Check for --skip-build flag
SKIP_BUILD=false
if [[ "$1" == "--skip-build" ]]; then
    SKIP_BUILD=true
fi

# Build if needed
if [[ "$SKIP_BUILD" == false ]] || [[ ! -f "$BINARY_PATH" ]]; then
    echo "=== Building QBZ (release mode) ==="
    echo "This may take a few minutes..."

    # Use npm run tauri build which handles frontend + backend properly
    npm run tauri build -- --no-bundle

    echo ""
    echo "=== Build complete ==="
    echo "Binary: $BINARY_PATH"
    echo "Size: $(du -h "$BINARY_PATH" | cut -f1)"
    echo ""
fi

# Kill any existing instance
pkill -f "qbz-nix" 2>/dev/null || true
sleep 0.5

echo "=== Running QBZ ==="
echo "Monitor with: btop or htop"
echo "Press Ctrl+C to stop"
echo ""

# Run the binary
exec "$BINARY_PATH"
