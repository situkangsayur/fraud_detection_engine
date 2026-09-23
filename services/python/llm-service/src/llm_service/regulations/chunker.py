"""Structure-aware chunking for Indonesian regulations (UU / POJK / PBI / SE) and internal policies.

Indonesian legal documents follow a fixed hierarchy::

    BAB I  KETENTUAN UMUM          (chapter + title on the next line)
      Bagian Kesatu  <title>       (part)
        Paragraf 1  <title>        (paragraph group, optional)
          Pasal 1                  (article — the natural retrieval & diff unit)
            (1) ... (2) ...        (ayat / clauses)
    PENJELASAN                     (elucidation, repeats "Pasal n ...")
    LAMPIRAN I                     (annex; subsections "I. LATAR BELAKANG", "II. ...")

Each *Pasal* becomes one unit carrying its section path. Pasal longer than ``max_chars`` are split on ayat
boundaries; anything still too long (or documents without Pasal, e.g. SOPs) falls back to word windows with
overlap.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

_BAB = re.compile(r"^BAB\s+([IVXLCDM]+|\d+)\b\.?\s*(.*)$", re.IGNORECASE)
_BAGIAN = re.compile(r"^Bagian\s+(Ke[a-z]+|\d+)\b\s*(.*)$", re.IGNORECASE)
_PARAGRAF = re.compile(r"^Paragraf\s+(\d+)\b\s*(.*)$", re.IGNORECASE)
_PASAL = re.compile(r"^Pasal\s+(\d+[A-Z]?)\s*$")
_PENJELASAN = re.compile(r"^PENJELASAN\b")
_LAMPIRAN = re.compile(r"^LAMPIRAN(?:\s+([IVXLC]+|\d+))?\s*$")
_ROMAN_HEADING = re.compile(r"^([IVXLC]+)\.\s+(\S.{2,120})$")
_AYAT = re.compile(r"^\(\d+[a-z]?\)\s")
_MD_HEADING = re.compile(r"^#{1,4}\s+(.+)$")


@dataclass
class Chunk:
    ordinal: int
    section: str  # human readable path, e.g. "BAB II PENERAPAN STRATEGI > Pasal 5"
    key: str  # stable diff key, e.g. "Pasal 5" / "Penjelasan Pasal 5" / "Pembukaan" / "part-3"
    title: str  # chapter title (for display / BM25 boost)
    text: str


@dataclass
class _Unit:
    key: str
    section: str
    title: str
    lines: list[str] = field(default_factory=list)

    @property
    def text(self) -> str:
        return "\n".join(self.lines).strip()


def _is_title_line(line: str, *, upper: bool) -> bool:
    """A heading title follows BAB (UPPERCASE) or Bagian/Paragraf (Title Case) on its own short line."""
    if any(p.match(line) for p in (_PASAL, _BAB, _BAGIAN, _PARAGRAF, _AYAT)):
        return False
    if len(line) > 120 or line.endswith((".", ";", ":", ",")):
        return False
    return line.isupper() if upper else line[:1].isupper()


def _word_windows(text: str, size: int, overlap: int) -> list[str]:
    words = text.split()
    if len(words) <= size:
        return [text] if text.strip() else []
    step = max(1, size - overlap)
    return [
        " ".join(words[i : i + size])
        for i in range(0, len(words), step)
        if words[i : i + size] and (i == 0 or i + overlap < len(words))
    ]


def _split_long(text: str, max_chars: int, window_words: int, overlap_words: int) -> list[str]:
    if len(text) <= max_chars:
        return [text]
    lines = text.split("\n")
    groups: list[list[str]] = [[]]
    for line in lines:  # group by ayat
        if _AYAT.match(line) and groups[-1]:
            groups.append([])
        groups[-1].append(line)
    parts: list[str] = []
    buf = ""
    for g in ("\n".join(x) for x in groups):
        if buf and len(buf) + len(g) + 1 > max_chars:
            parts.append(buf)
            buf = ""
        buf = f"{buf}\n{g}" if buf else g
    if buf:
        parts.append(buf)
    out: list[str] = []
    for p in parts:
        out.extend(_word_windows(p, window_words, overlap_words) if len(p) > max_chars else [p])
    return out


def _structural_units(lines: list[str]) -> list[_Unit]:
    units: list[_Unit] = []
    bab = bagian = paragraf = ""
    bab_title = ""
    in_penjelasan = False
    lampiran: str | None = None
    expect_title_for: str | None = None
    current = _Unit(key="Pembukaan", section="Pembukaan", title="")
    for raw in lines:
        line = raw.strip()
        if not line:
            continue
        if expect_title_for and _is_title_line(line, upper=expect_title_for == "bab"):
            if expect_title_for == "bab":
                bab_title = line
                bab = f"{bab} {line}"
            elif expect_title_for == "bagian":
                bagian = f"{bagian} {line}"
            elif expect_title_for == "paragraf":
                paragraf = f"{paragraf} {line}"
            expect_title_for = None
            continue
        expect_title_for = None
        if m := _LAMPIRAN.match(line):
            units.append(current)
            lampiran = f"Lampiran {m.group(1) or ''}".strip()
            current = _Unit(key=lampiran, section=lampiran, title=lampiran)
            continue
        if lampiran is not None:
            if m := _ROMAN_HEADING.match(line):
                units.append(current)
                heading = f"{m.group(1)}. {m.group(2).strip()}"
                current = _Unit(key=f"{lampiran} {m.group(1)}", section=f"{lampiran} > {heading}", title=lampiran)
                continue
            current.lines.append(line)
            continue
        if _PENJELASAN.match(line):
            in_penjelasan = True
            units.append(current)
            current = _Unit(key="Penjelasan Umum", section="Penjelasan > Umum", title="Penjelasan")
            bab = bagian = paragraf = ""
            continue
        if m := _BAB.match(line):
            bab = f"BAB {m.group(1).upper()}" + (f" {m.group(2).strip()}" if m.group(2).strip() else "")
            bab_title = m.group(2).strip()
            bagian = paragraf = ""
            expect_title_for = None if m.group(2).strip() else "bab"
            continue
        if m := _BAGIAN.match(line):
            bagian = f"Bagian {m.group(1).capitalize()}" + (f" {m.group(2).strip()}" if m.group(2).strip() else "")
            paragraf = ""
            expect_title_for = None if m.group(2).strip() else "bagian"
            continue
        if m := _PARAGRAF.match(line):
            paragraf = f"Paragraf {m.group(1)}" + (f" {m.group(2).strip()}" if m.group(2).strip() else "")
            expect_title_for = None if m.group(2).strip() else "paragraf"
            continue
        if m := _PASAL.match(line):
            units.append(current)
            pasal = f"Pasal {m.group(1)}"
            if in_penjelasan:
                current = _Unit(key=f"Penjelasan {pasal}", section=f"Penjelasan > {pasal}", title="Penjelasan")
            else:
                path = " > ".join(p for p in (bab, bagian, paragraf, pasal) if p)
                current = _Unit(key=pasal, section=path, title=bab_title)
            continue
        current.lines.append(line)
    units.append(current)
    return [u for u in units if u.text]


def _generic_units(lines: list[str]) -> list[_Unit]:
    """Documents without Pasal (SOP, internal policy): split on markdown/uppercase headings or paragraphs."""
    units: list[_Unit] = []
    current = _Unit(key="part-1", section="Bagian 1", title="")
    for raw in lines:
        line = raw.strip()
        heading = None
        if m := _MD_HEADING.match(line):
            heading = m.group(1).strip()
        elif line.isupper() and 3 < len(line) < 90 and len(line.split()) <= 10:
            heading = line
        if heading:
            if current.text:
                units.append(current)
            n = len(units) + 1
            current = _Unit(key=f"part-{n}", section=heading, title=heading)
            continue
        if line:
            current.lines.append(line)
    if current.text:
        units.append(current)
    return units


def chunk_document(
    text: str, *, max_chars: int = 1800, window_words: int = 220, overlap_words: int = 40
) -> list[Chunk]:
    lines = text.split("\n")
    has_pasal = sum(1 for ln in lines if _PASAL.match(ln.strip())) >= 2
    units = _structural_units(lines) if has_pasal else _generic_units(lines)
    chunks: list[Chunk] = []
    for unit in units:
        pieces = _split_long(unit.text, max_chars, window_words, overlap_words)
        for i, piece in enumerate(pieces):
            key = unit.key if len(pieces) == 1 else f"{unit.key}#{i + 1}"
            chunks.append(Chunk(ordinal=len(chunks), section=unit.section, key=key, title=unit.title, text=piece))
    return chunks


def section_texts(chunks: list[Chunk]) -> dict[str, str]:
    """Reassemble full text per diff unit (Pasal), merging split pieces (``key#n``)."""
    out: dict[str, str] = {}
    for c in chunks:
        base = c.key.split("#", 1)[0]
        out[base] = f"{out[base]}\n{c.text}" if base in out else c.text
    return out
