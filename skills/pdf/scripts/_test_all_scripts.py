#!/usr/bin/env python3
"""Smoke tests for pdf skill scripts (PyMuPDF). Run from scripts/ directory."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile

import pymupdf

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PY = sys.executable


def run(cmd: list[str], cwd: str | None = None) -> tuple[int, str]:
    r = subprocess.run(
        cmd,
        cwd=cwd or SCRIPT_DIR,
        capture_output=True,
        text=True,
    )
    out = (r.stdout or "") + (r.stderr or "")
    return r.returncode, out.strip()


def make_text_pdf(path: str) -> None:
    doc = pymupdf.open()
    page = doc.new_page(width=595, height=842)
    page.insert_text((72, 72), "Hello PDF skill test", fontsize=14)
    page.draw_line(pymupdf.Point(72, 100), pymupdf.Point(400, 100))
    doc.save(path)
    doc.close()


def make_fillable_pdf(path: str) -> None:
    doc = pymupdf.open()
    page = doc.new_page(width=595, height=842)
    widget = pymupdf.Widget()
    widget.field_type = pymupdf.PDF_WIDGET_TYPE_TEXT
    widget.field_name = "last_name"
    widget.field_label = "Last name"
    widget.rect = pymupdf.Rect(100, 100, 300, 130)
    widget.field_value = ""
    page.add_widget(widget)

    cb = pymupdf.Widget()
    cb.field_type = pymupdf.PDF_WIDGET_TYPE_CHECKBOX
    cb.field_name = "agree"
    cb.rect = pymupdf.Rect(100, 150, 120, 170)
    page.add_widget(cb)

    doc.save(path)
    doc.close()


def make_flat_pdf(path: str) -> None:
    doc = pymupdf.open()
    page = doc.new_page(width=595, height=842)
    page.insert_text((72, 72), "Name:", fontsize=12)
    page.draw_line(pymupdf.Point(72, 200), pymupdf.Point(520, 200))
    shape = page.new_shape()
    shape.draw_rect(pymupdf.Rect(400, 300, 412, 312))
    shape.finish(color=(0, 0, 0), fill=(1, 1, 1))
    shape.commit()
    doc.save(path)
    doc.close()


def ok(name: str) -> None:
    print(f"  PASS  {name}")


def fail(name: str, detail: str) -> None:
    print(f"  FAIL  {name}")
    print(f"        {detail[:800]}")


def main() -> int:
    os.chdir(SCRIPT_DIR)
    tmp = tempfile.mkdtemp(prefix="pdf_skill_test_")
    errors = 0

    text_pdf = os.path.join(tmp, "text.pdf")
    fillable_pdf = os.path.join(tmp, "fillable.pdf")
    flat_pdf = os.path.join(tmp, "flat.pdf")
    img_dir = os.path.join(tmp, "pages")
    os.makedirs(img_dir, exist_ok=True)

    make_text_pdf(text_pdf)
    make_fillable_pdf(fillable_pdf)
    make_flat_pdf(flat_pdf)

    print(f"Test artifacts: {tmp}\n")

    # check_fillable_fields
    code, out = run([PY, "check_fillable_fields.py", text_pdf])
    if code == 0 and "does not have fillable" in out:
        ok("check_fillable_fields (no fields)")
    else:
        fail("check_fillable_fields (no fields)", out)
        errors += 1

    code, out = run([PY, "check_fillable_fields.py", fillable_pdf])
    if code == 0 and "has fillable" in out:
        ok("check_fillable_fields (has fields)")
    else:
        fail("check_fillable_fields (has fields)", out)
        errors += 1

    # extract_form_field_info
    field_info_path = os.path.join(tmp, "field_info.json")
    code, out = run([PY, "extract_form_field_info.py", fillable_pdf, field_info_path])
    if code == 0 and os.path.isfile(field_info_path):
        with open(field_info_path, encoding="utf-8") as f:
            info = json.load(f)
        ids = {x["field_id"] for x in info}
        if "last_name" in ids:
            ok("extract_form_field_info")
        else:
            fail("extract_form_field_info", f"missing last_name in {ids}")
            errors += 1
    else:
        fail("extract_form_field_info", out)
        errors += 1

    # fill_fillable_fields
    values_path = os.path.join(tmp, "field_values.json")
    filled_path = os.path.join(tmp, "filled.pdf")
    with open(field_info_path, encoding="utf-8") as f:
        info = json.load(f)
    last = next(x for x in info if x["field_id"] == "last_name")
    agree = next(x for x in info if x["field_id"] == "agree")
    values = [
        {"field_id": "last_name", "description": "last", "page": last["page"], "value": "Smith"},
        {
            "field_id": "agree",
            "description": "agree",
            "page": agree["page"],
            "value": agree["checked_value"],
        },
    ]
    with open(values_path, "w", encoding="utf-8") as f:
        json.dump(values, f)

    code, out = run([PY, "fill_fillable_fields.py", fillable_pdf, values_path, filled_path])
    if code == 0 and os.path.isfile(filled_path):
        doc = pymupdf.open(filled_path)
        found = False
        for page in doc:
            for w in page.widgets():
                if w.field_name == "last_name" and w.field_value == "Smith":
                    found = True
        doc.close()
        if found:
            ok("fill_fillable_fields")
        else:
            fail("fill_fillable_fields", "last_name not Smith after fill")
            errors += 1
    else:
        fail("fill_fillable_fields", out)
        errors += 1

    # extract_form_structure
    structure_path = os.path.join(tmp, "structure.json")
    code, out = run([PY, "extract_form_structure.py", flat_pdf, structure_path])
    if code == 0 and os.path.isfile(structure_path):
        with open(structure_path, encoding="utf-8") as f:
            st = json.load(f)
        if st.get("labels") and st.get("pages"):
            ok("extract_form_structure")
        else:
            fail("extract_form_structure", json.dumps(st)[:400])
            errors += 1
    else:
        fail("extract_form_structure", out)
        errors += 1

    # convert_pdf_to_images
    code, out = run([PY, "convert_pdf_to_images.py", text_pdf, img_dir])
    png = os.path.join(img_dir, "page_1.png")
    if code == 0 and os.path.isfile(png) and os.path.getsize(png) > 100:
        ok("convert_pdf_to_images")
    else:
        fail("convert_pdf_to_images", out)
        errors += 1

    # fill_pdf_form_with_annotations + check_bounding_boxes + create_validation_image
    fields_json = os.path.join(tmp, "annot_fields.json")
    annot_out = os.path.join(tmp, "annotated.pdf")
    with open(png, "rb") as f:
        from PIL import Image

        im = Image.open(f)
        iw, ih = im.size

    payload = {
        "pages": [{"page_number": 1, "image_width": iw, "image_height": ih}],
        "form_fields": [
            {
                "page_number": 1,
                "description": "name entry",
                "label_bounding_box": [50, 50, 120, 80],
                "entry_bounding_box": [130, 50, 350, 80],
                "entry_text": {"text": "Test User", "font": "Arial", "font_size": 12},
            }
        ],
    }
    with open(fields_json, "w", encoding="utf-8") as f:
        json.dump(payload, f)

    code, out = run([PY, "check_bounding_boxes.py", fields_json])
    if code == 0 and "SUCCESS" in out:
        ok("check_bounding_boxes")
    else:
        fail("check_bounding_boxes", out)
        errors += 1

    val_img = os.path.join(tmp, "validation.png")
    code, out = run(
        [PY, "create_validation_image.py", "1", fields_json, png, val_img]
    )
    if code == 0 and os.path.isfile(val_img):
        ok("create_validation_image")
    else:
        fail("create_validation_image", out)
        errors += 1

    code, out = run(
        [PY, "fill_pdf_form_with_annotations.py", text_pdf, fields_json, annot_out]
    )
    if code == 0 and os.path.isfile(annot_out):
        doc = pymupdf.open(annot_out)
        ann = list(doc[0].annots())
        doc.close()
        if ann:
            ok("fill_pdf_form_with_annotations")
        else:
            fail("fill_pdf_form_with_annotations", "no annotations on output")
            errors += 1
    else:
        fail("fill_pdf_form_with_annotations", out)
        errors += 1

    # merge with bookmarks (SKILL.md pattern)
    merge_a = os.path.join(tmp, "merge_a.pdf")
    merge_b = os.path.join(tmp, "merge_b.pdf")
    doc = pymupdf.open()
    doc.new_page(width=595, height=842).insert_text((72, 72), "A", fontsize=14)
    doc.save(merge_a)
    doc.close()
    doc = pymupdf.open()
    doc.new_page(width=595, height=842).insert_text((72, 72), "B", fontsize=14)
    doc.new_page(width=595, height=842).insert_text((72, 72), "B2", fontsize=14)
    doc.save(merge_b)
    doc.close()

    merged_path = os.path.join(tmp, "merged.pdf")
    out = pymupdf.open()
    toc: list[list] = []
    page = 0
    for path in [merge_a, merge_b]:
        src = pymupdf.open(path)
        out.insert_pdf(src)
        name = os.path.basename(path)
        if name.lower().endswith(".pdf"):
            name = name[:-4]
        toc.append([1, name, page + 1])
        page += src.page_count
        src.close()
    out.set_toc(toc)
    out.save(merged_path, garbage=4, deflate=True)
    out.close()

    d = pymupdf.open(merged_path)
    pages_ok = d.page_count == 3
    titles = [t[1] for t in d.get_toc()]
    d.close()
    if pages_ok and titles == ["merge_a", "merge_b"]:
        ok("merge PDFs (bookmarks)")
    else:
        fail("merge PDFs (bookmarks)", f"pages={pages_ok} titles={titles}")
        errors += 1

    # standardize_a4_portrait — landscape A4 (842×595) and portrait A4
    land_a4 = os.path.join(tmp, "a4_landscape.pdf")
    port_a4 = os.path.join(tmp, "a4_portrait_src.pdf")
    doc = pymupdf.open()
    doc.new_page(width=842, height=595).insert_text((72, 72), "land", fontsize=14)
    doc.save(land_a4)
    doc.close()
    doc = pymupdf.open()
    doc.new_page(width=595, height=842).insert_text((72, 72), "port", fontsize=14)
    doc.save(port_a4)
    doc.close()

    land_out = os.path.join(tmp, "a4_landscape_out.pdf")
    code, out = run([PY, "standardize_a4_portrait.py", land_a4, land_out])
    if code == 0 and os.path.isfile(land_out):
        d = pymupdf.open(land_out)
        r = d[0].rect
        d.close()
        if abs(r.width - 595) < 2 and abs(r.height - 842) < 2:
            ok("standardize_a4_portrait (landscape A4 source)")
        else:
            fail("standardize_a4_portrait (landscape)", f"bad page size {r}")
            errors += 1
    else:
        fail("standardize_a4_portrait (landscape)", out)
        errors += 1

    port_out = os.path.join(tmp, "a4_portrait_out.pdf")
    code, out = run([PY, "standardize_a4_portrait.py", port_a4, port_out])
    if code == 0 and os.path.isfile(port_out):
        d = pymupdf.open(port_out)
        r = d[0].rect
        d.close()
        if abs(r.width - 595) < 2 and abs(r.height - 842) < 2:
            ok("standardize_a4_portrait (portrait A4 source)")
        else:
            fail("standardize_a4_portrait (portrait)", f"bad page size {r}")
            errors += 1
    else:
        fail("standardize_a4_portrait (portrait)", out)
        errors += 1

    print()
    if errors:
        print(f"{errors} test(s) failed")
        return 1
    print("All script smoke tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
