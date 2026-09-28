# Office skill dependencies (anthropics/skills)

The **docx** / **xlsx** / **pptx** / **pdf** skills are **not bundled** with Pointer.
Install them on demand from [anthropics/skills](https://github.com/anthropics/skills)
(MIT) — see the **find-skills** skill. This file only lists the runtimes those skills
need; install only what the loaded skill body asks for.

## Common

- **Python 3.12** preferred (3.9+ ok) — Office skill scripts
- **LibreOffice** (`soffice`) — unpack/pack, PDF export, formula recalc (xlsx)

## docx

- **pandoc** — text extraction
- **npm `docx`** — create new Word files (`npm install -g docx`)

## xlsx

- **LibreOffice** — `scripts/recalc.py` for formula recalculation

## pptx

- **markitdown** — `pip install "markitdown[pptx]"`
- **Pillow** — thumbnail grids (`pip install Pillow`)
- **pptxgenjs** — create from scratch (`npm install -g pptxgenjs`)

## pdf

- **pymupdf** — all PDF read/edit/merge/split/forms (see pdf skill); default **`sort=True`** for text
- **`media_understand` `mode=pdf`** — scanned-PDF fallback (Pdfium in-process render); use **pdf** skill + `terminal` first

## Verify (examples)

```bash
python3 --version
pandoc --version
soffice --version || libreoffice --version
python3 -c "import markitdown; print('markitdown ok')"
python3 -c "import pymupdf; print('pymupdf ok')"
```

Install Python packages only when the import check fails (e.g. `python3 -c "import pymupdf" || pip install pymupdf ...`).

## User attachments

Skill scripts expect a filesystem path. Use **localPath** from
`<!-- pointer-user-attachments -->`, not `pointer-media://` in shell commands.
