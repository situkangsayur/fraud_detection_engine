from __future__ import annotations

from dataclasses import dataclass

import pytest

from llm_service.regulations.diff import diff_sections
from llm_service.retrieval.fusion import reciprocal_rank_fusion


def test_diff_detects_added_removed_modified_and_ignores_whitespace() -> None:
    old = {
        "Pasal 1": "Definisi fraud.",
        "Pasal 2": "Batas Rp100.000.000,00 per transaksi.",
        "Pasal 3": "Wajib lapor   bulanan.",
        "Pasal 10": "Dihapus nanti.",
    }
    new = {
        "Pasal 1": "Definisi fraud.",
        "Pasal 2": "Batas Rp50.000.000,00 per transaksi dan per hari.",
        "Pasal 3": "Wajib lapor bulanan.",
        "Pasal 11": "Pasal baru tentang cashback.",
    }
    changes = {c.section: c for c in diff_sections(old, new)}
    assert set(changes) == {"Pasal 2", "Pasal 10", "Pasal 11"}
    assert changes["Pasal 2"].change == "modified" and 0 < changes["Pasal 2"].similarity < 1
    assert changes["Pasal 10"].change == "removed" and changes["Pasal 10"].after is None
    assert changes["Pasal 11"].change == "added"


def test_diff_orders_numerically_with_elucidation_last() -> None:
    old: dict[str, str] = {}
    new = {"Pasal 10": "a", "Pasal 2": "b", "Penjelasan Pasal 1": "c", "Pasal 2A": "d"}
    assert [c.section for c in diff_sections(old, new)] == ["Pasal 2", "Pasal 2A", "Pasal 10", "Penjelasan Pasal 1"]


@dataclass(frozen=True)
class Doc:
    chunk_id: str


def test_rrf_scores_and_order() -> None:
    a, b, c, d = Doc("a"), Doc("b"), Doc("c"), Doc("d")
    fused = reciprocal_rank_fusion([[a, b, c], [c, a, d]], k=60)
    ids = [x.chunk_id for x, _ in fused]
    assert ids[0] == "a"  # rank 1 + rank 2
    assert ids[1] == "c"  # rank 3 + rank 1
    score_a = dict((x.chunk_id, s) for x, s in fused)["a"]
    assert score_a == pytest.approx(1 / 61 + 1 / 62)
    assert len(reciprocal_rank_fusion([[a, b, c], [d]], limit=2)) == 2
    assert reciprocal_rank_fusion([]) == []
