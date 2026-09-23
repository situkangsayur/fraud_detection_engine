# llm-service — multi-stage build with uv; runtime image is slim, non-root, no build tooling.
# Works with the classic builder and BuildKit (no BuildKit-only syntax such as RUN --mount).

FROM python:3.12-slim AS builder
COPY --from=ghcr.io/astral-sh/uv:0.11 /uv /uvx /bin/
ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PYTHON_DOWNLOADS=never
WORKDIR /app
# 1) dependencies only (cached layer as long as the lockfile does not change)
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project --no-cache
# 2) the service itself, installed non-editable into the venv
COPY README.md ./
COPY src ./src
RUN uv sync --frozen --no-dev --no-editable --no-cache

FROM python:3.12-slim AS runtime
LABEL org.opencontainers.image.title="llm-service" \
      org.opencontainers.image.authors="Hendri Karisma <situkangsayur@gmail.com>"
RUN groupadd --system --gid 10001 app \
 && useradd --system --uid 10001 --gid app --home-dir /nonexistent --shell /usr/sbin/nologin app \
 && mkdir -p /regulations && chown app:app /regulations
COPY --from=builder --chown=app:app /app/.venv /app/.venv
ENV PATH="/app/.venv/bin:$PATH" \
    PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    REGULATION_DIR=/regulations
USER app
WORKDIR /app
EXPOSE 8002
HEALTHCHECK --interval=10s --timeout=5s --start-period=20s --retries=12 \
  CMD ["python", "-c", "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8002/health/live', timeout=3)"]
CMD ["uvicorn", "llm_service.main:app", "--host", "0.0.0.0", "--port", "8002", "--proxy-headers", "--no-server-header"]
