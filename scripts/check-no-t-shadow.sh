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
check_pattern "template each-loop shadowing (as t)" '\{#each[^\n]*\bas t\b'

# Plain variable/function-loop names using `t`.
check_pattern "variable declaration named t" '\b(const|let|var)\s+t\b'
check_pattern "for-loop variable named t" 'for\s*\(\s*(const|let|var)\s+t\b'

# Callback parameters named `t` (single or parenthesized).
check_pattern "arrow callback parameter named t" '\bt\s*=>|\(\s*t(?:\s*:[^)]*)?\s*\)\s*=>|\(\s*[^,)]*,\s*t(?:\s*:[^)]*)?\s*\)\s*=>'

# get(t) is fragile in Svelte files that import i18n store `t`.
check_pattern "get(t) usage in .svelte files" '\bget\s*\(\s*t\s*\)'

# Lightweight same-line checks for $derived expressions.
check_pattern "store translation access inside \$derived (same line)" '\$derived(?:\.by)?\([^)]*(\$t\(|get\(t\))'
check_pattern "usage of t inside \$derived (same line)" '\$derived(?:\.by)?\([^)]*\bt\b'


if [[ "${had_issues}" -ne 0 ]]; then
  echo
  echo "check-no-t-shadow: FAIL"
  exit 1
fi

echo "check-no-t-shadow: OK"
