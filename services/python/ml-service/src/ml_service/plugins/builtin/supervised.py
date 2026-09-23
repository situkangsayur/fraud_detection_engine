"""Built-in supervised plugins: PyTorch MLP (backpropagation) and scikit-learn baselines."""

from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any, ClassVar

import joblib
import numpy as np
import torch
from sklearn.ensemble import HistGradientBoostingClassifier, RandomForestClassifier
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import average_precision_score
from torch import nn

from ml_service.plugins.base import NOOP_PROGRESS, ProgressCallback, SupervisedPlugin


def _pr_auc(y: np.ndarray, p: np.ndarray) -> float | None:
    if len(np.unique(y)) < 2:
        return None
    return float(average_precision_score(y, p))


# ---------------------------------------------------------------------------------------------- MLP
class _Mlp(nn.Module):
    def __init__(self, input_dim: int, hidden: list[int], dropout: float, batch_norm: bool) -> None:
        super().__init__()
        layers: list[nn.Module] = []
        prev = input_dim
        for width in hidden:
            layers.append(nn.Linear(prev, width))
            if batch_norm:
                layers.append(nn.BatchNorm1d(width))
            layers.append(nn.ReLU())
            if dropout > 0:
                layers.append(nn.Dropout(dropout))
            prev = width
        layers.append(nn.Linear(prev, 1))
        self.net = nn.Sequential(*layers)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        return self.net(x).squeeze(-1)


class MlpBackpropPlugin(SupervisedPlugin):
    """Feed-forward neural network trained with backpropagation (Adam, BCE-with-logits)."""

    name = "mlp_backprop"
    version = "1.0.0"
    display_name = "Neural network (MLP, backpropagation)"
    description = (
        "Multi-layer perceptron trained with backpropagation. Handles class imbalance with a positive-class "
        "weight and stops early on validation PR-AUC. Explanations use gradient × input."
    )
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "hidden_layers": {
                "type": "array",
                "items": {"type": "integer", "minimum": 1, "maximum": 1024},
                "minItems": 1,
                "maxItems": 6,
                "default": [64, 32],
                "description": "Width of each hidden layer",
            },
            "dropout": {"type": "number", "minimum": 0, "maximum": 0.9, "default": 0.2},
            "batch_norm": {"type": "boolean", "default": True},
            "lr": {"type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 0.001},
            "weight_decay": {"type": "number", "minimum": 0, "maximum": 1, "default": 1e-5},
            "epochs": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 50},
            "batch_size": {
                "type": "integer",
                "minimum": 8,
                "maximum": 65536,
                "default": 256,
                "description": "Upper bound; small datasets use smaller batches (≥ ~10 steps/epoch)",
            },
            "patience": {"type": "integer", "minimum": 1, "maximum": 100, "default": 5},
            "pos_weight": {
                "oneOf": [{"type": "string", "enum": ["auto"]}, {"type": "number", "exclusiveMinimum": 0}],
                "default": "auto",
                "description": "'auto' = negatives/positives (capped at 1000)",
            },
            "seed": {"type": "integer", "default": 42},
        },
    }
    smoke_params: ClassVar[dict[str, Any]] = {"epochs": 3, "hidden_layers": [8]}

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._model: _Mlp | None = None
        self._input_dim = 0

    def _build(self, input_dim: int) -> _Mlp:
        self._input_dim = input_dim
        return _Mlp(
            input_dim,
            list(self.params["hidden_layers"]),
            float(self.params["dropout"]),
            bool(self.params["batch_norm"]),
        )

    def fit(
        self,
        X_train: np.ndarray,
        y_train: np.ndarray,
        X_val: np.ndarray,
        y_val: np.ndarray,
        feature_names: list[str],
        progress: ProgressCallback = NOOP_PROGRESS,
    ) -> dict[str, Any]:
        p = self.params
        torch.manual_seed(int(p["seed"]))
        gen = torch.Generator().manual_seed(int(p["seed"]))
        model = self._build(X_train.shape[1])
        xt = torch.as_tensor(X_train, dtype=torch.float32)
        yt = torch.as_tensor(y_train, dtype=torch.float32)
        xv = torch.as_tensor(X_val, dtype=torch.float32)

        n_pos = float(y_train.sum())
        n_neg = float(len(y_train) - n_pos)
        if p["pos_weight"] == "auto":
            pos_weight = min(1000.0, max(1.0, n_neg / max(n_pos, 1.0)))
        else:
            pos_weight = float(p["pos_weight"])
        loss_fn = nn.BCEWithLogitsLoss(pos_weight=torch.tensor(pos_weight))
        optim = torch.optim.Adam(model.parameters(), lr=float(p["lr"]), weight_decay=float(p["weight_decay"]))

        history: dict[str, Any] = {
            "train_loss": [],
            "val_pr_auc": [],
            "val_loss": [],
            "pos_weight": pos_weight,
        }
        # Early stopping key: PR-AUC first, validation loss as tie-breaker. PR-AUC alone saturates (1.0) on
        # separable data after one epoch, which would freeze nearly-untrained, badly calibrated weights.
        best_pr, best_loss = -np.inf, np.inf
        best_state = copy.deepcopy(model.state_dict())
        best_epoch = 0
        stale = 0
        epochs = int(p["epochs"])
        # Small datasets would otherwise get 1–2 optimizer steps per epoch and stay under-fitted.
        batch_size = max(8, min(int(p["batch_size"]), len(xt) // 10))
        history["batch_size"] = batch_size
        for epoch in range(epochs):
            model.train()
            perm = torch.randperm(len(xt), generator=gen)
            total = 0.0
            for start in range(0, len(perm), batch_size):
                idx = perm[start : start + batch_size]
                if len(idx) < 2 and p["batch_norm"]:
                    continue  # BatchNorm cannot train on a single sample
                optim.zero_grad()
                loss = loss_fn(model(xt[idx]), yt[idx])
                loss.backward()
                optim.step()
                total += float(loss.item()) * len(idx)
            history["train_loss"].append(total / max(len(xt), 1))

            model.eval()
            with torch.no_grad():
                logits = model(xv)
                val_loss = float(loss_fn(logits, torch.as_tensor(y_val, dtype=torch.float32)).item())
                proba = torch.sigmoid(logits).numpy()
            val_pr = _pr_auc(y_val, proba)
            history["val_pr_auc"].append(val_pr)
            history["val_loss"].append(val_loss)
            pr = val_pr if val_pr is not None else -np.inf
            improved = pr > best_pr + 1e-4 or (abs(pr - best_pr) <= 1e-4 and val_loss < best_loss - 1e-6)
            if improved:
                best_pr, best_loss = max(pr, best_pr), val_loss
                best_state, best_epoch, stale = copy.deepcopy(model.state_dict()), epoch, 0
            else:
                stale += 1
            progress((epoch + 1) / epochs)
            if stale >= int(p["patience"]):
                break
        model.load_state_dict(best_state)
        model.eval()
        self._model = model
        history["best_epoch"] = best_epoch
        history["epochs_run"] = len(history["train_loss"])
        return history

    def _require(self) -> _Mlp:
        if self._model is None:
            raise RuntimeError("model not fitted")
        return self._model

    def predict_proba(self, X: np.ndarray) -> np.ndarray:
        model = self._require()
        with torch.no_grad():
            return torch.sigmoid(model(torch.as_tensor(X, dtype=torch.float32))).numpy().astype(np.float64)

    def explain(
        self, X: np.ndarray, feature_names: list[str], top_k: int = 5
    ) -> list[list[tuple[str, float]]]:
        model = self._require()
        x = torch.as_tensor(X, dtype=torch.float32).requires_grad_(True)
        model(x).sum().backward()
        grad = x.grad
        contributions = (grad * x).detach().numpy() if grad is not None else np.zeros_like(X)
        out: list[list[tuple[str, float]]] = []
        for row in contributions:
            order = np.argsort(-np.abs(row))[:top_k]
            out.append([(feature_names[i], float(row[i])) for i in order if row[i] != 0.0])
        return out

    def save(self, directory: Path) -> None:
        self._write_params(directory)
        torch.save(self._require().state_dict(), directory / "model.pt")
        (directory / "meta.json").write_text(json.dumps({"input_dim": self._input_dim}))

    @classmethod
    def load(cls, directory: Path) -> MlpBackpropPlugin:
        plugin = cls(cls._read_params(directory))
        meta = json.loads((directory / "meta.json").read_text())
        model = plugin._build(int(meta["input_dim"]))
        model.load_state_dict(torch.load(directory / "model.pt", weights_only=True))
        model.eval()
        plugin._model = model
        return plugin


# -------------------------------------------------------------------------------------- scikit-learn
class _SklearnSupervised(SupervisedPlugin):
    """Shared persistence for scikit-learn estimators."""

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._estimator: Any = None

    def _make(self) -> Any:
        raise NotImplementedError

    def fit(
        self,
        X_train: np.ndarray,
        y_train: np.ndarray,
        X_val: np.ndarray,
        y_val: np.ndarray,
        feature_names: list[str],
        progress: ProgressCallback = NOOP_PROGRESS,
    ) -> dict[str, Any]:
        self._estimator = self._make()
        self._estimator.fit(X_train, y_train)
        self._after_fit()
        progress(1.0)
        return {"val_pr_auc": _pr_auc(y_val, self.predict_proba(X_val)) if len(X_val) else None}

    def _after_fit(self) -> None:
        return None

    def predict_proba(self, X: np.ndarray) -> np.ndarray:
        if self._estimator is None:
            raise RuntimeError("model not fitted")
        return self._estimator.predict_proba(X)[:, 1].astype(np.float64)

    def save(self, directory: Path) -> None:
        self._write_params(directory)
        joblib.dump(self._estimator, directory / "model.joblib")

    @classmethod
    def load(cls, directory: Path) -> _SklearnSupervised:
        plugin = cls(cls._read_params(directory))
        plugin._estimator = joblib.load(directory / "model.joblib")
        return plugin


class LogisticRegressionPlugin(_SklearnSupervised):
    name = "logistic_regression"
    version = "1.0.0"
    display_name = "Logistic regression"
    description = "Linear baseline with balanced class weights. Explanations are coefficient × feature value."
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "C": {
                "type": "number",
                "exclusiveMinimum": 0,
                "default": 1.0,
                "description": "Inverse regularisation",
            },
            "max_iter": {"type": "integer", "minimum": 10, "maximum": 100000, "default": 1000},
            "class_weight": {"type": "string", "enum": ["balanced", "none"], "default": "balanced"},
        },
    }

    def _make(self) -> Any:
        p = self.params
        return LogisticRegression(
            C=float(p["C"]),
            max_iter=int(p["max_iter"]),
            class_weight=None if p["class_weight"] == "none" else "balanced",
        )

    def explain(
        self, X: np.ndarray, feature_names: list[str], top_k: int = 5
    ) -> list[list[tuple[str, float]]]:
        coef = self._estimator.coef_[0]
        out: list[list[tuple[str, float]]] = []
        for row in X * coef:
            order = np.argsort(-np.abs(row))[:top_k]
            out.append([(feature_names[i], float(row[i])) for i in order if row[i] != 0.0])
        return out


class GradientBoostingPlugin(_SklearnSupervised):
    name = "gradient_boosting"
    version = "1.0.0"
    display_name = "Gradient boosting (histogram)"
    description = "scikit-learn HistGradientBoostingClassifier with balanced class weights."
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "learning_rate": {"type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 0.1},
            "max_iter": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 200},
            "max_leaf_nodes": {"type": "integer", "minimum": 2, "maximum": 1024, "default": 31},
            "max_depth": {"type": ["integer", "null"], "minimum": 1, "default": None},
            "l2_regularization": {"type": "number", "minimum": 0, "default": 0.0},
            "class_weight": {"type": "string", "enum": ["balanced", "none"], "default": "balanced"},
            "seed": {"type": "integer", "default": 42},
        },
    }
    smoke_params: ClassVar[dict[str, Any]] = {"max_iter": 10}

    def _make(self) -> Any:
        p = self.params
        return HistGradientBoostingClassifier(
            learning_rate=float(p["learning_rate"]),
            max_iter=int(p["max_iter"]),
            max_leaf_nodes=int(p["max_leaf_nodes"]),
            max_depth=p["max_depth"],
            l2_regularization=float(p["l2_regularization"]),
            class_weight=None if p["class_weight"] == "none" else "balanced",
            random_state=int(p["seed"]),
        )


class RandomForestPlugin(_SklearnSupervised):
    name = "random_forest"
    version = "1.0.0"
    display_name = "Random forest"
    description = "scikit-learn RandomForestClassifier with balanced-subsample class weights."
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "n_estimators": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 200},
            "max_depth": {"type": ["integer", "null"], "minimum": 1, "default": None},
            "min_samples_leaf": {"type": "integer", "minimum": 1, "default": 2},
            "n_jobs": {
                "type": "integer",
                "minimum": -1,
                "default": -1,
                "description": "Training parallelism",
            },
            "seed": {"type": "integer", "default": 42},
        },
    }
    smoke_params: ClassVar[dict[str, Any]] = {"n_estimators": 10}

    def _make(self) -> Any:
        p = self.params
        return RandomForestClassifier(
            n_estimators=int(p["n_estimators"]),
            max_depth=p["max_depth"],
            min_samples_leaf=int(p["min_samples_leaf"]),
            class_weight="balanced_subsample",
            n_jobs=int(p["n_jobs"]),
            random_state=int(p["seed"]),
        )

    def _after_fit(self) -> None:
        # Single-row serving latency: thread fan-out costs more than it saves.
        self._estimator.n_jobs = 1


PLUGINS: list[type[SupervisedPlugin]] = [
    MlpBackpropPlugin,
    LogisticRegressionPlugin,
    GradientBoostingPlugin,
    RandomForestPlugin,
]
