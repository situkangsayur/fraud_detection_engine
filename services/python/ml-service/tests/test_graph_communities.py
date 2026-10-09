from __future__ import annotations

import json

from ml_service.graph.communities import detect_communities, parse_edges


def test_parse_edges_accepts_key_variants_and_skips_self_loops() -> None:
    lines = [
        json.dumps({"source": "a", "target": "b", "weight": 2}),
        json.dumps({"a": "b", "b": "c"}),
        json.dumps({"customer_a": "c", "customer_b": "c"}),
        "",
    ]
    assert parse_edges(lines) == [("a", "b", 2.0), ("b", "c", 1.0)]


def test_louvain_separates_two_cliques_and_counts_fraud() -> None:
    edges = [
        (a, b, 1.0)
        for a, b in [
            ("a1", "a2"),
            ("a2", "a3"),
            ("a1", "a3"),
            ("b1", "b2"),
            ("b2", "b3"),
            ("b1", "b3"),
            ("a3", "b1"),
        ]
    ]
    edges[-1] = ("a3", "b1", 0.1)
    assignments, stats = detect_communities(edges, fraud={"b1", "b2"})
    assert assignments["a1"] == assignments["a2"] == assignments["a3"]
    assert assignments["b1"] == assignments["b2"] == assignments["b3"]
    assert assignments["a1"] != assignments["b1"]
    fraud_comm = next(s for s in stats if s["community_id"] == assignments["b1"])
    assert fraud_comm["fraud_count"] == 2 and abs(fraud_comm["fraud_rate"] - 2 / 3) < 1e-6


def test_empty_graph() -> None:
    assert detect_communities([], set()) == ({}, [])
