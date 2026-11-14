#!/usr/bin/env python3
"""Strip Windows 8+ entry points from the Windows 7 client build.

Why this is needed
------------------
`x86_64-win7-windows-msvc` gives us a std that only needs Windows 7 APIs, but
third-party crates still statically import a few Windows 8+ entry points. The
Windows loader resolves *every* static import before the process starts, so the
client aborts with "Entry Point Not Found" on Windows 7 even though the missing
functions are never called.

Why not fix it in source
------------------------
Defining the symbol locally does not help: MSVC import libraries group every
import of one DLL into a single member (`windows.0.53.0.lib(advapi32.dll)`), so
as soon as any advapi32 import is needed the whole member is linked and
`EventSetInformation` comes along. The only remaining options are forking the
`windows` crate or rewriting the import name, and the rewrite is far smaller.

How it works
------------
The PE import table stores names as NUL-terminated strings in place, so a
shorter name followed by NUL padding is byte-length neutral and needs no RVA
fixups. Nothing in the program calls the original entry point, so the
substitute is never executed — it only has to resolve.

The script fails loudly if a name it is supposed to remove survives, so a
toolchain change cannot silently ship a binary that refuses to start.

Usage
-----
    python3 scripts/patch-win7-imports.py <path-to-exe>
"""

from __future__ import annotations

import sys

# (missing on Windows 7, Windows 7 replacement exported by the same DLL)
#
# EventSetInformation is a Windows 8 ETW call. Nothing in the dependency tree
# calls it; it rides along on the advapi32 import member.
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

    for missing, _ in REPLACEMENTS:
        if missing.encode() in data:
            raise SystemExit(f"{missing} survived the patch")

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
