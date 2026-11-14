#!/usr/bin/env python3
"""Neutralise Windows 8+ imports in the Windows 7 client build.

Why this exists
---------------
Building against `x86_64-win7-windows-msvc` gives us a std that only needs
Windows 7 APIs, but third-party crates still drag in a few Windows 8+ entry
points as *static* imports. The Windows loader resolves those before any of our
code runs, so the client dies with "Entry Point Not Found" on Windows 7 even
though the missing functions are never actually called.

What it does
------------
Rewrites each offending import name to a Windows 7 function exported by the
same DLL. The PE import table stores names as NUL-terminated strings in place,
so a shorter name followed by NUL padding is byte-length neutral and needs no
RVA fixups. Because nothing in the program calls the original entry point, the
substitute is never executed — it only has to resolve.

Usage
-----
    python3 scripts/patch-win7-imports.py <path-to-exe>
"""

from __future__ import annotations

import sys

# (missing on Windows 7, Windows 7 replacement from the same DLL)
#
# EventSetInformation (advapi32) is a Windows 8 addition. It is pulled in by
# linker granularity from the `windows` crate's ETW module: no crate in the
# dependency tree calls it, the object containing it simply gets linked.
REPLACEMENTS: list[tuple[str, str]] = [
    ("EventSetInformation", "EventRegister"),
]


def patch(path: str) -> int:
    with open(path, "rb") as fh:
        data = fh.read()

    original_len = len(data)
    changed: list[str] = []

    for missing, replacement in REPLACEMENTS:
        needle = missing.encode() + b"\x00"
        if needle not in data:
            continue
        if len(replacement) >= len(missing):
            raise SystemExit(f"{replacement} is not shorter than {missing}")
        filler = b"\x00" * (len(needle) - len(replacement) - 1)
        data = data.replace(needle, replacement.encode() + b"\x00" + filler)
        changed.append(f"{missing} -> {replacement}")

    if len(data) != original_len:
        raise SystemExit("patch changed the file size, refusing to write")

    if not changed:
        print("patch-win7-imports: nothing to patch (already clean)")
        return 0

    with open(path, "wb") as fh:
        fh.write(data)
    for entry in changed:
        print(f"patch-win7-imports: {entry}")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} <path-to-exe>")
    raise SystemExit(patch(sys.argv[1]))
