FROM python:3.12-slim AS build
COPY --from=ghcr.io/astral-sh/uv:0.11 /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PYTHON_DOWNLOADS=never UV_NO_CACHE=1
WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project
COPY simulator ./simulator
RUN uv sync --frozen --no-dev

FROM python:3.12-slim
LABEL org.opencontainers.image.title="fraud-simulator" \
      org.opencontainers.image.authors="Hendri Karisma <situkangsayur@gmail.com>"
ENV PYTHONUNBUFFERED=1 PATH="/app/.venv/bin:$PATH"
RUN useradd --system --uid 10001 --no-create-home sim
WORKDIR /app
COPY --from=build --chown=sim /app /app
USER sim
ENTRYPOINT ["python", "-m", "simulator"]
CMD ["run"]
