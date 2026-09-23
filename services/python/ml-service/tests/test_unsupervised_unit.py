from __future__ import annotations

import numpy as np

from ml_service.training.unsupervised import ClusterAssigner, cluster_summaries


def test_cluster_assigner_nearest_centroid_and_noise() -> None:
    rng = np.random.default_rng(0)
    X = np.vstack([rng.normal(0, 0.1, (50, 2)), rng.normal(5, 0.1, (50, 2))])
    labels = np.array([0] * 50 + [1] * 50)
    assigner = ClusterAssigner.fit(X, labels)
    out = assigner.assign(np.array([[0.0, 0.0], [5.0, 5.0], [50.0, 50.0]]))
    assert out.tolist() == [0, 1, -1]
    assert ClusterAssigner.fit(X, np.full(100, -1)).assign(X[:2]).tolist() == [-1, -1]


def test_cluster_summaries_fraud_rate_and_top_features() -> None:
    X = np.array([[0, 0], [0, 1], [10, 0], [10, 1]], dtype=float)
    labels = np.array([0, 0, 1, 1])
    fraud = np.array([False, False, True, False])
    labelled = np.array([True, False, True, True])
    out = {
        c["cluster_id"]: c for c in cluster_summaries(X, X, ["a", "b"], ["a", "b"], labels, fraud, labelled)
    }
    assert out[0]["fraud_rate"] == 0.0 and out[0]["labeled_count"] == 1
    assert out[1]["fraud_rate"] == 0.5 and out[1]["size"] == 2
    assert out[1]["top_features"][0]["feature"] == "a"
    assert out[1]["profile"]["a"] == 10.0
