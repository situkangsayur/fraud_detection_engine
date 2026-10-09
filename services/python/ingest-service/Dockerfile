# ---- build: resolve deps with uv into a venv ----
FROM python:3.12-slim AS build
COPY --from=ghcr.io/astral-sh/uv:0.11 /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PYTHON_DOWNLOADS=never UV_NO_CACHE=1
WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project
COPY app ./app
RUN uv sync --frozen --no-dev

# ---- runtime: slim, non-root ----
FROM python:3.12-slim AS runtime
LABEL org.opencontainers.image.title="ingest-service" \
      org.opencontainers.image.authors="Hendri Karisma <situkangsayur@gmail.com>"
ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1 PATH="/app/.venv/bin:$PATH" UPLOAD_DIR=/uploads
RUN groupadd --system --gid 10001 app && useradd --system --uid 10001 --gid app --no-create-home app \
    && mkdir -p /uploads && chown app:app /uploads
WORKDIR /app
COPY --from=build --chown=app:app /app /app
USER app
EXPOSE 8003
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8003", "--proxy-headers", "--no-server-header", "--log-level", "warning"]
