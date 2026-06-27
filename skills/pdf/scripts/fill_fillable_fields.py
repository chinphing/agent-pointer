import json
import sys

import pymupdf

from pdf_form_common import apply_field_values, get_field_info, validation_error_for_field_value


def fill_pdf_fields(input_pdf_path: str, fields_json_path: str, output_pdf_path: str) -> None:
    with open(fields_json_path, encoding="utf-8") as f:
        fields = json.load(f)

    doc = pymupdf.open(input_pdf_path)
    try:
        field_info = get_field_info(doc)
        fields_by_ids = {f["field_id"]: f for f in field_info}

        has_error = False
        for field in fields:
            existing = fields_by_ids.get(field["field_id"])
            if not existing:
                has_error = True
                print(f"ERROR: `{field['field_id']}` is not a valid field ID")
            elif field["page"] != existing["page"]:
                has_error = True
                print(
                    f"ERROR: Incorrect page number for `{field['field_id']}` "
                    f"(got {field['page']}, expected {existing['page']})"
                )
            elif "value" in field:
                err = validation_error_for_field_value(existing, field["value"])
                if err:
                    print(err)
                    has_error = True

        if has_error:
            sys.exit(1)

        apply_field_values(doc, fields)
        doc.save(output_pdf_path)
    finally:
        doc.close()


if __name__ == "__main__":
    if len(sys.argv) != 4:
        print("Usage: fill_fillable_fields.py [input pdf] [field_values.json] [output pdf]")
        sys.exit(1)
    fill_pdf_fields(sys.argv[1], sys.argv[2], sys.argv[3])
