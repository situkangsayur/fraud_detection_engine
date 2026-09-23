"""Louvain communities over the customer–customer projection exported by graph-service.

Export format (NDJSON, one edge per line): {"source": "<customer uuid>", "target": "<customer uuid>",
"weight": <float>}. `a`/`b` and `customer_a`/`customer_b` key variants are accepted too.
"""

from __future__ import annotations

import json
from collections.abc import Iterable
from typing import Any
from uuid import UUID

import httpx
import networkx as nx

from ml_service.config import get_settings
from ml_service.db import tenant_session
from ml_service.errors import ProblemError
from ml_service.logging import get_logger
from ml_service.repository import fraud_customers, replace_communities

log = get_logger(__name__)
_KEY_PAIRS = (("source", "target"), ("a", "b"), ("customer_a", "customer_b"))


def parse_edges(lines: Iterable[str]) -> list[tuple[str, str, float]]:
    edges: list[tuple[str, str, float]] = []
    for line in lines:
        line = line.strip()
        if not line:
            continue
        obj = json.loads(line)
        for a_key, b_key in _KEY_PAIRS:
            if a_key in obj and b_key in obj:
                a, b = str(obj[a_key]), str(obj[b_key])
                if a != b:
                    edges.append((a, b, float(obj.get("weight", 1.0) or 1.0)))
                break
    return edges


def detect_communities(
    edges: list[tuple[str, str, float]], fraud: set[str], seed: int = 42
) -> tuple[dict[str, int], list[dict[str, Any]]]:
    """Returns (customer → community id, per-community stats). Ids are ordered by size (0 = largest)."""
    graph = nx.Graph()
    for a, b, w in edges:
        if graph.has_edge(a, b):
            graph[a][b]["weight"] += w
        else:
            graph.add_edge(a, b, weight=w)
    if graph.number_of_nodes() == 0:
        return {}, []
    communities = nx.community.louvain_communities(graph, weight="weight", seed=seed)
    communities = sorted(communities, key=lambda c: (-len(c), min(c)))
    assignments: dict[str, int] = {}
    stats: list[dict[str, Any]] = []
    for cid, members in enumerate(communities):
        fraud_count = sum(1 for m in members if m in fraud)
        for m in members:
            assignments[m] = cid
        stats.append(
            {
                "community_id": cid,
                "size": len(members),
                "fraud_count": fraud_count,
                "fraud_rate": round(fraud_count / len(members), 6),
            }
        )
    return assignments, stats


def fetch_export(tenant_id: UUID, project_id: UUID) -> list[str]:
    settings = get_settings()
    url = f"{settings.graph_service_url}/v1/projects/{project_id}/export"
    headers = {
        "Authorization": f"Bearer {settings.internal_api_token}",
        "X-Tenant-Id": str(tenant_id),
        "X-Project-Id": str(project_id),
    }
    try:
        with (
            httpx.Client(timeout=settings.http_timeout_seconds) as client,
            client.stream("GET", url, headers=headers) as resp,
        ):
            resp.raise_for_status()
            return list(resp.iter_lines())
    except httpx.HTTPError as exc:
        raise ProblemError(
            502, "Bad Gateway", f"graph-service export failed: {exc}", type_="upstream_error"
        ) from exc


def recompute(tenant_id: UUID, project_id: UUID, lines: list[str] | None = None) -> dict[str, Any]:
    edges = parse_edges(lines if lines is not None else fetch_export(tenant_id, project_id))
    with tenant_session(tenant_id) as conn:
        fraud = fraud_customers(conn, project_id)
    assignments, stats = detect_communities(edges, fraud)
    with tenant_session(tenant_id) as conn:
        replace_communities(conn, tenant_id, project_id, assignments, stats)
    result = {
        "customers": len(assignments),
        "edges": len(edges),
        "communities": len(stats),
        "communities_with_fraud": sum(1 for s in stats if s["fraud_count"] > 0),
    }
    log.info("graph_communities_recomputed", project_id=str(project_id), **result)
    return result
