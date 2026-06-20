# Office skill dependencies (anthropics/skills)

Bundled **docx** / **xlsx** / **pptx** skills come from
[anthropics/skills](https://github.com/anthropics/skills) (MIT). Install only what the
loaded skill body asks for.

## Common

- **Python 3.9+** — skill scripts under `scripts/`
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

## Verify (examples)

```bash
python3 --version
pandoc --version
soffice --version || libreoffice --version
python3 -c "import markitdown; print('markitdown ok')"
```

## User attachments

Skill scripts expect a filesystem path. Use **localPath** from
`<!-- pointer-user-attachments -->`, not `pointer-media://` in shell commands.
