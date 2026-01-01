#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT_DIR="$SCRIPT_DIR/../test-modules"

echo "=== Building all WASM modules ==="

# Rust modules
echo ""
echo "--- Building Rust modules ---"
for dir in "$SCRIPT_DIR"/rust/*/; do
    name=$(basename "$dir")
    echo "Building $name..."
    (cd "$dir" && cargo build --target wasm32-unknown-unknown --release 2>&1)
done

# AssemblyScript module (using stub runtime to avoid abort import)
echo ""
echo "--- Building AssemblyScript module ---"
if [ -f "$SCRIPT_DIR/assemblyscript/package.json" ]; then
    (cd "$SCRIPT_DIR/assemblyscript" && npx asc assembly/index.ts -o build/release.wasm --optimize --runtime stub 2>&1)
    echo "Built release.wasm"
else
    echo "AssemblyScript project not found, skipping..."
fi

# Zig module (freestanding, no WASI)
echo ""
echo "--- Building Zig module ---"
if command -v zig &> /dev/null; then
    (cd "$SCRIPT_DIR/zig/hello" && zig build-exe -target wasm32-freestanding -O ReleaseSmall main.zig -femit-bin=hello_zig.wasm 2>&1)
    echo "Built hello_zig.wasm"
else
    echo "Zig not installed, skipping..."
fi

# Copy to test-modules
echo ""
echo "--- Copying to test-modules ---"
mkdir -p "$OUTPUT_DIR"

# Rust
cp "$SCRIPT_DIR/rust/hello/target/wasm32-unknown-unknown/release/hello_rust.wasm" "$OUTPUT_DIR/"
cp "$SCRIPT_DIR/rust/fibonacci/target/wasm32-unknown-unknown/release/fibonacci_rust.wasm" "$OUTPUT_DIR/"
cp "$SCRIPT_DIR/rust/logging/target/wasm32-unknown-unknown/release/logging_rust.wasm" "$OUTPUT_DIR/"
cp "$SCRIPT_DIR/rust/memory/target/wasm32-unknown-unknown/release/memory_rust.wasm" "$OUTPUT_DIR/"

# AssemblyScript
if [ -f "$SCRIPT_DIR/assemblyscript/build/release.wasm" ]; then
    cp "$SCRIPT_DIR/assemblyscript/build/release.wasm" "$OUTPUT_DIR/hello_asc.wasm"
fi

# Zig
if [ -f "$SCRIPT_DIR/zig/hello/hello_zig.wasm" ]; then
    cp "$SCRIPT_DIR/zig/hello/hello_zig.wasm" "$OUTPUT_DIR/"
fi

echo ""
echo "=== Build complete ==="
echo "WASM modules in $OUTPUT_DIR:"
ls -la "$OUTPUT_DIR"/*.wasm 2>/dev/null | awk '{print $NF, $5}'
