#!/bin/bash
set -e

$CLI -w . generate-test --format xml > /dev/null 2>&1
$CLI -w . export --xml --java > /dev/null 2>&1

mkdir -p out
javac -d out -sourcepath gen/server/code gen/server/code/com/game/config/*.java \
    gen/server/code/com/game/config/types/*.java \
    gen/server/code/com/game/config/hero/*.java \
    $(find gen/server/code -name "*.java" -path "*/global/*" 2>/dev/null) \
    TestMain.java 2>&1 | grep -v "unchecked" | grep -v "^$" || true

java -cp out -Dfile.encoding=UTF-8 -Dstdout.encoding=UTF-8 TestMain gen/server/data/xml
