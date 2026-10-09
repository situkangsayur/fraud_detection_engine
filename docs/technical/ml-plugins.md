# ML algorithm plugins

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

    def __init__(self, params: dict) -> None: ...
    @abstractmethod
    def save(self, directory: Path) -> None: ...
    @classmethod
    @abstractmethod
    def load(cls, directory: Path) -> "AlgorithmPlugin": ...

class SupervisedPlugin(AlgorithmPlugin):
    kind = "supervised"
    @abstractmethod
    def fit(self, X_train: np.ndarray, y_train: np.ndarray, X_val: np.ndarray, y_val: np.ndarray,
            feature_names: list[str], progress: ProgressCallback) -> dict: ...   # returns training history
    @abstractmethod
    def predict_proba(self, X: np.ndarray) -> np.ndarray: ...                  # P(fraud), shape (n,)
    def explain(self, X: np.ndarray, feature_names: list[str], top_k: int = 5) -> list[list[tuple[str, float]]]:
        ...  # optional; the default implementation uses permutation-based local sensitivity

class AnomalyPlugin(AlgorithmPlugin):
    kind = "anomaly"
    @abstractmethod
    def fit(self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback) -> dict: ...
    @abstractmethod
    def score(self, X: np.ndarray) -> np.ndarray: ...     # anomaly score in [0,1], higher = more anomalous

class ClusteringPlugin(AlgorithmPlugin):
    kind = "clustering"
    supports_predict: ClassVar[bool]                      # False → nearest-centroid assignment is used
    @abstractmethod
    def fit(self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback) -> dict: ...
    @abstractmethod
    def labels(self) -> np.ndarray: ...                   # training labels, -1 = noise
    def predict(self, X: np.ndarray) -> np.ndarray: ...
```

The platform owns everything around the plugin: data loading per project (tenant-scoped session), the feature
pipeline (imputation, scaling, one-hot; persisted with the model), time-based train/validation split, class
imbalance reporting, metrics, artifact storage, the model registry, activation and serving. Plugins only see numpy
arrays. **A plugin never touches the database or the network.**

## 2. Built-in plugins

| name | kind | notes |
|---|---|---|
| `mlp_backprop` | supervised | PyTorch MLP trained with backpropagation: configurable hidden layers, ReLU, dropout, batch-norm, Adam, BCE with `pos_weight` for imbalance, early stopping on validation PR-AUC; explain = gradient × input |
| `logistic_regression` | supervised | scikit-learn, class_weight balanced; baseline |
| `gradient_boosting` | supervised | scikit-learn HistGradientBoostingClassifier |
| `random_forest` | supervised | scikit-learn |
| `isolation_forest` | anomaly | scores min-max normalised on training distribution |
| `local_outlier_factor` | anomaly | novelty=True |
| `autoencoder` | anomaly | PyTorch; reconstruction error percentile |
| `hdbscan` | clustering | scikit-learn HDBSCAN; noise = -1; predict via nearest exemplar |
| `kmeans` | clustering | scikit-learn |
| `dbscan` | clustering | scikit-learn |
| `gaussian_mixture` | clustering | scikit-learn, BIC-selected components optional |

## 3. Discovery & hot loading

* Built-ins live in `ml_service/plugins/builtin/`.
* **External plugins:** Python files or packages dropped into `PLUGIN_DIR` (a docker volume `ml_plugins`, default
  `/plugins`), each exposing `PLUGINS = [MyPlugin, ...]` in the module. Optional `requirements.txt` per plugin
  package is **not** auto-installed (supply chain risk). Dependencies must already exist in the image, or the plugin must
  vendor pure-Python code.
* On startup, and on `POST /v1/algorithms/reload` (platform admin via gateway, or INT), ml-service imports all
  plugin modules and validates each class: the interface is implemented, `param_schema` is valid JSON Schema, and a
  smoke test with a tiny synthetic dataset passes (fit, predict/score, save, load round-trip). Valid plugins are upserted
  into `ml.algorithms (name, kind, version, display_name, description, param_schema, source ('builtin'|'plugin'),
  module, status ('available'|'invalid'|'disabled'), error, loaded_at)`.
* Invalid plugins are recorded with `status='invalid'` and the error, and are never selectable.
* Model artifacts record `algorithm name + version`. If a plugin disappears, models that use it cannot be activated
  and existing active models keep serving from their already-loaded instance until restart.
* Uploading plugins from the UI is **not** supported in v1 (it would be remote code execution). This is on the backlog
  as "signed plugin packages".

## 4. Training & serving flow

1. `POST /api/v1/projects/{pid}/ml/supervised/train` with an optional `{algorithm?, params?}` override
   (default: the project `ml_config`). This creates an `ml.models` row (`status=training`) and starts a background job.
2. The job loads `core.events` + `core.event_features` + `core.event_labels` for the project (only labelled events for
   supervised), builds the feature matrix per `ml_config.features`, does a time-based 80/20 split, calls
   `fit`, computes metrics (ROC-AUC, PR-AUC, precision/recall/F1 at thresholds 0.3/0.5/0.7, confusion matrix,
   calibration bins, global permutation importance), saves artifacts to
   `MODEL_DIR/<tenant>/<project>/<model_id>/`, and sets `status=ready`.
3. The model goes through maker–checker to become `active` (one active per project and kind). ml-service
   hot-swaps the in-memory model for that project.
4. Unsupervised training fits the anomaly plugin and the clustering plugin on the same matrix (last `since_days`),
   stores `ml.event_anomaly` (score, cluster, PCA x/y) and `ml.clusters` (size, fraud rate from labels, profile,
   top distinguishing features by standardized mean difference).
5. `POST /v1/projects/{pid}/supervised/predict` and `/unsupervised/score` serve the active models. An LRU keeps loaded
   models per project in memory.
6. **Graph communities:** `POST /v1/projects/{pid}/graph/communities/recompute` pulls the project graph export from
   graph-service, runs Louvain (networkx), and writes `ml.graph_communities` + `ml.graph_community_stats`.
   graph-service reads the community fraud rate for the `community_fraud_rate` metric.
