"""Section-level (Pasal-level) diff between two versions of a regulation."""

from __future__ import annotations

import difflib
import re
from dataclasses import asdict, dataclass
from typing import Any, Literal

_MAX_SNIPPET = 2000


@dataclass(frozen=True)
class SectionChange:
    section: str
    change: Literal["added", "removed", "modified"]
    before: str | None
    after: str | None
    similarity: float

    def as_dict(self) -> dict[str, Any]:
        return asdict(self)


def _norm(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip().lower()


def _section_sort_key(key: str) -> tuple[int, int, str, str]:
    penjelasan = 1 if key.startswith("Penjelasan") else 0
    m = re.search(r"Pasal (\d+)([A-Z]?)", key)
    return (penjelasan, int(m.group(1)) if m else -1, m.group(2) if m else "", key)


def diff_sections(old: dict[str, str], new: dict[str, str], threshold: float = 0.985) -> list[SectionChange]:
    """Compare section → text maps. Whitespace/case-only edits are not changes."""
    changes: list[SectionChange] = []
    for key in sorted(set(old) | set(new), key=_section_sort_key):
        before, after = old.get(key), new.get(key)
        if before is None and after is not None:
            changes.append(SectionChange(key, "added", None, after[:_MAX_SNIPPET], 0.0))
        elif after is None and before is not None:
            changes.append(SectionChange(key, "removed", before[:_MAX_SNIPPET], None, 0.0))
        elif before is not None and after is not None:
            a, b = _norm(before), _norm(after)
            if a == b:
                continue
            ratio = difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()
            if ratio < threshold:
                changes.append(
                    SectionChange(key, "modified", before[:_MAX_SNIPPET], after[:_MAX_SNIPPET], round(ratio, 4))
                )
    return changes


def unified_snippet(before: str, after: str, max_lines: int = 40) -> str:
    lines = list(difflib.unified_diff(before.split("\n"), after.split("\n"), lineterm="", n=1))
    return "\n".join(lines[2 : 2 + max_lines])
