from __future__ import annotations

import json

from llm_service.analysis.service import _slim_clusters, _slim_components


def test_clusters_keep_riskiest_and_drop_heavy_fields() -> None:
    raw = {
        "model_id": "m",
        "items": [
            {
                "cluster_id": i,
                "size": 100 + i,
                "fraud_rate": i / 100,
                "labeled_count": 5,
                "label": None,
                "profile": {f"f{j}": j for j in range(60)},
                "top_features": [
                    {"feature": f"x{j}", "smd": j, "cluster_mean": 1, "overall_mean": 0} for j in range(10)
                ],
            }
            for i in range(37)
        ],
    }
    out = _slim_clusters(raw)
    assert len(out) == 12 and out[0]["cluster_id"] == 36  # highest fraud rate first
    assert "profile" not in out[0] and len(out[0]["top_features"]) == 3
    assert len(json.dumps(out)) < len(json.dumps(raw)) / 10


def test_components_drop_member_ids_and_rank_by_fraud() -> None:
    raw = [
        {
            "component_id": f"c{i}",
            "size": 10,
            "fraud_count": i,
            "fraud_rate": i / 10,
            "sample_customer_ids": [f"u{j}" for j in range(10)],
        }
        for i in range(30)
    ]
    out = _slim_components(raw, 5)
    assert [c["fraud_count"] for c in out] == [29, 28, 27, 26, 25]
    assert set(out[0]) == {"size", "fraud_count", "fraud_rate"}
