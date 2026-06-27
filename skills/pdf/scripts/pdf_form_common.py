"""Shared PyMuPDF helpers for pdf skill form scripts."""

from __future__ import annotations

import pymupdf


def pymupdf_rect_to_pdf(rect: pymupdf.Rect, page_height: float) -> list[float]:
    """PDF /Rect style [left, bottom, right, top], origin at page bottom."""
    return [rect.x0, page_height - rect.y1, rect.x1, page_height - rect.y0]


def pdf_rect_to_pymupdf(
    left: float, bottom: float, right: float, top: float, page_height: float
) -> pymupdf.Rect:
    return pymupdf.Rect(left, page_height - top, right, page_height - bottom)


def image_bbox_to_pymupdf(
    bbox: list[float],
    image_width: float,
    image_height: float,
    pdf_width: float,
    pdf_height: float,
) -> pymupdf.Rect:
    x_scale = pdf_width / image_width
    y_scale = pdf_height / image_height
    left = bbox[0] * x_scale
    right = bbox[2] * x_scale
    top = bbox[1] * y_scale
    bottom = bbox[3] * y_scale
    return pymupdf.Rect(left, top, right, bottom)


def _checkbox_states(widget: pymupdf.Widget) -> tuple[str, str]:
    states = widget.button_states() or {}
    normal = states.get("normal") or states.get("down") or []
    off = "/Off"
    checked = None
    for s in normal:
        if s != off:
            checked = s
            break
    if checked is None and len(normal) == 2:
        checked = normal[0] if normal[1] == off else normal[1]
    if checked is None:
        checked = widget.on_state() or normal[0] if normal else "Yes"
    return checked, off


def make_field_dict(widget: pymupdf.Widget, field_id: str) -> dict:
    field_dict: dict = {"field_id": field_id}
    ft = widget.field_type
    if ft == pymupdf.PDF_WIDGET_TYPE_TEXT:
        field_dict["type"] = "text"
    elif ft == pymupdf.PDF_WIDGET_TYPE_CHECKBOX:
        field_dict["type"] = "checkbox"
        checked, unchecked = _checkbox_states(widget)
        field_dict["checked_value"] = checked
        field_dict["unchecked_value"] = unchecked
    elif ft in (pymupdf.PDF_WIDGET_TYPE_LISTBOX, pymupdf.PDF_WIDGET_TYPE_COMBOBOX):
        field_dict["type"] = "choice"
        values = widget.choice_values or []
        field_dict["choice_options"] = [{"value": v, "text": v} for v in values]
    elif ft == pymupdf.PDF_WIDGET_TYPE_RADIOBUTTON:
        field_dict["type"] = "radio_button"
        field_dict["radio_value"] = widget.on_state() or widget.field_value
    else:
        field_dict["type"] = widget.field_type_string or f"unknown ({ft})"
    return field_dict


def get_field_info(doc: pymupdf.Document) -> list[dict]:
    field_info_by_id: dict[str, dict] = {}
    radio_fields_by_id: dict[str, dict] = {}

    for page_index, page in enumerate(doc):
        page_h = page.rect.height
        for widget in page.widgets():
            field_id = widget.field_name
            if not field_id:
                continue
            rect = pymupdf_rect_to_pdf(widget.rect, page_h)
            ft = widget.field_type

            if ft == pymupdf.PDF_WIDGET_TYPE_RADIOBUTTON:
                if field_id not in radio_fields_by_id:
                    radio_fields_by_id[field_id] = {
                        "field_id": field_id,
                        "type": "radio_group",
                        "page": page_index + 1,
                        "radio_options": [],
                    }
                on_val = widget.on_state() or widget.field_value
                if on_val:
                    radio_fields_by_id[field_id]["radio_options"].append(
                        {"value": on_val, "rect": rect}
                    )
                continue

            info = make_field_dict(widget, field_id)
            info["page"] = page_index + 1
            info["rect"] = rect
            field_info_by_id[field_id] = info

    fields_with_location = list(field_info_by_id.values())
    sorted_fields = fields_with_location + list(radio_fields_by_id.values())

    def sort_key(f: dict) -> list:
        if f.get("type") == "radio_group" and f.get("radio_options"):
            rect = f["radio_options"][0]["rect"]
        else:
            rect = f.get("rect") or [0, 0, 0, 0]
        return [f.get("page"), [-rect[1], rect[0]]]

    sorted_fields.sort(key=sort_key)
    return sorted_fields


def validation_error_for_field_value(field_info: dict, field_value) -> str | None:
    field_type = field_info["type"]
    field_id = field_info["field_id"]
    if field_type == "checkbox":
        checked_val = field_info["checked_value"]
        unchecked_val = field_info["unchecked_value"]
        if field_value != checked_val and field_value != unchecked_val:
            return (
                f'ERROR: Invalid value "{field_value}" for checkbox field "{field_id}". '
                f'The checked value is "{checked_val}" and the unchecked value is "{unchecked_val}"'
            )
    elif field_type == "radio_group":
        option_values = [opt["value"] for opt in field_info["radio_options"]]
        if field_value not in option_values:
            return (
                f'ERROR: Invalid value "{field_value}" for radio group field "{field_id}". '
                f"Valid values are: {option_values}"
            )
    elif field_type == "choice":
        choice_values = [opt["value"] for opt in field_info["choice_options"]]
        if field_value not in choice_values:
            return (
                f'ERROR: Invalid value "{field_value}" for choice field "{field_id}". '
                f"Valid values are: {choice_values}"
            )
    return None


def apply_field_values(doc: pymupdf.Document, fields: list[dict]) -> None:
    """Set widget values from field_values.json entries (must be pre-validated)."""
    by_page: dict[int, dict[str, object]] = {}
    for field in fields:
        if "value" not in field:
            continue
        page = field["page"]
        by_page.setdefault(page, {})[field["field_id"]] = field["value"]

    for page_num, values in by_page.items():
        page = doc[page_num - 1]
        for widget in page.widgets():
            name = widget.field_name
            if name not in values:
                continue
            value = values[name]
            if widget.field_type == pymupdf.PDF_WIDGET_TYPE_CHECKBOX:
                if value in (False, "/Off", "Off", "off", None, ""):
                    widget.field_value = False
                else:
                    widget.field_value = value
            else:
                widget.field_value = value
            widget.update()
