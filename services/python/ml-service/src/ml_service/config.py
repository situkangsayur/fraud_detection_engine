"""12-factor configuration (environment variables only)."""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=None, extra="ignore")

    database_url: str = Field(default="postgresql+psycopg://ml_service:ml@localhost:5433/fraud")
    database_pool_size: int = 10
    internal_api_token: str = Field(default="dev-internal-token")
    jwt_secret: str = Field(default="dev-jwt-secret")
    jwt_algorithm: str = "HS256"
    model_dir: Path = Path("/models")
    plugin_dir: Path = Path("/plugins")
    training_workers: int = 2
    graph_service_url: str = "http://graph-service:8082"
    log_level: str = "info"
    # Built-ins are covered by the test-suite; smoke-testing them on every start only slows boot.
    smoke_test_builtins: bool = False
    # Cache of loaded models per (project, kind).
    model_cache_size: int = 64
    # How often a cached model re-checks whether a newer model became active (multi-replica safety).
    model_refresh_seconds: int = 60
    no_model_negative_ttl_seconds: int = 30
    max_training_rows: int = 200_000
    max_unsupervised_rows: int = 50_000
    http_timeout_seconds: float = 30.0


@lru_cache(maxsize=1)
def get_settings() -> Settings:
    return Settings()
