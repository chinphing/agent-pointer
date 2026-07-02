---
name: pdf
description: Use this skill whenever the user wants to do anything with PDF files. This includes reading or extracting text/tables from PDFs, combining or merging multiple PDFs into one, splitting PDFs apart, rotating pages, standardizing page size (e.g. A4 portrait), adding watermarks, creating new PDFs, filling PDF forms, encrypting/decrypting PDFs, extracting images, and OCR on scanned PDFs to make them searchable. If the user mentions a .pdf file or asks to produce one, use this skill.
license: Proprietary. LICENSE.txt has complete terms
---

# PDF Processing Guide

## Overview

All PDF work in **`terminal`** uses **PyMuPDF** only (install **`pymupdf`** if missing — see **Dependencies**).

Official import (recommended since 1.24):

```python
import pymupdf
```

**About the name `fitz`:** older PyMuPDF versions imported as `fitz` (MuPDF history). Today **`import pymupdf` is official**; `import fitz` still works as a **legacy alias** for old scripts. Do **not** use `import fitz` in new code — a separate unrelated PyPI package named `fitz` can break it. Do **not** `pip install fitz`.

Official docs: **https://pymupdf.readthedocs.io/** — tutorial, classes (`Matrix`, `Pixmap`, `Rect`, …), and edge cases.

### Main API entry points

All types live on the **`pymupdf`** module (same binding; no separate “fitz library”):

| API | Typical use |
|-----|-------------|
| `pymupdf.open`, `Document`, `Page` | Open, save, merge, split, metadata |
| `pymupdf.Matrix`, `Rect`, `Point`, `IRect` | Zoom, clip, coordinates, overlays |
| `pymupdf.Pixmap`, `page.get_pixmap()` | Render pages, decode embedded images |
| `pymupdf.Shape`, `pymupdf.TextWriter` | Vector drawing, styled text on new PDFs |
| Page annot methods | Redactions, free text on flat forms |
| `pymupdf.PDF_WIDGET_TYPE_*`, `page.widgets()` | AcroForm fields |
| `pymupdf.PDF_ENCRYPT_*` | Encryption constants |
| `get_text(..., sort=True)`, `find_tables()` | Text / tables |

Prefer **`pymupdf.Rect` / `Point` / `Matrix`** over raw tuples when rendering, clipping, or placing content.

## Dependencies

**Python 3.12** preferred (3.9+ ok). Package: **`pymupdf`**. No Python/pip → **`skill_read`** **dev-env-setup** (see **references/python.md**).

**Do not reinstall every session.** Check first; install only when import fails:

```bash
python3 -c "import pymupdf" 2>/dev/null || pip install pymupdf -i https://mirrors.huaweicloud.com/repository/pypi/simple
```

On Windows, use `python` instead of `python3` if needed.

## Pointer: reading attached PDFs

**Default path — this skill first.** Do **not** call `media_understand` until you have tried text extraction here.

When the user **attaches a PDF** and wants content read, summarized, or transcribed:

1. **`skill_read`** this skill, then **`terminal`** using **`localPath`** from the attachment manifest (not `pointer-media://`).
2. Extract text with **PyMuPDF** — default **`sort=True`** (see **Text extraction**).
3. If extraction yields usable text, answer from that text (summarize, quote, table-parse, etc.) in your reply.

### When the PDF is scanned (then use `media_understand`)

Call **`media_understand`** with **`mode=pdf`**, **`refs`** (one attachment ref), and a clear **`goal`** **only after** PyMuPDF text extraction shows the file is **image-based / scanned**, for example:

- Per-page text is **empty**, or only page numbers / watermarks (&lt; ~48 meaningful characters per page).
- Text is **garbled** (CID mojibake, high replacement-char ratio) even after retrying unsorted extraction.
- Full-page raster images dominate and text layer is absent on sampled pages.

Then:

1. Call **`media_understand`** with **`mode=pdf`**, **`refs`**, and **`goal`** (host: **Pdfium** renders each page to JPEG, then vision model).
2. **`pageStart` / `pageEnd`:** per **`media_understand`** tool schema (max **10** pages/call).

Do **not** use local Tesseract or other OCR CLIs for chat attachments when `media_understand` is available.

For merge, split, forms, creation, and editing — stay on this skill (`terminal` + PyMuPDF); use **`media_understand`** only for **reading scanned** attachments.

## Text extraction

**Default: `sort=True`** (reading order). Use **`sort=False`** only when the user asks for **raw block order**, **exact search offsets**, or sorted output is garbled.

```python
import pymupdf

doc = pymupdf.open("document.pdf")
for page in doc:
    text = page.get_text("text", sort=True)       # default
    blocks = page.get_text("blocks", sort=True)   # text + bounding boxes
    structured = page.get_text("dict", sort=True) # spans, fonts, positions
```

Other useful modes: **`"html"`**, **`"xhtml"`**, **`"json"`** — see PyMuPDF docs for `Page.get_text()`.

## Tables

```python
import pymupdf

doc = pymupdf.open("document.pdf")
for page in doc:
    for tab in page.find_tables():
        rows = tab.extract()  # list of rows; see docs for Table.to_pandas()
        print(rows)
```

## Quick Start

```python
import pymupdf

doc = pymupdf.open("document.pdf")
print(f"Pages: {doc.page_count}")
print(doc.metadata)

text = "".join(page.get_text("text", sort=True) for page in doc)
print(text[:500])
```

## Common operations

### Merge PDFs

Add one **bookmark per source file** (label = filename without `.pdf`):

```python
import os
import pymupdf

paths = ["doc1.pdf", "doc2.pdf", "doc3.pdf"]
out = pymupdf.open()
toc = []
page = 0
for path in paths:
    src = pymupdf.open(path)
    out.insert_pdf(src)
    name = os.path.basename(path)
    if name.lower().endswith(".pdf"):
        name = name[:-4]
    toc.append([1, name, page + 1])  # level, title, 1-based page
    page += src.page_count
    src.close()
out.set_toc(toc)
out.save("merged.pdf", garbage=4, deflate=True)
out.close()
```

### Split (one file per page)

```python
import pymupdf

src = pymupdf.open("input.pdf")
for i in range(src.page_count):
    single = pymupdf.open()
    single.insert_pdf(src, from_page=i, to_page=i)
    single.save(f"page_{i + 1}.pdf")
    single.close()
src.close()
```

### Extract page range

```python
import pymupdf

src = pymupdf.open("input.pdf")
dst = pymupdf.open()
dst.insert_pdf(src, from_page=0, to_page=4)  # pages 1–5
dst.save("pages1-5.pdf")
dst.close()
src.close()
```

### Rotate pages

```python
import pymupdf

doc = pymupdf.open("input.pdf")
doc[i].set_rotation(90)  # page i, clockwise degrees
doc.save("rotated.pdf")
doc.close()
```

### Standardize to A4 portrait

**Goal:** every output page is **A4 portrait** (~595×842 pt) and **content is shown with its long edge vertical** (竖版 = long side runs up–down on the sheet).

Content is **vector-copied** via `show_pdf_page` (`keep_proportion=True`), centered; leftover area stays blank.

**Rotation rule (per source page, use `page.rect`):**

| Source orientation | Example | Action |
|--------------------|---------|--------|
| Long edge **vertical** (`height ≥ width`) | A4 portrait 595×842, tall scan | Fit only; undo `/Rotate` with `-page.rotation` |
| Long edge **horizontal** (`width > height`) | A4 landscape 842×595, slides | **Rotate 90°** so content long edge becomes vertical on A4 portrait |

So an **A4-sized landscape page** (842×595) must rotate — same as any other landscape page.

```python
import pymupdf

A4 = pymupdf.paper_rect("a4")


def rotation_for_a4_portrait(page: pymupdf.Page) -> float:
    r = page.rect
    undo = -page.rotation
    if r.width > r.height:  # long edge horizontal → rotate for A4 portrait
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
```

CLI: `python scripts/standardize_a4_portrait.py input.pdf output.pdf [--margin 36]`

- **Mixed sizes** (scan, slides, letter): same pipeline — uniform A4 portrait output.
- Other paper sizes: **`pymupdf.paper_sizes()`**; default target is **`a4`** portrait unless the user specifies otherwise.

### Metadata

```python
import pymupdf

doc = pymupdf.open("document.pdf")
print(doc.metadata)
doc.set_metadata({"title": "New title", "author": "Name"})
doc.save("updated.pdf", incremental=True, encryption=0)
doc.close()
```

### Extract embedded images (`Pixmap`)

```python
import pymupdf

doc = pymupdf.open("input.pdf")
for pno, page in enumerate(doc):
    for img in page.get_images():
        xref = img[0]
        pix = pymupdf.Pixmap(doc, xref)
        if pix.n - pix.alpha > 3:  # CMYK → RGB; see Pixmap docs
            pix = pymupdf.Pixmap(pymupdf.csRGB, pix)
        pix.save(f"page{pno + 1}_xref{xref}.png")
        pix = None
doc.close()
```

### Render page to image (`Matrix` + `Pixmap`)

```python
import pymupdf

doc = pymupdf.open("input.pdf")
page = doc[0]
zoom = pymupdf.Matrix(2, 2)           # 2× scale
clip = pymupdf.Rect(0, 0, 400, 400)   # optional crop; omit for full page
pix = page.get_pixmap(matrix=zoom, clip=clip, alpha=False)
pix.save("page1.png")
doc.close()
```

### Create a new PDF

Simple text/lines — `Page.insert_text` / `draw_line`. For styled runs or paths, use **`pymupdf.TextWriter`** or **`pymupdf.Shape`** (see docs).

```python
import pymupdf

doc = pymupdf.open()
page = doc.new_page(width=595, height=842)  # A4 pt
page.insert_text(pymupdf.Point(72, 72), "Hello World", fontsize=12)
page.draw_line(pymupdf.Point(72, 100), pymupdf.Point(400, 100))
doc.save("hello.pdf")
doc.close()
```

### Watermark / overlay

```python
import pymupdf

doc = pymupdf.open("document.pdf")
wm = pymupdf.open("watermark.pdf")
for page in doc:
    page.show_pdf_page(page.rect, wm, 0, overlay=True)
doc.save("watermarked.pdf")
doc.close()
wm.close()
```

### Password protection

```python
import pymupdf

doc = pymupdf.open("input.pdf")
doc.save(
    "encrypted.pdf",
    encryption=pymupdf.PDF_ENCRYPT_AES_256,
    user_pw="user",
    owner_pw="owner",
)
doc.close()
```

Open encrypted files: `pymupdf.open("file.pdf", password="user")`.

### Fill PDF forms (AcroForm widgets)

```python
import pymupdf

doc = pymupdf.open("form.pdf")
for page in doc:
    for field in page.widgets():
        if field.field_type == pymupdf.PDF_WIDGET_TYPE_TEXT:
            field.field_value = "Example"
            field.update()
doc.save("filled.pdf")
doc.close()
```

Inspect fields with `page.widgets()`; field types, checkboxes, and radio groups — see **Widget** in the PyMuPDF docs.

For **non-fillable (flat) forms**, use annotations:

- `page.search_for(text)` + `page.add_redact_annot(rect)` + `page.apply_redactions()`
- or `page.add_freetext_annot(pymupdf.Rect(...), "...")` for overlay text

See **Annot** / **Redact** in the PyMuPDF docs.

## Bundled form scripts (`scripts/`)

All scripts use **PyMuPDF** only (shared helpers in `scripts/pdf_form_common.py`). Run from the **pdf** skill directory:

| Script | Purpose |
|--------|---------|
| `check_fillable_fields.py` | Detect AcroForm widgets |
| `extract_form_field_info.py` | Export fillable field JSON |
| `fill_fillable_fields.py` | Fill from `field_values.json` |
| `extract_form_structure.py` | Labels/lines/checkboxes on flat forms |
| `convert_pdf_to_images.py` | Page PNGs for visual form mapping |
| `fill_pdf_form_with_annotations.py` | Free-text annots on flat forms |
| `standardize_a4_portrait.py` | A4 portrait; rotate landscape pages 90° |
| `check_bounding_boxes.py` | Validate flat-form bbox JSON |
| `create_validation_image.py` | Draw bboxes on PNG (Pillow) |

Form workflow details: **forms.md**.

## Scanned PDFs (attachments)

After PyMuPDF extraction is **empty or unusable** (see **Pointer: reading attached PDFs**):

1. Call **`media_understand`** with **`mode=pdf`**, the attachment in **`refs`**, and a **`goal`**.
2. **`pageStart` / `pageEnd`:** per **`media_understand`** tool schema (max **10** pages/call).

## Quick reference

| Task | Approach |
|------|----------|
| Extract text (attached PDF) | `pymupdf.open` + `get_text("text", sort=True)` |
| Extract tables | `page.find_tables()` → `.extract()` |
| Read scanned attachment | **`media_understand` `mode=pdf`** (after empty/garbled text) |
| Merge / split / rotate | `insert_pdf`, `set_toc`, `set_rotation` |
| Standardize A4 portrait | `standardize_a4_portrait.py` or `show_pdf_page` + rotate when `width > height` |
| Images / render pages | **`pymupdf.Pixmap`**, **`pymupdf.Matrix`**, `get_pixmap()` |
| Create PDF | `new_page()`, **`pymupdf.Point`**, `TextWriter` / `Shape` |
| Forms (fillable) | `page.widgets()`, **`pymupdf.PDF_WIDGET_TYPE_*`** |
| Forms (flat) | **`pymupdf.Rect`**, redact / freetext annots |
| Encrypt / decrypt | **`pymupdf.PDF_ENCRYPT_*`**, `open(..., password=...)` |
| Advanced API | **https://pymupdf.readthedocs.io/** |
