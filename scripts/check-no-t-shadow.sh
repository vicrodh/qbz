#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_DIR="${ROOT_DIR}/src"

echo "Checking for forbidden i18n 't' shadowing patterns in .svelte files..."

had_issues=0

check_pattern() {
  local label="$1"
  local pattern="$2"
  if rg -n --glob '*.svelte' "${pattern}" "${SRC_DIR}" >/tmp/qbz_t_shadow_check.out 2>/dev/null; then
    echo
    echo "Found ${label}:"
    cat /tmp/qbz_t_shadow_check.out
    had_issues=1
  fi
}

# Template loop variable shadowing.
check_pattern "template each-loop shadowing (as t)" '\{#each[^\n]* as t\b'

# Dangerous callbacks in files using $t(...).
while IFS= read -r file; do
  if rg -q '\b(map|forEach|filter|find|some|every|reduce)\(\s*t\s*=>|\b(map|forEach|filter|find|some|every|reduce)\(\s*\(\s*t\b[^)]*\)\s*=>|\b(map|forEach|filter|find|some|every|reduce)\(\s*\([^)]*,\s*t\b[^)]*\)\s*=>' "${file}"; then
    echo
    echo "Found callback shadowing in file using \$t(...): ${file}"
    rg -n '\b(map|forEach|filter|find|some|every|reduce)\(\s*t\s*=>|\b(map|forEach|filter|find|some|every|reduce)\(\s*\(\s*t\b[^)]*\)\s*=>|\b(map|forEach|filter|find|some|every|reduce)\(\s*\([^)]*,\s*t\b[^)]*\)\s*=>' "${file}"
    had_issues=1
  fi
done < <(rg -l --glob '*.svelte' '\$t\(' "${SRC_DIR}")

# Any $derived expression using variable name t.
check_pattern "usage of t inside \$derived" '\$derived\([^)]*\bt\b'


if [[ "${had_issues}" -ne 0 ]]; then
  echo
  echo "check-no-t-shadow: FAIL"
  exit 1
fi

echo "check-no-t-shadow: OK"
