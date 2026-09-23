# service_stats.py
from app.db.mongo import db
from app.models.base import ResponseModel

async def get_users_statistics_service():
    users = await db["users"].find().to_list(length=1000)
    result = []
    for user in users:
        trxs = await db["transactions"].find({"id_user": user["id_user"]}).to_list(length=100)
        avg_risk = sum([t.get("risk_score", 0) for t in trxs]) / len(trxs) if trxs else 0
        result.append({"id_user": user["id_user"], "avg_risk": avg_risk})
    return ResponseModel(success=True, message="User stats", data=result)

async def get_user_statistics_service(id_user: str):
    trxs = await db["transactions"].find({"id_user": id_user}).to_list(length=100)
    avg_risk = sum([t.get("risk_score", 0) for t in trxs]) / len(trxs) if trxs else 0
    return ResponseModel(success=True, message="Single user stats", data={"id_user": id_user, "avg_risk": avg_risk})

async def get_transactions_statistics_service():
    trxs = await db["transactions"].find().to_list(length=1000)
    result = {
        "total_transactions": len(trxs),
        "avg_risk": sum([t.get("risk_score", 0) for t in trxs]) / len(trxs) if trxs else 0
    }
    return ResponseModel(success=True, message="Transaction stats", data=result)

async def get_policies_performance_service():
    policies = await db["policies"].find().to_list(length=1000)
    result = []
    for policy in policies:
        rule_ids = [r.get("_id") for r in policy["rules"]]
        matched = await db["transactions"].count_documents({"matched_rules": {"$in": rule_ids}})
        result.append({
            "policy_id": policy["policy_id"],
            "name": policy["name"],
            "total_matched_transactions": matched
        })
    return ResponseModel(success=True, message="Policies performance", data=result)

async def get_rules_performance_service():
    rules = await db["rules"].find().to_list(length=1000)
    result = []
    for rule in rules:
        matched = await db["transactions"].count_documents({"matched_rules": rule["_id"]})
        result.append({
            "rule_id": str(rule["_id"]),
            "description": rule["description"],
            "times_matched": matched,
            "average_risk_point": rule.get("risk_point", 0)
        })
    return ResponseModel(success=True, message="Rules performance", data=result)
