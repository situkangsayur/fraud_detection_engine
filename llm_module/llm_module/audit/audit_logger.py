import os
from pymongo import MongoClient
from datetime import datetime


def log_audit_entry(action_type: str, details: dict):
    mongo_uri = os.getenv("MONGO_URI", "mongodb://localhost:27017")
    db_name = os.getenv("MONGO_DB_NAME", "fraud_detection")
    client = MongoClient(mongo_uri)
    audit_collection = client[db_name]["audit_trail"]

    entry = {
        "timestamp": datetime.utcnow(),
        "action_type": action_type,
        "details": details,
    }
    audit_collection.insert_one(entry)
    print(f"✅ Audit log created: {action_type}")
