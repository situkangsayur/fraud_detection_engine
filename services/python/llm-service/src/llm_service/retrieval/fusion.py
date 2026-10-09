"""Reciprocal Rank Fusion (Cormack et al., 2009) for hybrid BM25 + kNN retrieval."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Protocol


class HasId(Protocol):
    @property
    def chunk_id(self) -> str: ...


def reciprocal_rank_fusion[T: HasId](
    rankings: Sequence[Sequence[T]], k: int = 60, limit: int | None = None
) -> list[tuple[T, float]]:
    """Merge several ranked lists. score(d) = Σ 1 / (k + rank_i(d)), rank starting at 1.

    Documents are identified by ``chunk_id``; the first occurrence's object is kept.
    """
    scores: dict[str, float] = {}
    items: dict[str, T] = {}
    for ranking in rankings:
        for rank, item in enumerate(ranking, start=1):
            scores[item.chunk_id] = scores.get(item.chunk_id, 0.0) + 1.0 / (k + rank)
            items.setdefault(item.chunk_id, item)
    ordered = sorted(scores.items(), key=lambda kv: (-kv[1], kv[0]))
    if limit is not None:
        ordered = ordered[:limit]
    return [(items[cid], score) for cid, score in ordered]
