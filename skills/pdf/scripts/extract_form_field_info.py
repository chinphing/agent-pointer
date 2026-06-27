import json
import sys

import pymupdf

from pdf_form_common import get_field_info


def write_field_info(pdf_path: str, json_output_path: str) -> None:
    doc = pymupdf.open(pdf_path)
    try:
        field_info = get_field_info(doc)
    finally:
        doc.close()
    with open(json_output_path, "w", encoding="utf-8") as f:
        json.dump(field_info, f, indent=2)
    print(f"Wrote {len(field_info)} fields to {json_output_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: extract_form_field_info.py [input pdf] [output json]")
        sys.exit(1)
    write_field_info(sys.argv[1], sys.argv[2])
