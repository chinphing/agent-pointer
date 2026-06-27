#!/usr/bin/env python3
"""Rebuild a PDF with every page as A4 portrait; content long edge vertical."""

from __future__ import annotations

import argparse
import sys

import pymupdf

A4 = pymupdf.paper_rect("a4")


def rotation_for_a4_portrait(page: pymupdf.Page) -> float:
    """Return show_pdf_page rotation so content's long edge is vertical on A4 portrait.

    Portrait output means the **long edge runs vertically** on the sheet (~595×842 pt).
    If the source page has its **long edge horizontal** (width > height), including
    A4 landscape (842×595), add 90° so content reads upright on A4 portrait.
    """
    r = page.rect
    undo = -page.rotation
    if r.width > r.height:
        return undo + 90
    return undo


def standardize_to_a4_portrait(
    src_path: str, out_path: str, margin_pt: float = 0
) -> None:
    src = pymupdf.open(src_path)
    dst = pymupdf.open()
    try:
        target = A4
        if margin_pt:
            m = float(margin_pt)
            target = pymupdf.Rect(m, m, A4.width - m, A4.height - m)
        for i in range(src.page_count):
            sp = src[i]
            page = dst.new_page(width=A4.width, height=A4.height)
            page.show_pdf_page(
                target,
                src,
                i,
                keep_proportion=True,
                rotate=rotation_for_a4_portrait(sp),
            )
        dst.save(out_path, garbage=4, deflate=True)
    finally:
        dst.close()
        src.close()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Standardize all pages to A4 portrait (long edge vertical)."
    )
    parser.add_argument("input_pdf")
    parser.add_argument("output_pdf")
    parser.add_argument(
        "--margin",
        type=float,
        default=0,
        help="Inset margin in points (e.g. 36 for ~0.5 inch)",
    )
    args = parser.parse_args()
    standardize_to_a4_portrait(args.input_pdf, args.output_pdf, args.margin)
    print(f"Saved A4 portrait PDF to {args.output_pdf}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
