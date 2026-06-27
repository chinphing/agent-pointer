import os
import sys

import pymupdf


def convert(pdf_path: str, output_dir: str, max_dim: int = 1000, dpi: int = 200) -> None:
    zoom = dpi / 72.0
    doc = pymupdf.open(pdf_path)
    try:
        for i, page in enumerate(doc):
            pix = page.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom), alpha=False)
            if max(pix.width, pix.height) > max_dim:
                scale = max_dim / max(pix.width, pix.height)
                pix = pymupdf.Pixmap(
                    pix, int(pix.width * scale), int(pix.height * scale)
                )
            image_path = os.path.join(output_dir, f"page_{i + 1}.png")
            pix.save(image_path)
            print(f"Saved page {i + 1} as {image_path} (size: {pix.width}x{pix.height})")
        print(f"Converted {doc.page_count} pages to PNG images")
    finally:
        doc.close()


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: convert_pdf_to_images.py [input pdf] [output directory]")
        sys.exit(1)
    os.makedirs(sys.argv[2], exist_ok=True)
    convert(sys.argv[1], sys.argv[2])
