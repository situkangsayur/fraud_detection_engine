# ml-service

Algorithm plugin registry, training jobs, model registry (maker–checker) and low-latency serving for the fraud
platform. Contracts: `docs/technical/ml-plugins.md`, `docs/technical/api-contract.md` §D,
`docs/technical/feature-catalog.md`, schema `db/migrations/0005_ml.sql`.

## Layout

```
src/ml_service/
  main.py            app factory, request-id + metrics middleware
  config.py          env settings (12-factor)
  auth.py            JWT (users) + internal token (services); project role checks
  db.py              SQLAlchemy engine, tenant_session() → SET app.tenant_id (RLS)
  repository.py      all SQL (project-scoped)
  ml_config.py       validation/resolution of a project's ml_config against the registry
  plugins/           base interfaces, registry (built-in + PLUGIN_DIR, hot reload), smoke test, built-ins
  features/          feature-catalog v1 + FeaturePipeline (impute, scale, one-hot, source.* extras)
  training/          supervised & unsupervised jobs, metrics, bounded job runner
  serving/           LRU cache of active models per project (hot swap on approval)
  graph/             Louvain communities over the graph-service export
  api/               routers (health, algorithms, models, inference, unsupervised, graph)
```

## Development

```bash
uv sync                      # Python 3.12, CPU-only PyTorch
uv run uvicorn ml_service.main:app --reload --port 8001
uv run ruff check src tests && uv run ruff format --check src tests && uv run mypy
uv run pytest                # unit + API tests
```

Database-backed tests (real Postgres, RLS active) run when both URLs are set:

```bash
# throwaway DB with the platform migrations (see db/tests/test_migrations.sh for the init + migrate steps)
export TEST_DATABASE_URL=postgresql+psycopg://ml_service:<pw>@127.0.0.1:5433/fraud
export TEST_ADMIN_DATABASE_URL=postgresql+psycopg://migrator:<pw>@127.0.0.1:5433/fraud
uv run pytest tests/test_db_integration.py
```

## Plugins

Drop a module exposing `PLUGINS = [...]` into `PLUGIN_DIR` (compose mounts `./plugins`), then
`POST /api/v1/ml/algorithms/reload` (platform admin). Each plugin is validated (interface, JSON-Schema params,
synthetic fit/predict/save/load smoke test) before it becomes selectable. See `plugins/example_knn_anomaly.py`.

## Operational notes

* One uvicorn worker per container: the model cache, plugin registry and training executor are process state.
  Scale with more containers. Cached models re-check the active model every `MODEL_REFRESH_SECONDS` (60).
* Training runs in-process (`TRAINING_WORKERS` threads). A model left in `training` by a restart is marked
  `failed` lazily ("interrupted by service restart"). Run training on a single replica.
* Artifacts: `MODEL_DIR/<tenant>/<project>/<model_id>/` (feature pipeline, plugin state, metadata).
