# service_policy.py
from app.db.mongo import db
from app.models.base import Policy, ResponseModel

collection = db["policies"]


async def create_policy_service(policy: Policy):
    policy_data = policy.dict()
    # Pastikan rules diubah dari object ke dict
    policy_data["rules"] = [
        rule.dict() if hasattr(rule, "dict") else rule for rule in policy_data["rules"]
    ]
    await collection.insert_one(policy_data)
    return ResponseModel(success=True, message="Policy created", data=policy_data)


async def get_policy_service(policy_id: str):
    data = await collection.find_one({"policy_id": policy_id})
    if data:
        return ResponseModel(success=True, message="Policy fetched", data=data)
    return ResponseModel(success=False, message="Policy not found")


async def list_policies_service():
    policies = await collection.find().to_list(length=100)
    return ResponseModel(success=True, message="Policies list", data=policies)


async def update_policy_service(policy_id: str, policy: Policy):
    result = await collection.update_one(
        {"policy_id": policy_id}, {"$set": policy.dict()}
    )
    if result.modified_count:
        return ResponseModel(success=True, message="Policy updated", data=policy.dict())
    return ResponseModel(success=False, message="Policy not found or unchanged")


async def delete_policy_service(policy_id: str):
    result = await collection.delete_one({"policy_id": policy_id})
    if result.deleted_count:
        return ResponseModel(success=True, message="Policy deleted")
    return ResponseModel(success=False, message="Policy not found")
