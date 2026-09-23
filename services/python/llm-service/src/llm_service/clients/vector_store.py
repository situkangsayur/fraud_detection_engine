"""OpenSearch vector store — one index per tenant (``reg-chunks-<tenant_id>``).

k-NN field uses HNSW (lucene engine, cosinesimil) which supports efficient pre-filtering on
``regulation_id`` for project-scoped retrieval (OpenSearch >= 2.4, verified on 2.13 and 2.19).
"""

from __future__ import annotations

import contextlib
import uuid
from dataclasses import dataclass
from typing import Any

from opensearchpy import OpenSearch, helpers
from opensearchpy.exceptions import NotFoundError, RequestError
from starlette.concurrency import run_in_threadpool

from llm_service.config import Settings


@dataclass(frozen=True)
class ChunkDoc:
    chunk_id: str
    regulation_id: str
    code: str
    version: int
    doc_type: str
    issuer: str
    effective_date: str | None
    section: str
    title: str
    text: str
    ordinal: int
    embedding: list[float]


@dataclass(frozen=True)
class SearchHit:
    chunk_id: str
    regulation_id: str
    code: str
    version: int
    section: str
    title: str
    text: str
    score: float


class VectorStore:
    def __init__(self, settings: Settings, client: OpenSearch | None = None) -> None:
        self._prefix = settings.opensearch_index_prefix
        self._client = client or OpenSearch(
            hosts=[settings.opensearch_url], timeout=30, max_retries=3, retry_on_timeout=True
        )

    def index_name(self, tenant_id: uuid.UUID) -> str:
        return f"{self._prefix}{tenant_id}"

    # ------------------------------------------------------------------ admin
    def _ensure_index_sync(self, tenant_id: uuid.UUID, dim: int) -> None:
        name = self.index_name(tenant_id)
        if self._client.indices.exists(index=name):
            return
        body = {
            "settings": {"index": {"knn": True, "number_of_shards": 1, "number_of_replicas": 0}},
            "mappings": {
                "properties": {
                    "chunk_id": {"type": "keyword"},
                    "regulation_id": {"type": "keyword"},
                    "code": {"type": "keyword"},
                    "version": {"type": "integer"},
                    "doc_type": {"type": "keyword"},
                    "issuer": {"type": "keyword"},
                    "effective_date": {"type": "date", "ignore_malformed": True},
                    "section": {"type": "text", "fields": {"raw": {"type": "keyword"}}},
                    "title": {"type": "text"},
                    "text": {"type": "text"},
                    "ordinal": {"type": "integer"},
                    "embedding": {
                        "type": "knn_vector",
                        "dimension": dim,
                        "method": {
                            "name": "hnsw",
                            "space_type": "cosinesimil",
                            "engine": "lucene",
                            "parameters": {"ef_construction": 128, "m": 16},
                        },
                    },
                }
            },
        }
        try:
            self._client.indices.create(index=name, body=body)
        except RequestError as exc:  # concurrent creation
            if "resource_already_exists_exception" not in str(exc):
                raise

    async def ensure_index(self, tenant_id: uuid.UUID, dim: int) -> None:
        await run_in_threadpool(self._ensure_index_sync, tenant_id, dim)

    def _index_chunks_sync(self, tenant_id: uuid.UUID, chunks: list[ChunkDoc]) -> int:
        name = self.index_name(tenant_id)
        actions = [{"_op_type": "index", "_index": name, "_id": c.chunk_id, "_source": c.__dict__} for c in chunks]
        ok, _ = helpers.bulk(self._client, actions, refresh="wait_for")
        return int(ok)

    async def index_chunks(self, tenant_id: uuid.UUID, chunks: list[ChunkDoc]) -> int:
        if not chunks:
            return 0
        await self.ensure_index(tenant_id, len(chunks[0].embedding))
        return await run_in_threadpool(self._index_chunks_sync, tenant_id, chunks)

    def _delete_regulation_sync(self, tenant_id: uuid.UUID, regulation_id: uuid.UUID) -> None:
        with contextlib.suppress(NotFoundError):
            self._client.delete_by_query(
                index=self.index_name(tenant_id),
                body={"query": {"term": {"regulation_id": str(regulation_id)}}},
                refresh=True,
            )

    async def delete_regulation(self, tenant_id: uuid.UUID, regulation_id: uuid.UUID) -> None:
        await run_in_threadpool(self._delete_regulation_sync, tenant_id, regulation_id)

    # ------------------------------------------------------------------ search
    @staticmethod
    def _to_hits(resp: dict[str, Any]) -> list[SearchHit]:
        hits = []
        for h in resp.get("hits", {}).get("hits", []):
            s = h["_source"]
            hits.append(
                SearchHit(
                    chunk_id=s["chunk_id"],
                    regulation_id=s["regulation_id"],
                    code=s["code"],
                    version=int(s.get("version", 1)),
                    section=s.get("section", ""),
                    title=s.get("title", ""),
                    text=s["text"],
                    score=float(h["_score"]),
                )
            )
        return hits

    def _bm25_sync(self, tenant_id: uuid.UUID, query: str, regulation_ids: list[str], k: int) -> list[SearchHit]:
        body = {
            "size": k,
            "_source": {"excludes": ["embedding"]},
            "query": {
                "bool": {
                    "must": [{"multi_match": {"query": query, "fields": ["text", "section^2", "title"]}}],
                    "filter": [{"terms": {"regulation_id": regulation_ids}}],
                }
            },
        }
        try:
            return self._to_hits(self._client.search(index=self.index_name(tenant_id), body=body))
        except NotFoundError:
            return []

    def _knn_sync(
        self, tenant_id: uuid.UUID, vector: list[float], regulation_ids: list[str], k: int
    ) -> list[SearchHit]:
        body = {
            "size": k,
            "_source": {"excludes": ["embedding"]},
            "query": {
                "knn": {
                    "embedding": {
                        "vector": vector,
                        "k": k,
                        "filter": {"terms": {"regulation_id": regulation_ids}},
                    }
                }
            },
        }
        try:
            return self._to_hits(self._client.search(index=self.index_name(tenant_id), body=body))
        except NotFoundError:
            return []

    async def bm25(self, tenant_id: uuid.UUID, query: str, regulation_ids: list[str], k: int) -> list[SearchHit]:
        if not regulation_ids:
            return []
        return await run_in_threadpool(self._bm25_sync, tenant_id, query, regulation_ids, k)

    async def knn(
        self, tenant_id: uuid.UUID, vector: list[float], regulation_ids: list[str], k: int
    ) -> list[SearchHit]:
        if not regulation_ids:
            return []
        return await run_in_threadpool(self._knn_sync, tenant_id, vector, regulation_ids, k)

    def _ping_sync(self) -> bool:
        try:
            return bool(self._client.ping())
        except Exception:
            return False

    async def ping(self) -> bool:
        return await run_in_threadpool(self._ping_sync)
