# External ML algorithm plugins

Drop plugin modules (`*.py` or packages) here. Each module must expose `PLUGINS = [MyPluginClass, ...]`.
The folder is mounted read-only into ml-service at `/plugins`. Load them without restarting:

    POST /api/v1/ml/algorithms/reload      (platform admin)

The contract, validation and smoke test are described in `docs/technical/ml-plugins.md`.
An example lives in `plugins/example_knn_anomaly.py`.
