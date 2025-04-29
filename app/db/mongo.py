# db_mongo.py
import os
from motor.motor_asyncio import AsyncIOMotorClient
from app.config import get_settings

USE_MOCK = os.getenv("USE_MOCK", "false").lower() == "true"

import logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

logger.info(f"USE_MOCK: {USE_MOCK}")

if USE_MOCK:
    import mongomock_motor
    logger.info("Using mongomock_motor client")
    client = mongomock_motor.AsyncMongoMockClient()
else:
    settings = get_settings()
    logger.info(f"Using AsyncIOMotorClient with MONGO_URI: {settings.MONGO_URI}")
    client = AsyncIOMotorClient(settings.MONGO_URI)

db = client[os.getenv("MONGO_DB_NAME", "fraud_detection")]
logger.info(f"Using database: {os.getenv('MONGO_DB_NAME', 'fraud_detection')}")
