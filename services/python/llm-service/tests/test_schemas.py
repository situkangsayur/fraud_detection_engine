from __future__ import annotations

from typing import Any

from llm_service.analysis import schemas


def _open_objects(node: Any, path: str = "$") -> list[str]:
    """Paths of objects that would be closed by a grammar: no fixed key list fits them, so they must be open."""
    bad: list[str] = []
    if isinstance(node, dict):
        if node.get("type") == "object" and node.get("additionalProperties") is not True:
            props = node.get("properties", {})
            # an object is "complete" only if it lists every key the model may need; free-form ones must be open
            if not props or set(props) == {"kind"}:
                bad.append(path)
        for k, v in node.items():
            bad += _open_objects(v, f"{path}.{k}")
    elif isinstance(node, list):
        for i, v in enumerate(node):
            bad += _open_objects(v, f"{path}[{i}]")
    return bad


def test_free_form_objects_allow_additional_properties() -> None:
    for name in dir(schemas):
        if name.endswith("_SCHEMA"):
            assert _open_objects(getattr(schemas, name)) == [], name


def _unbounded_strings(node: Any, path: str = "$") -> list[str]:
    bad: list[str] = []
    if isinstance(node, dict):
        if node.get("type") == "string" and not {"enum", "pattern", "maxLength"} & node.keys():
            bad.append(path)
        for k, v in node.items():
            bad += _unbounded_strings(v, f"{path}.{k}")
    elif isinstance(node, list):
        for i, v in enumerate(node):
            bad += _unbounded_strings(v, f"{path}[{i}]")
    return bad


def test_free_text_is_bounded_so_generation_terminates() -> None:
    for name in dir(schemas):
        if name.endswith("_SCHEMA"):
            assert _unbounded_strings(getattr(schemas, name)) == [], name
    assert schemas.RECOMMEND_OUTPUT_SCHEMA["properties"]["summary_md"]["maxLength"] == 6000
