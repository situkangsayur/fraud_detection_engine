"""Built-in anomaly-detection plugins. Scores are rank-normalised to [0, 1] via `EmpiricalCdf`."""

from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any, ClassVar

import joblib
import numpy as np
import torch
from sklearn.ensemble import IsolationForest
from sklearn.neighbors import LocalOutlierFactor
from torch import nn

from ml_service.plugins.base import NOOP_PROGRESS, AnomalyPlugin, EmpiricalCdf, ProgressCallback


class _CdfAnomaly(AnomalyPlugin):
    """Anomaly plugin whose raw scores (higher = more anomalous) are normalised by the training CDF."""

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._cdf: EmpiricalCdf | None = None

    def _raw(self, X: np.ndarray) -> np.ndarray:
        raise NotImplementedError

    def score(self, X: np.ndarray) -> np.ndarray:
        if self._cdf is None:
            raise RuntimeError("model not fitted")
        return self._cdf(self._raw(X))

    def _save_cdf(self, directory: Path) -> None:
        (directory / "cdf.json").write_text(json.dumps(self._cdf.to_list() if self._cdf else []))

    def _load_cdf(self, directory: Path) -> None:
        self._cdf = EmpiricalCdf.from_list(json.loads((directory / "cdf.json").read_text()))


class IsolationForestPlugin(_CdfAnomaly):
    name = "isolation_forest"
    version = "1.0.0"
    display_name = "Isolation forest"
    description = "Tree-based isolation of rare points. Fast, robust default for tabular anomaly detection."
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "n_estimators": {"type": "integer", "minimum": 10, "maximum": 5000, "default": 200},
            "max_samples": {
                "oneOf": [{"type": "string", "enum": ["auto"]}, {"type": "integer", "minimum": 2}],
                "default": "auto",
            },
            "max_features": {"type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 1.0},
            "seed": {"type": "integer", "default": 42},
        },
    }
    smoke_params: ClassVar[dict[str, Any]] = {"n_estimators": 20}

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._model: IsolationForest | None = None

    def fit(
        self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS
    ) -> dict[str, Any]:
        p = self.params
        self._model = IsolationForest(
            n_estimators=int(p["n_estimators"]),
            max_samples=p["max_samples"],
            max_features=float(p["max_features"]),
            random_state=int(p["seed"]),
            n_jobs=1,
        ).fit(X)
        self._cdf = EmpiricalCdf(self._raw(X))
        progress(1.0)
        return {}

    def _raw(self, X: np.ndarray) -> np.ndarray:
        if self._model is None:
            raise RuntimeError("model not fitted")
        return -self._model.score_samples(X)

    def save(self, directory: Path) -> None:
        self._write_params(directory)
        joblib.dump(self._model, directory / "model.joblib")
        self._save_cdf(directory)

    @classmethod
    def load(cls, directory: Path) -> IsolationForestPlugin:
        plugin = cls(cls._read_params(directory))
        plugin._model = joblib.load(directory / "model.joblib")
        plugin._load_cdf(directory)
        return plugin


class LocalOutlierFactorPlugin(_CdfAnomaly):
    name = "local_outlier_factor"
    version = "1.0.0"
    display_name = "Local outlier factor"
    description = (
        "Density-based: flags points in sparser neighbourhoods than their neighbours (novelty mode)."
    )
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "n_neighbors": {"type": "integer", "minimum": 2, "maximum": 1000, "default": 20},
            "metric": {
                "type": "string",
                "enum": ["euclidean", "manhattan", "minkowski"],
                "default": "euclidean",
            },
        },
    }

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._model: LocalOutlierFactor | None = None

    def fit(
        self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS
    ) -> dict[str, Any]:
        n_neighbors = min(int(self.params["n_neighbors"]), max(len(X) - 1, 1))
        self._model = LocalOutlierFactor(n_neighbors=n_neighbors, metric=self.params["metric"], novelty=True)
        self._model.fit(X)
        self._cdf = EmpiricalCdf(-self._model.negative_outlier_factor_)
        progress(1.0)
        return {"n_neighbors_used": n_neighbors}

    def _raw(self, X: np.ndarray) -> np.ndarray:
        if self._model is None:
            raise RuntimeError("model not fitted")
        return -self._model.score_samples(X)

    def save(self, directory: Path) -> None:
        self._write_params(directory)
        joblib.dump(self._model, directory / "model.joblib")
        self._save_cdf(directory)

    @classmethod
    def load(cls, directory: Path) -> LocalOutlierFactorPlugin:
        plugin = cls(cls._read_params(directory))
        plugin._model = joblib.load(directory / "model.joblib")
        plugin._load_cdf(directory)
        return plugin


class _AutoEncoder(nn.Module):
    def __init__(self, input_dim: int, hidden: list[int]) -> None:
        super().__init__()
        enc: list[nn.Module] = []
        prev = input_dim
        for width in hidden:
            enc += [nn.Linear(prev, width), nn.ReLU()]
            prev = width
        dec: list[nn.Module] = []
        for width in [*reversed(hidden[:-1]), input_dim]:
            dec.append(nn.Linear(prev, width))
            if width != input_dim:
                dec.append(nn.ReLU())
            prev = width
        self.encoder = nn.Sequential(*enc)
        self.decoder = nn.Sequential(*dec)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        return self.decoder(self.encoder(x))


class AutoencoderPlugin(_CdfAnomaly):
    name = "autoencoder"
    version = "1.0.0"
    display_name = "Autoencoder (reconstruction error)"
    description = "PyTorch autoencoder; rows that reconstruct poorly are anomalous."
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "hidden_layers": {
                "type": "array",
                "items": {"type": "integer", "minimum": 1},
                "minItems": 1,
                "maxItems": 5,
                "default": [32, 8],
                "description": "Encoder widths; last = latent",
            },
            "epochs": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 30},
            "lr": {"type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 0.001},
            "batch_size": {"type": "integer", "minimum": 8, "default": 256},
            "seed": {"type": "integer", "default": 42},
        },
    }
    smoke_params: ClassVar[dict[str, Any]] = {"epochs": 2, "hidden_layers": [4]}

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._model: _AutoEncoder | None = None
        self._input_dim = 0

    def fit(
        self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS
    ) -> dict[str, Any]:
        p = self.params
        torch.manual_seed(int(p["seed"]))
        gen = torch.Generator().manual_seed(int(p["seed"]))
        self._input_dim = X.shape[1]
        model = _AutoEncoder(self._input_dim, list(p["hidden_layers"]))
        optim = torch.optim.Adam(model.parameters(), lr=float(p["lr"]))
        xt = torch.as_tensor(X, dtype=torch.float32)
        losses: list[float] = []
        best_state, best_loss = copy.deepcopy(model.state_dict()), np.inf
        epochs, batch = int(p["epochs"]), int(p["batch_size"])
        for epoch in range(epochs):
            model.train()
            perm = torch.randperm(len(xt), generator=gen)
            total = 0.0
            for start in range(0, len(perm), batch):
                idx = perm[start : start + batch]
                optim.zero_grad()
                loss = ((model(xt[idx]) - xt[idx]) ** 2).mean()
                loss.backward()
                optim.step()
                total += float(loss.item()) * len(idx)
            epoch_loss = total / max(len(xt), 1)
            losses.append(epoch_loss)
            if epoch_loss < best_loss:
                best_loss, best_state = epoch_loss, copy.deepcopy(model.state_dict())
            progress((epoch + 1) / epochs)
        model.load_state_dict(best_state)
        model.eval()
        self._model = model
        self._cdf = EmpiricalCdf(self._raw(X))
        return {"train_loss": losses}

    def _raw(self, X: np.ndarray) -> np.ndarray:
        if self._model is None:
            raise RuntimeError("model not fitted")
        with torch.no_grad():
            xt = torch.as_tensor(X, dtype=torch.float32)
            return ((self._model(xt) - xt) ** 2).mean(dim=1).numpy().astype(np.float64)

    def save(self, directory: Path) -> None:
        self._write_params(directory)
        if self._model is None:
            raise RuntimeError("model not fitted")
        torch.save(self._model.state_dict(), directory / "model.pt")
        (directory / "meta.json").write_text(json.dumps({"input_dim": self._input_dim}))
        self._save_cdf(directory)

    @classmethod
    def load(cls, directory: Path) -> AutoencoderPlugin:
        plugin = cls(cls._read_params(directory))
        plugin._input_dim = int(json.loads((directory / "meta.json").read_text())["input_dim"])
        model = _AutoEncoder(plugin._input_dim, list(plugin.params["hidden_layers"]))
        model.load_state_dict(torch.load(directory / "model.pt", weights_only=True))
        model.eval()
        plugin._model = model
        plugin._load_cdf(directory)
        return plugin


PLUGINS: list[type[AnomalyPlugin]] = [IsolationForestPlugin, LocalOutlierFactorPlugin, AutoencoderPlugin]
