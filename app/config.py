# config.py
from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    MONGO_URI: str = "mongodb://localhost:27017"
    MONGO_DB_NAME: str = "fraud_detection"
    USE_MOCK: bool = False

    # Tambahan agar .env ai
    llm_provider: str = "ollama"
    ollama_model: str = "deepseek-8b-instruct"
    ollama_base_url: str = "http://localhost:11434"
    openai_model: str = "gpt-4"
    openai_api_key: str = ""
    google_api_key: str = ""

    class Config:
        env_file = ".env"


def get_settings():
    return Settings()
