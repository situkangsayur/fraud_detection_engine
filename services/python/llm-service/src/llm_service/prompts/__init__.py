"""Versioned prompt templates (Jinja2). Each file starts with ``{#- prompt: <name>  version: <n> -#}``."""

from __future__ import annotations

import json
import re
from functools import lru_cache
from pathlib import Path
from typing import Any

from jinja2 import Environment, FileSystemLoader, StrictUndefined

PROMPT_DIR = Path(__file__).parent
_VERSION = re.compile(r"version:\s*(\d+)")


@lru_cache
def _env() -> Environment:
    env = Environment(  # noqa: S701 — prompts are plain text, not HTML
        loader=FileSystemLoader(PROMPT_DIR),
        undefined=StrictUndefined,
        keep_trailing_newline=True,
        trim_blocks=True,
        lstrip_blocks=True,
    )
    # Jinja's built-in tojson HTML-escapes quotes (\u0027); prompts want compact, readable UTF-8 JSON.
    env.filters["tojson"] = _to_json
    return env


def _to_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, default=str, separators=(",", ":"))


def render(name: str, **context: Any) -> str:
    return _env().get_template(name).render(**context).strip()


def version_of(name: str) -> str:
    head = (PROMPT_DIR / name).read_text(encoding="utf-8").split("\n", 1)[0]
    m = _VERSION.search(head)
    return f"{name.split('.')[0]}@{m.group(1) if m else '0'}"
