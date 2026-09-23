"""Text extraction from uploaded regulation/policy documents (PDF, DOCX, TXT, MD)."""

from __future__ import annotations

import io
import re
from pathlib import Path

from pypdf import PdfReader

SUPPORTED_EXTENSIONS = {".pdf", ".docx", ".txt", ".md"}


class ExtractionError(ValueError):
    pass


def _clean(text: str) -> str:
    text = text.replace("\r\n", "\n").replace("\r", "\n").replace(" ", " ")
    text = re.sub(r"(\w)-\n(\w)", r"\1\2", text)  # de-hyphenate line breaks
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return "\n".join(line.strip() for line in text.split("\n")).strip()


def extract_text(data: bytes, file_name: str) -> str:
    ext = Path(file_name).suffix.lower()
    if ext not in SUPPORTED_EXTENSIONS:
        raise ExtractionError(f"unsupported file type {ext}; supported: {sorted(SUPPORTED_EXTENSIONS)}")
    if ext == ".pdf":
        try:
            reader = PdfReader(io.BytesIO(data))
            pages = [(page.extract_text() or "") for page in reader.pages]
        except Exception as exc:
            raise ExtractionError(f"cannot read PDF: {exc}") from exc
        # drop typical page furniture: lone page numbers like "- 3 -"
        text = "\n".join(re.sub(r"^\s*-\s*\d+\s*-\s*$", "", p, flags=re.MULTILINE) for p in pages)
    elif ext == ".docx":
        from docx import Document

        try:
            doc = Document(io.BytesIO(data))
        except Exception as exc:
            raise ExtractionError(f"cannot read DOCX: {exc}") from exc
        text = "\n".join(p.text for p in doc.paragraphs)
    else:
        text = data.decode("utf-8", errors="replace")
    text = _clean(text)
    if not text:
        raise ExtractionError("document contains no extractable text (scanned PDF? OCR is on the backlog)")
    return text
