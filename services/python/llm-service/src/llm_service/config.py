"""Service configuration (12-factor: environment variables only)."""

from __future__ import annotations

from functools import lru_cache

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=None, extra="ignore")

    database_url: str = "postgresql+psycopg://llm_service:llm@localhost:5433/fraud"
    internal_api_token: str = Field(default="dev-internal-token", min_length=8)
    jwt_secret: str = Field(default="dev-jwt-secret-change-me-32-chars!!", min_length=16)
    jwt_algorithm: str = "HS256"

    core_api_url: str = "http://core-api:8080"
    rule_service_url: str = "http://rule-service:8081"
    graph_service_url: str = "http://graph-service:8082"
    ml_service_url: str = "http://ml-service:8001"

    ollama_url: str = "http://ollama:11434"
    ollama_chat_model: str = "qwen2.5:7b-instruct"
    ollama_embed_model: str = "bge-m3"
    ollama_timeout_s: float = 180.0
    ollama_num_ctx: int = 8192

    opensearch_url: str = "http://opensearch:9200"
    opensearch_index_prefix: str = "reg-chunks-"

    regulation_dir: str = "/regulations"
    max_upload_mb: int = 50

    chat_max_tool_iterations: int = 6
    tool_timeout_s: float = 20.0
    rule_repair_attempts: int = 2
    retrieval_k: int = 6
    embed_batch_size: int = 16

    log_level: str = "info"


@lru_cache
def get_settings() -> Settings:
    return Settings()
