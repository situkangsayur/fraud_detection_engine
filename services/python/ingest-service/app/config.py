"""Service configuration (12-factor: environment variables only)."""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=None, extra="ignore")

    database_url: str = "postgresql+psycopg://ingest_service:ingest@localhost:5433/fraud"
    core_api_url: str = "http://core-api:8080"
    llm_service_url: str = "http://llm-service:8002"
    internal_api_token: str = Field(default="dev-internal-token", min_length=8)
    jwt_secret: str = Field(default="dev-jwt-secret-change-me-32-chars!!", min_length=16)
    jwt_algorithm: str = "HS256"
    upload_dir: Path = Path("/uploads")
    max_upload_mb: int = 200
    upload_ttl_hours: int = 24
    log_level: str = "info"

    batch_size: int = 500
    # Files up to this many rows are loaded fully so they can be sorted by occurred_at before sending
    # (velocity features need chronological order). Bigger files stream in file order.
    sort_max_rows: int = 2_000_000
    core_timeout_s: float = 120.0
    core_max_retries: int = 5
    preview_rows: int = 20
    inspect_sample_rows: int = 1000
    connector_poll_enabled: bool = True
    connector_tick_s: float = 5.0


@lru_cache
def get_settings() -> Settings:
    return Settings()
