#!/bin/bash
set -e

if [ -z "$CLI" ]; then
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
    cargo build --release -p tablet-cli --manifest-path "$ROOT_DIR/Cargo.toml" > /dev/null 2>&1
    CLI="$ROOT_DIR/target/release/tablet-cli"
fi

$CLI -w . generate-test --empty --format json > /dev/null 2>&1
$CLI -w . -s export.json.empty_as=omit export --json --java > /dev/null 2>&1

mkdir -p out
javac -d out -sourcepath gen/server/java gen/server/java/com/game/config/*.java \
    gen/server/java/com/game/config/types/*.java \
    gen/server/java/com/game/config/tpl/*.java \
    TestMain.java 2>&1 | grep -v "unchecked" | grep -v "^$" || true

java -cp out -Dfile.encoding=UTF-8 -Dstdout.encoding=UTF-8 TestMain gen/server/data/json
