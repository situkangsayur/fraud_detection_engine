"""Internal hot-path inference (called by core-api's scoring pipeline). No DB access when models are cached."""

from __future__ import annotations

import time
from typing import Annotated, Any
from uuid import UUID

import numpy as np
from fastapi import APIRouter, Depends
from prometheus_client import Histogram

from ml_service.api.schemas import (
    PredictBatchRequest,
    PredictRequest,
    PredictResponse,
    ScoreRequest,
    ScoreResponse,
)
from ml_service.auth import Principal, require_internal_project
from ml_service.serving.cache import get_cache
from ml_service.serving.loaded import LoadedSupervised, LoadedUnsupervised

router = APIRouter(prefix="/v1/projects/{pid}", tags=["inference"])
Internal = Annotated[Principal, Depends(require_internal_project())]
LATENCY = Histogram(
    "ml_inference_seconds",
    "Model inference latency",
    ["kind"],
    buckets=(0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 1.0),
)


def _supervised(principal: Principal, pid: UUID) -> LoadedSupervised:
    assert principal.tenant_id is not None
    model = get_cache().get(principal.tenant_id, pid, "supervised")
    assert isinstance(model, LoadedSupervised)
    return model


@router.post("/supervised/predict", response_model=PredictResponse)
def predict(pid: UUID, body: PredictRequest, principal: Internal) -> PredictResponse:
    started = time.perf_counter()
    model = _supervised(principal, pid)
    proba, top = model.predict(body.features, body.source, body.explain)
    LATENCY.labels("supervised").observe(time.perf_counter() - started)
    return PredictResponse(
        event_id=body.event_id,
        model_id=UUID(model.model_id),
        model_version=model.version,
        algorithm=model.algorithm,
        fraud_probability=round(proba, 6),
        top_features=top,
    )


@router.post("/supervised/predict/batch")
def predict_batch(pid: UUID, body: PredictBatchRequest, principal: Internal) -> dict[str, Any]:
    model = _supervised(principal, pid)
    probs = model.predict_batch([(i.features, i.source) for i in body.items]) if body.items else np.zeros(0)
    return {
        "model_id": model.model_id,
        "model_version": model.version,
        "algorithm": model.algorithm,
        "results": [
            {"event_id": str(i.event_id) if i.event_id else None, "fraud_probability": round(float(p), 6)}
            for i, p in zip(body.items, probs, strict=True)
        ],
    }


@router.post("/unsupervised/score", response_model=ScoreResponse)
def score(pid: UUID, body: ScoreRequest, principal: Internal) -> ScoreResponse:
    started = time.perf_counter()
    assert principal.tenant_id is not None
    model = get_cache().get(principal.tenant_id, pid, "unsupervised")
    assert isinstance(model, LoadedUnsupervised)
    anomaly, cluster, rate = model.score(body.features, body.source)
    LATENCY.labels("unsupervised").observe(time.perf_counter() - started)
    return ScoreResponse(
        event_id=body.event_id,
        model_id=UUID(model.model_id),
        model_version=model.version,
        anomaly_score=round(anomaly, 6),
        cluster_id=cluster,
        cluster_fraud_rate=rate,
    )
