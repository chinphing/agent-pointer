"""
Extract form structure from a non-fillable PDF.

Finds text labels, horizontal lines, and small square rects (checkboxes).
Uses PyMuPDF only.

Usage: python extract_form_structure.py <input.pdf> <output.json>
"""

from __future__ import annotations

import json
import sys

import pymupdf


def _point_xy(p) -> tuple[float, float]:
    if hasattr(p, "x"):
        return float(p.x), float(p.y)
    return float(p[0]), float(p[1])


def _rect_bounds(r) -> tuple[float, float, float, float]:
    if hasattr(r, "x0"):
        return float(r.x0), float(r.y0), float(r.x1), float(r.y1)
    return float(r[0]), float(r[1]), float(r[2]), float(r[3])


def extract_form_structure(pdf_path: str) -> dict:
    structure: dict = {
        "pages": [],
        "labels": [],
        "lines": [],
        "checkboxes": [],
        "row_boundaries": [],
    }

    doc = pymupdf.open(pdf_path)
    try:
        for page_num, page in enumerate(doc, 1):
            w, h = float(page.rect.width), float(page.rect.height)
            structure["pages"].append(
                {"page_number": page_num, "width": w, "height": h}
            )

            for x0, y0, x1, y1, text, *_rest in page.get_text("words", sort=True):
                structure["labels"].append(
                    {
                        "page": page_num,
                        "text": text,
                        "x0": round(x0, 1),
                        "top": round(y0, 1),
                        "x1": round(x1, 1),
                        "bottom": round(y1, 1),
                    }
                )

            for path in page.get_drawings():
                for item in path.get("items", []):
                    kind = item[0]
                    if kind == "l" and len(item) >= 3:
                        x0, y0 = _point_xy(item[1])
                        x1, y1 = _point_xy(item[2])
                        if abs(x1 - x0) > w * 0.5:
                            structure["lines"].append(
                                {
                                    "page": page_num,
                                    "y": round(y0, 1),
                                    "x0": round(x0, 1),
                                    "x1": round(x1, 1),
                                }
                            )
                    elif kind == "re" and len(item) >= 2:
                        rx0, ry0, rx1, ry1 = _rect_bounds(item[1])
                        width = rx1 - rx0
                        height = ry1 - ry0
                        if 5 <= width <= 15 and 5 <= height <= 15 and abs(width - height) < 2:
                            structure["checkboxes"].append(
                                {
                                    "page": page_num,
                                    "x0": round(rx0, 1),
                                    "top": round(ry0, 1),
                                    "x1": round(rx1, 1),
                                    "bottom": round(ry1, 1),
                                    "center_x": round((rx0 + rx1) / 2, 1),
                                    "center_y": round((ry0 + ry1) / 2, 1),
                                }
                            )
    finally:
        doc.close()

    lines_by_page: dict[int, list[float]] = {}
    for line in structure["lines"]:
        lines_by_page.setdefault(line["page"], []).append(line["y"])

    for page, y_coords in lines_by_page.items():
        y_coords = sorted(set(y_coords))
        for i in range(len(y_coords) - 1):
            structure["row_boundaries"].append(
                {
                    "page": page,
                    "row_top": y_coords[i],
                    "row_bottom": y_coords[i + 1],
                    "row_height": round(y_coords[i + 1] - y_coords[i], 1),
                }
            )

    return structure


def main() -> None:
    if len(sys.argv) != 3:
        print("Usage: extract_form_structure.py <input.pdf> <output.json>")
        sys.exit(1)

    pdf_path = sys.argv[1]
    output_path = sys.argv[2]

    print(f"Extracting structure from {pdf_path}...")
    structure = extract_form_structure(pdf_path)

    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(structure, f, indent=2)

    print("Found:")
    print(f"  - {len(structure['pages'])} pages")
    print(f"  - {len(structure['labels'])} text labels")
    print(f"  - {len(structure['lines'])} horizontal lines")
    print(f"  - {len(structure['checkboxes'])} checkboxes")
    print(f"  - {len(structure['row_boundaries'])} row boundaries")
    print(f"Saved to {output_path}")


if __name__ == "__main__":
    main()
