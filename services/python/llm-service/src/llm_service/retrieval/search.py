"""Hybrid regulation retrieval (BM25 + kNN, fused with RRF), restricted to a project's attached documents."""

from __future__ import annotations

import uuid
from dataclasses import dataclass
from typing import Any

from llm_service.clients.ollama import OllamaClient
from llm_service.clients.vector_store import SearchHit, VectorStore
from llm_service.retrieval.fusion import reciprocal_rank_fusion


@dataclass(frozen=True)
class RetrievedChunk:
    hit: SearchHit
    score: float

    def citation(self, excerpt_chars: int = 400) -> dict[str, Any]:
        h = self.hit
        return {
            "regulation_id": h.regulation_id,
            "chunk_id": h.chunk_id,
            "code": h.code,
            "version": h.version,
            "section": h.section,
            "excerpt": h.text[:excerpt_chars],
            "score": round(self.score, 5),
        }

    def as_document(self) -> dict[str, Any]:
        return {"code": self.hit.code, "section": self.hit.section, "text": self.hit.text}


class RegulationSearch:
    def __init__(self, store: VectorStore, ollama: OllamaClient) -> None:
        self._store = store
        self._ollama = ollama

    async def search(
        self, tenant_id: uuid.UUID, query: str, regulation_ids: list[uuid.UUID], k: int = 6
    ) -> list[RetrievedChunk]:
        ids = [str(r) for r in regulation_ids]
        if not ids or not query.strip():
            return []
        candidates = max(k * 3, 10)
        vector = (await self._ollama.embed([query]))[0]
        knn = await self._store.knn(tenant_id, vector, ids, candidates)
        bm25 = await self._store.bm25(tenant_id, query, ids, candidates)
        fused = reciprocal_rank_fusion([knn, bm25], limit=k)
        return [RetrievedChunk(hit=h, score=s) for h, s in fused]
