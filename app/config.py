# config.py
from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    MONGO_URI: str = "mongodb://localhost:27017"
    MONGO_DB_NAME: str = "fraud_detection"
    USE_MOCK: bool = False

    class Config:
        env_file = ".env"


def get_settings():
    return Settings()
