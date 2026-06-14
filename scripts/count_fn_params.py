#!/usr/bin/env python3
"""Count Rust/TS function parameters; report functions with more than N args."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def count_params(params: str) -> int:
    depth = 0
    count = 0
    segment = ""
    for ch in params + ",":
        if ch in "([<{": depth += 1
        elif ch in ")]>}": depth -= 1
        elif ch == "," and depth == 0:
            if segment.strip():
                count += 1
            segment = ""
            continue
        segment += ch
    if segment.strip():
        count += 1
    return count


RUST_FN = re.compile(r"\b(?:pub\s+)?(?:async\s+)?fn\s+(\w+)\s*\(")
TS_FN = re.compile(
    r"\b(?:export\s+)?(?:async\s+)?function\s+(\w+)\s*\("
    r"|\b(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s*)?\("
)


def scan_rust(path: Path) -> list[tuple[int, str, int, str]]:
    text = path.read_text(encoding="utf-8", errors="replace")
    results: list[tuple[int, str, int, str]] = []
    for m in RUST_FN.finditer(text):
        name = m.group(1)
        pos = m.end() - 1
        depth = 0
        for j in range(pos, len(text)):
            if text[j] == "(":
                depth += 1
            elif text[j] == ")":
                depth -= 1
                if depth == 0:
                    n = count_params(text[pos + 1 : j])
                    if n > 0:
                        line = text[: m.start()].count("\n") + 1
                        results.append((n, str(path), line, name))
                    break
    return results


def scan_ts(path: Path) -> list[tuple[int, str, int, str]]:
    text = path.read_text(encoding="utf-8", errors="replace")
    results: list[tuple[int, str, int, str]] = []
    for m in TS_FN.finditer(text):
        name = m.group(1) or m.group(2)
        pos = m.end() - 1
        depth = 0
        for j in range(pos, len(text)):
            if text[j] == "(":
                depth += 1
            elif text[j] == ")":
                depth -= 1
                if depth == 0:
                    n = count_params(text[pos + 1 : j])
                    if n > 0:
                        line = text[: m.start()].count("\n") + 1
                        results.append((n, str(path), line, name))
                    break
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", help="Files or directories to scan")
    parser.add_argument("--min", type=int, default=11, help="Report functions with > this many params")
    args = parser.parse_args()

    rows: list[tuple[int, str, int, str]] = []
    for raw in args.paths:
        path = Path(raw)
        if path.is_file():
            if path.suffix == ".rs":
                rows.extend(scan_rust(path))
            elif path.suffix in {".ts", ".vue", ".js"}:
                rows.extend(scan_ts(path))
        elif path.is_dir():
            for f in sorted(path.rglob("*")):
                if "node_modules" in f.parts or "target" in f.parts:
                    continue
                if f.suffix == ".rs":
                    rows.extend(scan_rust(f))
                elif f.suffix in {".ts", ".vue", ".js"}:
                    rows.extend(scan_ts(f))

    threshold = args.min
    hot = [r for r in rows if r[0] > threshold]
    hot.sort(key=lambda r: (-r[0], r[1], r[2]))

    for n, path, line, name in hot:
        print(f"{n:2d}  {path}:{line}  {name}")
    print(f"\nTotal >{threshold} params: {len(hot)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
