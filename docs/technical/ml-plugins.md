# ML algorithm plugins

> Document status: verified against commit 07ee765 on 2026-10-13

Goal: add supervised or unsupervised algorithms **on the fly**, without rebuilding images or changing platform code.
Each project picks the algorithms it uses from the available catalogue (`core.projects.ml_config`).

## 1. Plugin kinds & interfaces (`ml_service.plugins.base`)

```python
class AlgorithmPlugin(ABC):
    name: ClassVar[str]              # unique, ^[a-z][a-z0-9_]{2,40}$
    kind: ClassVar[Literal["supervised", "anomaly", "clustering"]]
    version: ClassVar[str]           # semver of the plugin
    display_name: ClassVar[str]
    description: ClassVar[str]
    param_schema: ClassVar[dict]     # JSON Schema (draft 2020-12) of hyper-parameters, with defaults
                                     # → the UI renders the form automatically
    smoke_params: ClassVar[dict] = {}  # param overrides used only by the registry smoke test (e.g. fewer epochs)

    def __init__(self, params: dict | None = None) -> None: ...  # merges schema defaults + params, validates → self.params
    @abstractmethod
    def save(self, directory: Path) -> None: ...
    @classmethod
    @abstractmethod
    def load(cls, directory: Path) -> "AlgorithmPlugin": ...

class SupervisedPlugin(AlgorithmPlugin):
    kind = "supervised"
    @abstractmethod
    def fit(self, X_train: np.ndarray, y_train: np.ndarray, X_val: np.ndarray, y_val: np.ndarray,
            feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS) -> dict: ...  # JSON training history
    @abstractmethod
    def predict_proba(self, X: np.ndarray) -> np.ndarray: ...                  # P(fraud), shape (n,)
    def explain(self, X: np.ndarray, feature_names: list[str], top_k: int = 5) -> list[list[tuple[str, float]]]:
        ...  # optional; default = local sensitivity: set each feature to 0 (= training mean after scaling)
             # and rank features by |Δ P(fraud)|

class AnomalyPlugin(AlgorithmPlugin):
    kind = "anomaly"
    @abstractmethod
    def fit(self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS) -> dict: ...
    @abstractmethod
    def score(self, X: np.ndarray) -> np.ndarray: ...     # anomaly score in [0,1], higher = more anomalous

class ClusteringPlugin(AlgorithmPlugin):
    kind = "clustering"
    supports_predict: ClassVar[bool] = False              # False → platform nearest-centroid assignment is used
    @abstractmethod
    def fit(self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS) -> dict: ...
    @abstractmethod
    def labels(self) -> np.ndarray: ...                   # training labels, -1 = noise
    def predict(self, X: np.ndarray) -> np.ndarray: ...   # only when supports_predict = True
```

`ml_service.plugins.base` also provides `EmpiricalCdf`, the helper every built-in anomaly plugin (and the example
plugin) uses to map raw scores to [0, 1] by rank within the training distribution (0.99 = more anomalous than 99 % of
training rows).

The platform owns everything around the plugin: data loading per project (tenant-scoped session), the feature
pipeline (imputation, scaling, one-hot; persisted with the model), time-based train/validation split, class
imbalance reporting, metrics, artifact storage, the model registry, activation and serving. Plugins only see numpy
arrays. **A plugin never touches the database or the network.**

## 2. Built-in plugins

Registered in `ml_service/plugins/builtin/__init__.py` (`BUILTIN_PLUGINS`), all at version `1.0.0`:

| name | kind | notes |
|---|---|---|
| `mlp_backprop` | supervised | PyTorch MLP trained with backpropagation: hidden layers (default `[64, 32]`), ReLU, dropout 0.2, batch-norm, Adam (lr 1e-3, weight decay 1e-5), BCE-with-logits with `pos_weight` (`auto` = negatives/positives, capped at 1000), up to 50 epochs, early stopping (patience 5) on validation PR-AUC with validation loss as tie-breaker; batch size shrinks on small datasets (≥ ~10 steps/epoch); explain = gradient × input |
| `logistic_regression` | supervised | scikit-learn baseline, `class_weight` balanced by default (configurable); explain = coefficient × feature value |
| `gradient_boosting` | supervised | scikit-learn `HistGradientBoostingClassifier`, `class_weight` balanced by default; explain = default zero-out sensitivity |
| `random_forest` | supervised | scikit-learn `RandomForestClassifier`, `class_weight="balanced_subsample"`; `n_jobs` reset to 1 after fit for single-row latency |
| `isolation_forest` | anomaly | raw score = −`score_samples`, rank-normalised with `EmpiricalCdf` of the training scores |
| `local_outlier_factor` | anomaly | `novelty=True`; `n_neighbors` capped at n−1; `EmpiricalCdf`-normalised |
| `autoencoder` | anomaly | PyTorch; per-row mean squared reconstruction error, `EmpiricalCdf`-normalised (percentile rank) |
| `hdbscan` | clustering | scikit-learn `HDBSCAN`; noise = −1; no `predict` → serving uses nearest-centroid assignment (§4) |
| `kmeans` | clustering | scikit-learn; `supports_predict = True` |
| `dbscan` | clustering | scikit-learn; noise = −1; no `predict` → nearest-centroid assignment |
| `gaussian_mixture` | clustering | scikit-learn, fixed `n_components` (default 8, `diag` covariance); hard assignment = most likely component; `supports_predict = True` |

Example external plugin: `plugins/example_knn_anomaly.py` (`knn_distance_anomaly`, anomaly: mean distance to the k
nearest training neighbours, `EmpiricalCdf`-normalised).

## 3. Discovery & hot loading

* Built-ins live in `ml_service/plugins/builtin/`.
* **External plugins:** Python files (`*.py`) or packages (folder with `__init__.py`) in `PLUGIN_DIR` (default
  `/plugins`; in `docker-compose.yml` the repo folder `./plugins` is bind-mounted read-only), each exposing
  `PLUGINS = [MyPlugin, ...]`. Names starting with `_` or `.` are ignored. Plugin dependencies are **not**
  installed (supply chain risk): they must already exist in the image, or the plugin must vendor pure-Python code.
* On startup, and on `POST /api/v1/ml/algorithms/reload` (platform admin, or INT token), ml-service (re)imports all
  plugin modules and validates each class: it subclasses the base for its `kind`, has no abstract methods left,
  `name` matches `^[a-z][a-z0-9_]{2,40}$`, `version` is non-empty, `param_schema` is valid JSON Schema and its
  defaults validate. External plugins must also pass a smoke test on a tiny synthetic dataset (fit → predict/score in
  [0, 1] → save → load → identical outputs). Built-ins are smoke-tested only when `SMOKE_TEST_BUILTINS=true` (default
  `false`; the test suite covers them).
* Results are upserted into `ml.algorithms (name, kind, version, display_name, description, param_schema,
  source ('builtin'|'plugin'), module, status ('available'|'invalid'|'disabled'), error, loaded_at)`. A row an
  operator set to `disabled` stays disabled across reloads.
* Invalid plugin classes are stored with `status='invalid'` and the error, and are never selectable. Module-level
  failures (import error, missing/empty `PLUGINS`, invalid name, name clashing with a built-in) cannot be stored in
  `ml.algorithms`; they are returned in the reload response (`{loaded, invalid: [{module, name, error}]}`) and logged.
* `GET /api/v1/ml/algorithms[?kind=]` lists the catalogue with schema defaults. Project `ml_config` is validated
  against the registry by `POST /v1/algorithms/validate-config` (INT; used by core-api).
* Model artifacts record `algorithm name + version`. A reload clears the in-memory model cache, so models are
  rebuilt from the reloaded classes. If a plugin disappeared, its models cannot be approved/loaded and requests
  for that project fail with `503 model_unavailable` until another model is activated.
* Uploading plugins from the UI is **not** supported in v1 (it would be remote code execution). This is on the backlog
  as "signed plugin packages".

## 4. Training & serving flow

1. `POST /api/v1/projects/{pid}/ml/supervised/train` (analyst) with optional `{algorithm?, params?, since_days?,
   label_maturity_days?}` (default: the project `ml_config`; `label_maturity_days` defaults to 14, `0`/`null` =
   labelled events only). This creates an `ml.models` row (`status=training`) and starts a background job
   (`TRAINING_WORKERS`, default 2).
2. The job loads `core.events` + `core.event_features` + latest `core.event_labels` for the project, oldest → newest,
   at most `MAX_TRAINING_ROWS` (200 000) most recent rows: labelled events plus unlabelled events older than
   `label_maturity_days` (counted as legit). It needs ≥ 20 rows with ≥ 2 per class. It builds the feature matrix per
   `ml_config.features` (feature-catalog.md §4) with the pipeline fitted on the first 80 % of rows only, does a time-based
   80/20 split (stratified fallback when one side lacks a class; recorded as `metrics.split`), calls `fit`, and
   computes metrics on the validation part (`training/metrics.py`):
   * `roc_auc`, `pr_auc` (average precision);
   * `thresholds`: precision, recall, F1 and flagged rate at 0.3 / 0.5 / 0.7;
   * `confusion_matrix` (tp/fp/tn/fn) at 0.5; `calibration`: 10 equal-width bins (mean predicted vs observed rate);
   * `feature_importance`: permutation importance = drop in PR-AUC when a column is shuffled (≤ 2 000 validation
     rows, top 25);
   * `class_balance` (fraud, legit, fraud rate, implicit legit, label maturity) and train/validation sizes.

   Artifacts (`pipeline.joblib`, `supervised/`, `meta.json`) are saved to `MODEL_DIR/<tenant>/<project>/<model_id>/`
   and the model becomes `status=ready`.
3. Maker–checker: `POST …/ml/models/{id}/submit` (analyst, `ready` → `pending_approval`), then `…/approve` (approver,
   who must differ from the submitter, else `403 four_eyes_violation`) or `…/reject` (back to `ready`). Approval loads
   the artifact first (an unloadable model never becomes active), archives the previous active model of that kind and
   hot-swaps the in-memory model. One active model per project and kind.
4. `POST /api/v1/projects/{pid}/ml/unsupervised/train` (analyst; `since_days` default 90) fits the anomaly plugin
   and the clustering plugin on the same matrix (≤ `MAX_UNSUPERVISED_ROWS` = 50 000 rows, ≥ 20 rows, labelled or
   not). For clusterers without `predict` it fits a nearest-centroid assigner (a point farther than the cluster's
   95th-percentile radius gets −1). It stores `ml.event_anomaly` (score, cluster, PCA x/y) and `ml.clusters` (size,
   labelled count, fraud rate from labels, centroid, raw-feature profile, top 5 features by standardized mean
   difference). Metrics: `n_rows`, `n_clusters`, `noise_ratio`, anomaly score quantiles (0.5/0.9/0.95/0.99),
   `anomaly_roc_auc_vs_labels` (labelled rows only) and `silhouette` (≤ 2 000 sampled non-noise rows).
   Analyst views: `GET …/ml/unsupervised/{clusters,projection,anomalies}`, `PATCH …/clusters/{model_id}/{cluster_id}`
   (label/notes).
5. Internal serving (INT, called by core-api): `POST /v1/projects/{pid}/supervised/predict` (probability + top-5
   explanation), `/supervised/predict/batch` (≤ 5 000 items) and `/unsupervised/score` (anomaly score, cluster id,
   cluster fraud rate). An LRU (`MODEL_CACHE_SIZE`, 64) keeps loaded models per (project, kind); entries re-check the
   active model every `MODEL_REFRESH_SECONDS` (60) for multi-replica safety, and "no active model" is cached for 30 s
   (`404 no_active_model`).
6. **Graph communities:** `POST /api/v1/projects/{pid}/ml/graph-communities/recompute` (analyst) pulls the project's
   customer–customer edge export from graph-service (`GET /v1/projects/{pid}/export`, NDJSON), runs Louvain
   (networkx, weighted, seed 42) and replaces `ml.graph_communities` + `ml.graph_community_stats` (size, fraud count,
   fraud rate). `GET …/ml/graph-communities` lists them. graph-service reads the community fraud rate for the
   `community_fraud_rate` metric.
