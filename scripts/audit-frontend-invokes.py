#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
from collections import Counter
from pathlib import Path


def load_text(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="ignore")


def main() -> int:
    root = Path(__file__).resolve().parent.parent

    front_globs = [
        "src/lib/**/*.ts",
        "src/lib/**/*.svelte",
        "src/routes/**/*.ts",
        "src/routes/**/*.svelte",
    ]

    invoke_pat = re.compile(r"invoke\(\s*['\"]([^'\"]+)['\"]")
    registrations_pat = re.compile(r"commands_v2::(v2_[A-Za-z0-9_]+|runtime_[A-Za-z0-9_]+)")

    front_paths = []
    for g in front_globs:
        front_paths.extend(root.glob(g))

    invokes: list[tuple[str, str]] = []
    for path in front_paths:
        text = load_text(path)
        rel = path.relative_to(root)
        for idx, line in enumerate(text.splitlines(), 1):
            for m in invoke_pat.finditer(line):
                invokes.append((m.group(1), f"{rel}:{idx}"))

    lib_rs = load_text(root / "src-tauri/src/lib.rs")
    registered_v2 = set(registrations_pat.findall(lib_rs))

    legacy_patterns = [
        r"commands::([A-Za-z0-9_]+)",
        r"library::commands::([A-Za-z0-9_]+)",
        r"cast::commands::([A-Za-z0-9_]+)",
        r"cast::dlna::commands::([A-Za-z0-9_]+)",
        r"offline_cache::commands::([A-Za-z0-9_]+)",
        r"offline::commands::([A-Za-z0-9_]+)",
        r"network::commands::([A-Za-z0-9_]+)",
        r"lyrics::commands::([A-Za-z0-9_]+)",
        r"reco_store::commands::([A-Za-z0-9_]+)",
    ]
    registered_legacy = set()
    for pat in legacy_patterns:
        registered_legacy |= set(re.findall(pat, lib_rs))

    v2_ok: list[tuple[str, str]] = []
    missing_v2: list[tuple[str, str]] = []
    legacy_calls: list[tuple[str, str]] = []
    unknown: list[tuple[str, str]] = []

    for cmd, loc in invokes:
        if cmd.startswith("v2_") or cmd.startswith("runtime_"):
            if cmd in registered_v2:
                v2_ok.append((cmd, loc))
            else:
                missing_v2.append((cmd, loc))
        else:
            if cmd in registered_legacy:
                legacy_calls.append((cmd, loc))
            else:
                unknown.append((cmd, loc))

    summary = {
        "total_callsites": len(invokes),
        "v2_ok_callsites": len(v2_ok),
        "missing_v2_callsites": len(missing_v2),
        "legacy_callsites": len(legacy_calls),
        "unknown_callsites": len(unknown),
        "unique_total": len({c for c, _ in invokes}),
        "unique_v2_ok": len({c for c, _ in v2_ok}),
        "unique_missing_v2": len({c for c, _ in missing_v2}),
        "unique_legacy": len({c for c, _ in legacy_calls}),
        "unique_unknown": len({c for c, _ in unknown}),
    }

    report_dir = root / "tmp"
    report_dir.mkdir(parents=True, exist_ok=True)
    (report_dir / "frontend_invoke_summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8"
    )

    def write_tsv(name: str, rows: list[tuple[str, str]]) -> None:
        with (report_dir / name).open("w", encoding="utf-8") as fh:
            for cmd, loc in rows:
                fh.write(f"{cmd}\t{loc}\n")

    write_tsv("frontend_v2_ok_callsites.tsv", v2_ok)
    write_tsv("frontend_missing_v2_callsites.tsv", missing_v2)
    write_tsv("frontend_legacy_callsites.tsv", legacy_calls)
    write_tsv("frontend_unknown_callsites.tsv", unknown)

    print(json.dumps(summary, indent=2))
    print("\nTop missing_v2 commands:")
    for cmd, count in Counter(c for c, _ in missing_v2).most_common(20):
        print(f"{count}\t{cmd}")

    print("\nTop legacy commands:")
    for cmd, count in Counter(c for c, _ in legacy_calls).most_common(20):
        print(f"{count}\t{cmd}")

    # Hard fail if frontend still invokes any legacy command.
    if legacy_calls:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
