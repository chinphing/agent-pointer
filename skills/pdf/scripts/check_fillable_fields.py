import sys

import pymupdf


def has_fillable_fields(pdf_path: str) -> bool:
    doc = pymupdf.open(pdf_path)
    try:
        for page in doc:
            if page.first_widget:
                return True
        return False
    finally:
        doc.close()


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: check_fillable_fields.py <file.pdf>")
        sys.exit(1)
    if has_fillable_fields(sys.argv[1]):
        print("This PDF has fillable form fields")
    else:
        print(
            "This PDF does not have fillable form fields; "
            "you will need to visually determine where to enter data"
        )
