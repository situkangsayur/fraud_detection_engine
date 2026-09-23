# ---- build: resolve and install dependencies into a venv with uv (CPU-only PyTorch) ----
FROM python:3.12-slim AS build
COPY --from=ghcr.io/astral-sh/uv:0.11 /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PYTHON_DOWNLOADS=never
WORKDIR /app
COPY pyproject.toml uv.lock README.md ./
# deps layer first (cached until pyproject/uv.lock change); no BuildKit-only features → builds on any engine
RUN uv sync --frozen --no-dev --no-install-project && rm -rf /root/.cache/uv
COPY src ./src
RUN uv sync --frozen --no-dev && rm -rf /root/.cache/uv

# ---- runtime ----
FROM python:3.12-slim AS runtime
RUN groupadd --system app && useradd --system --gid app --home /app --shell /usr/sbin/nologin app \
    && mkdir -p /models /plugins && chown app:app /models
WORKDIR /app
COPY --from=build --chown=app:app /app /app
ENV PATH="/app/.venv/bin:$PATH" \
    PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    MODEL_DIR=/models \
    PLUGIN_DIR=/plugins \
    OMP_NUM_THREADS=2
USER app
EXPOSE 8001
# One worker: model cache, plugin registry and training executor are per-process state.
# Scale horizontally with more containers (activation re-check handles multi-replica caches).
CMD ["uvicorn", "ml_service.main:app", "--host", "0.0.0.0", "--port", "8001", "--workers", "1", \
     "--proxy-headers", "--no-server-header"]
