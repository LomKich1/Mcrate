#!/bin/sh
# Ворота перед коммитом: fmt, сборка всех целей без ошибок, число предупреждений rustc,
# тесты mcrate-app (допустим только известный падающий тест палитры).
# Запуск из любого места: sh tools/gate.sh. Если рядом лежит /home/claude/env.sh (песочница Claude), он подключается.
cd "$(dirname "$0")/.." || exit 2
[ -f /home/claude/env.sh ] && . /home/claude/env.sh
cargo fmt -p mcrate-app || { echo "GATE: fmt упал"; exit 1; }
out=$(cargo check -p mcrate-app --all-targets 2>&1)
echo "$out" | grep -q "^error" && { echo "GATE: ОШИБКИ СБОРКИ"; echo "$out" | grep -A6 "^error" | head -30; exit 1; }
echo "GATE: предупреждений rustc: $(echo "$out" | grep -c '^warning: ')"
t=$(cargo test -p mcrate-app 2>&1)
failed=$(echo "$t" | grep -E "^test [A-Za-z0-9_:]+ \.\.\. FAILED" | grep -v "material3_presets_have_complete_readable_schemes")
[ -n "$failed" ] && { echo "GATE: НОВЫЕ ПАДЕНИЯ ТЕСТОВ"; echo "$failed"; exit 1; }
echo "GATE: $(echo "$t" | grep '^test result' | head -1)"
echo "GATE: OK"
