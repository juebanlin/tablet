#!/bin/bash
set -e

if [ -z "$CLI" ]; then
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
    cargo build --release -p tablet-cli --manifest-path "$ROOT_DIR/Cargo.toml" > /dev/null 2>&1
    CLI="$ROOT_DIR/target/release/tablet-cli"
fi

$CLI -w . generate-test --format json --lang go > /dev/null 2>&1
$CLI -w . export --json --go > /dev/null 2>&1

cd test_main_go && go run . ../gen/server/data/json
