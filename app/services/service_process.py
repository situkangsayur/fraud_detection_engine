# service_process.py
from app.db.mongo import db
from app.models.base import ResponseModel
from app.services.service_policy import get_policy_service
from datetime import datetime, timedelta


async def process_transaction_service(id_transaction: str):
    trx = await db["transactions"].find_one({"id_transaction": id_transaction})
    if not trx:
        return ResponseModel(success=False, message="Transaction not found", data=None)

    policies = db["policies"].find()
    rules = await db["rules"].find().to_list(length=1000)
    risk_score = 0
    matched_rule_ids = []

    for rule in rules:
        if rule["rule_type"] == "standard":
            field_value = trx.get(rule["field"])
            if rule["operator"] == ">" and field_value > rule["value"]:
                risk_score += rule["risk_point"]
                matched_rule_ids.append(rule["_id"])
        elif rule["rule_type"] == "velocity":
            # implement logic aggregasi transaksi (mocked here)
            risk_score += rule["risk_point"]
            matched_rule_ids.append(rule["_id"])

    status = "normal"
    if risk_score > 85:
        status = "fraud"
    elif risk_score >= 40:
        status = "suspect"

    await db["transactions"].update_one(
        {"id_transaction": id_transaction},
        {
            "$set": {
                "risk_score": risk_score,
                "detected_status": status,
                "matched_rules": matched_rule_ids,
            }
        },
    )

    return ResponseModel(
        success=True,
        message="Transaction evaluated",
        data={
            "risk_score": risk_score,
            "detected_status": status,
            "matched_rules": [str(rule_id) for rule_id in matched_rule_ids],
        },
    )
