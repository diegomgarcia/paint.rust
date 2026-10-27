#!/usr/bin/env python3
"""Paint.NET 5.1.12 UI actions for the pinned 1024x808 UTM window layout."""

from __future__ import annotations

import copy
import re
import time
from pathlib import Path
from typing import Any

from host_ui import capture_window, click, key, type_text, wait_stable_window
from utm import ReferenceCommandError

WINDOW_SIZE = (1024, 808)
FILE_MENU = (22, 87)
NEW_ITEM = (57, 115)
SAVE_AS_ITEM = (57, 241)
ADJUSTMENTS_MENU = (250, 87)
INVERT_COLORS_ITEM = (277, 306)


def _click_relative(point: tuple[int, int], semantic_target: str) -> dict[str, Any]:
    window = wait_stable_window(required_size=WINDOW_SIZE)
    absolute = (float(window["x"]) + point[0], float(window["y"]) + point[1])
    click(*absolute)
    return {
        "action": "click",
        "semantic_target": semantic_target,
        "window_relative_point": list(point),
        "host_global_point": list(absolute),
        "required_window_size": list(WINDOW_SIZE),
    }


def _capture(directory: Path, filename: str, stage: str) -> dict[str, Any]:
    evidence = capture_window(directory / filename, ocr=True)
    evidence["stage"] = stage
    return evidence


def _require_ocr(evidence: dict[str, Any], *patterns: str) -> None:
    text = evidence.get("ocr_text", "")
    for pattern in patterns:
        if re.search(pattern, text, flags=re.IGNORECASE) is None:
            raise ReferenceCommandError(
                f"OCR gate {pattern!r} failed at {evidence.get('stage')}; text={text[:2000]!r}"
            )


def new_default_document(directory: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    key("escape")
    time.sleep(0.7)
    start = _capture(directory, "00-start.png", "start")
    _require_ocr(start, r"Paint\.\s*NET")
    if re.search(r"Save\s+As|Save\s+Configuration|Pixel\s+size", start.get("ocr_text", ""), re.IGNORECASE):
        raise ReferenceCommandError("Paint.NET did not return to its main window after Escape")
    actions = [{"action": "key", "key": "Escape", "semantic_target": "close inherited menu or modal"}]
    actions.append(_click_relative(FILE_MENU, "File menu"))
    time.sleep(0.4)
    actions.append(_click_relative(NEW_ITEM, "File > New"))
    time.sleep(1.5)
    dialog = _capture(directory, "01-new-dialog.png", "new-dialog")
    _require_ocr(dialog, r"\bNew\b", r"Pixel\s+size")
    actions.append({"action": "key", "key": "Enter", "semantic_target": "accept default New dialog"})
    key("enter")
    time.sleep(2.5)
    created = _capture(directory, "02-new-document.png", "new-document")
    _require_ocr(created, r"Untitled", r"800\s*x\s*600", r"Background")
    return actions, [dialog, created]


def invert_colors(directory: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    actions = [_click_relative(ADJUSTMENTS_MENU, "Adjustments menu")]
    time.sleep(0.5)
    menu = _capture(directory, "03-adjustments-menu.png", "adjustments-menu")
    _require_ocr(menu, r"Invert\s+Colors")
    actions.append(_click_relative(INVERT_COLORS_ITEM, "Adjustments > Invert Colors"))
    time.sleep(2.5)
    inverted = _capture(directory, "04-inverted.png", "inverted")
    _require_ocr(inverted, r"Untitled")
    return actions, [menu, inverted]


def save_png_with_defaults(directory: Path, filename: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    if re.fullmatch(r"[a-z0-9][a-z0-9.-]{1,60}", filename) is None or not filename.endswith(".png"):
        raise ValueError("PNG filename must be 3..61 lowercase US-ASCII characters and end in .png")
    actions = [_click_relative(FILE_MENU, "File menu")]
    time.sleep(0.4)
    actions.append(_click_relative(SAVE_AS_ITEM, "File > Save As"))
    time.sleep(1.5)
    dialog = _capture(directory, "05-save-as.png", "save-as")
    _require_ocr(dialog, r"Save\s+As", r"Documents", r"PNG")
    actions.append({"action": "type_us_ascii", "text": filename, "semantic_target": "Save As filename"})
    type_text(filename)
    time.sleep(0.7)
    typed = _capture(directory, "06-save-as-path.png", "save-as-path")
    _require_ocr(typed, re.escape(filename))
    actions.append({"action": "key", "key": "Enter", "semantic_target": "submit Save As"})
    key("enter")
    time.sleep(1.5)
    configuration = _capture(directory, "07-png-configuration.png", "png-configuration")
    _require_ocr(configuration, r"Save\s+Configuration", r"Auto-detect")
    actions.append({"action": "key", "key": "Enter", "semantic_target": "accept default PNG Save Configuration"})
    key("enter")
    return actions, [dialog, typed, configuration]


def localize_capture(evidence: dict[str, Any], root: Path) -> dict[str, Any]:
    result = copy.deepcopy(evidence)
    for field in ("path", "ocr_path"):
        if field in result:
            result[field] = str(Path(result[field]).resolve().relative_to(root.resolve()))
    return result
