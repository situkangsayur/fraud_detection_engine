"""Request/response models."""

from __future__ import annotations

from typing import Any
from uuid import UUID

from pydantic import BaseModel, Field


class PredictRequest(BaseModel):
    event_id: UUID | None = None
    features: dict[str, Any] = Field(default_factory=dict)
    source: dict[str, Any] | None = Field(
        default=None, description="Raw source record; only needed when the model uses extra `source.*` fields"
    )
    explain: bool = True


class TopFeature(BaseModel):
    name: str
    contribution: float


class PredictResponse(BaseModel):
    event_id: UUID | None = None
    model_id: UUID
    model_version: int
    algorithm: str
    fraud_probability: float
    top_features: list[TopFeature]


class PredictBatchItem(BaseModel):
    event_id: UUID | None = None
    features: dict[str, Any] = Field(default_factory=dict)
    source: dict[str, Any] | None = None


class PredictBatchRequest(BaseModel):
    items: list[PredictBatchItem] = Field(max_length=5000)


class ScoreRequest(BaseModel):
    event_id: UUID | None = None
    features: dict[str, Any] = Field(default_factory=dict)
    source: dict[str, Any] | None = None


class ScoreResponse(BaseModel):
    event_id: UUID | None = None
    model_id: UUID
    model_version: int
    anomaly_score: float
    cluster_id: int
    cluster_fraud_rate: float | None


class TrainSupervisedRequest(BaseModel):
    algorithm: str | None = None
    params: dict[str, Any] | None = None
    since_days: int | None = Field(default=None, ge=1, le=3650)
    # Label maturity: events older than this many days without any label are treated as legit
    # (standard practice — fraud is usually reported within the chargeback/complaint window).
    # 0 or null → train on explicitly labelled events only.
    label_maturity_days: int | None = Field(default=14, ge=0, le=365)


class TrainUnsupervisedRequest(BaseModel):
    anomaly_algorithm: str | None = None
    anomaly_params: dict[str, Any] | None = None
    clustering_algorithm: str | None = None
    clustering_params: dict[str, Any] | None = None
    since_days: int | None = Field(default=90, ge=1, le=3650)


class TrainResponse(BaseModel):
    model_id: UUID
    version: int
    status: str


class DecisionRequest(BaseModel):
    comment: str | None = Field(default=None, max_length=2000)


class ClusterPatch(BaseModel):
    label: str | None = Field(default=None, max_length=200)
    notes: str | None = Field(default=None, max_length=5000)


class ValidateConfigRequest(BaseModel):
    ml_config: dict[str, Any]


class ValidationErrorItem(BaseModel):
    path: str
    message: str


class ValidateConfigResponse(BaseModel):
    valid: bool
    errors: list[ValidationErrorItem]
