# service_rule.py
from app.db.mongo import db
from app.models.base import StandardRule, VelocityRule, ResponseModel
from bson import ObjectId

collection = db["rules"]

async def create_standard_rule_service(rule: StandardRule):
    result = await collection.insert_one(rule.dict())
    return ResponseModel(success=True, message="Standard Rule created", data={"_id": str(result.inserted_id)})

async def create_velocity_rule_service(rule: VelocityRule):
    result = await collection.insert_one(rule.dict())
    return ResponseModel(success=True, message="Velocity Rule created", data={"_id": str(result.inserted_id)})

async def list_rules_service():
    rules = await collection.find().to_list(length=100)
    for rule in rules:
        rule["_id"] = str(rule["_id"])
    return ResponseModel(success=True, message="Rules list", data=rules)

async def get_rule_service(rule_id: str):
    rule = await collection.find_one({"_id": ObjectId(rule_id)})
    if rule:
        rule["_id"] = str(rule["_id"])
        return ResponseModel(success=True, message="Rule fetched", data=rule)
    return ResponseModel(success=False, message="Rule not found")

async def delete_rule_service(rule_id: str):
    result = await collection.delete_one({"_id": ObjectId(rule_id)})
    if result.deleted_count:
        return ResponseModel(success=True, message="Rule deleted")
    return ResponseModel(success=False, message="Rule not found")
