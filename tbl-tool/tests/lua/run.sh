#!/bin/bash
set -e

if [ -z "$CLI" ]; then
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
    cargo build --release -p tbl-cli --manifest-path "$ROOT_DIR/Cargo.toml" > /dev/null 2>&1
    CLI="$ROOT_DIR/target/release/tbl-cli"
fi

$CLI -w . generate-test --format json > /dev/null 2>&1
$CLI -w . export --lua > /dev/null 2>&1

echo "=== HeroBase.lua ==="
cat gen/client/hero/HeroBase.lua
echo ""
echo "=== GlobalConst.lua ==="
cat gen/client/global/GlobalConst.lua
