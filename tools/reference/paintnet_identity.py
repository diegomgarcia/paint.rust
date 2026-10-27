#!/usr/bin/env python3
"""Enforce the reviewed Paint.NET executable identity from observed evidence."""

from __future__ import annotations

from typing import Any


# Reviewed against the authentic passing Invert and PDN environment inventories.
REVIEWED_FILE_VERSION = "5.112.9563.32325"
REVIEWED_PRODUCT_VERSION = "5.112.9563.32325"
REVIEWED_EXECUTABLE_SHA256 = "17b27aee8eb0d18e7b39964e6c1c43b1dac5705f23727af550f2cde7fe737e2e"


class PaintNetIdentityError(ValueError):
    """Observed Paint.NET identity does not match the reviewed reference."""


def _observed_display_version(file_version: str) -> str:
    """Decode Paint.NET's observed major/minor/patch file-version fields."""
    parts = file_version.split(".")
    if len(parts) != 4 or not all(part.isdigit() for part in parts) or len(parts[1]) < 2:
        raise PaintNetIdentityError(f"Paint.NET file_version has unknown display semantics: {file_version!r}")
    minor = parts[1][0]
    patch = str(int(parts[1][1:]))
    return f"{parts[0]}.{minor}.{patch}"


def require_reviewed_paintnet(environment: dict[str, Any]) -> dict[str, Any]:
    """Return manifest identity only after exact reviewed-evidence checks pass."""
    guest = environment.get("guest") if isinstance(environment, dict) else None
    paintnet = guest.get("paintnet") if isinstance(guest, dict) else None
    if not isinstance(paintnet, dict):
        raise PaintNetIdentityError("missing guest.paintnet executable evidence")

    expected = {
        "file_version": REVIEWED_FILE_VERSION,
        "product_version": REVIEWED_PRODUCT_VERSION,
        "signature_status": "Valid",
        "sha256": REVIEWED_EXECUTABLE_SHA256,
    }
    for field, reviewed in expected.items():
        observed = paintnet.get(field)
        if observed != reviewed:
            raise PaintNetIdentityError(
                f"Paint.NET {field} is {observed!r}; expected reviewed {reviewed!r}"
            )

    file_version = paintnet["file_version"]
    return {
        "display_version": _observed_display_version(file_version),
        "file_version": file_version,
        "product_version": paintnet["product_version"],
        "executable_path": paintnet.get("path"),
        "source_sha256": paintnet["sha256"],
        "signature_status": paintnet["signature_status"],
        "signer_subject": paintnet.get("signer_subject"),
    }
