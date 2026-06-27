import json
import sys

import pymupdf

from pdf_form_common import image_bbox_to_pymupdf, pdf_rect_to_pymupdf


def _parse_color(hex_color: str) -> tuple[float, float, float]:
    h = hex_color.lstrip("#")
    if len(h) != 6:
        return (0.0, 0.0, 0.0)
    r = int(h[0:2], 16) / 255.0
    g = int(h[2:4], 16) / 255.0
    b = int(h[4:6], 16) / 255.0
    return (r, g, b)


def _font_name(name: str) -> str:
    lower = (name or "helv").lower()
    if lower in ("arial", "helvetica", "sans"):
        return "helv"
    if lower in ("times", "times new roman", "serif"):
        return "times"
    if lower in ("courier", "mono", "monospace"):
        return "cour"
    return "helv"


def fill_pdf_form(input_pdf_path: str, fields_json_path: str, output_pdf_path: str) -> None:
    with open(fields_json_path, encoding="utf-8") as f:
        fields_data = json.load(f)

    doc = pymupdf.open(input_pdf_path)
    count = 0
    try:
        for field in fields_data["form_fields"]:
            page_num = field["page_number"]
            page = doc[page_num - 1]
            page_h = page.rect.height
            page_info = next(
                p for p in fields_data["pages"] if p["page_number"] == page_num
            )

            bbox = field["entry_bounding_box"]
            if "pdf_width" in page_info:
                left, bottom, right, top = bbox[0], bbox[1], bbox[2], bbox[3]
                rect = pdf_rect_to_pymupdf(left, bottom, right, top, page_h)
            else:
                rect = image_bbox_to_pymupdf(
                    bbox,
                    float(page_info["image_width"]),
                    float(page_info["image_height"]),
                    float(page.rect.width),
                    page_h,
                )

            entry_text = field.get("entry_text") or {}
            text = entry_text.get("text") or ""
            if not text:
                continue

            font_size = float(entry_text.get("font_size", 14))
            fontname = _font_name(entry_text.get("font", "Arial"))
            text_color = _parse_color(entry_text.get("font_color", "000000"))

            page.add_freetext_annot(
                rect,
                text,
                fontsize=font_size,
                fontname=fontname,
                text_color=text_color,
                fill_color=None,
                border_color=None,
            )
            count += 1

        doc.save(output_pdf_path)
    finally:
        doc.close()

    print(f"Successfully filled PDF form and saved to {output_pdf_path}")
    print(f"Added {count} text annotations")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        print("Usage: fill_pdf_form_with_annotations.py [input pdf] [fields.json] [output pdf]")
        sys.exit(1)
    fill_pdf_form(sys.argv[1], sys.argv[2], sys.argv[3])
