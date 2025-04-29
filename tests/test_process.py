import pytest
from httpx import AsyncClient
from app.main import app

@pytest.mark.asyncio
async def test_process_transaction():
    async with AsyncClient(app=app, base_url="http://test") as ac:
        # Setup: Create User
        user_payload = {
            "id_user": "proc_test_user",
            "nama_lengkap": "Processor",
            "email": "proc@example.com",
            "domain_email": "example.com",
            "address": "Test",
            "address_zip": "12345",
            "address_city": "Jakarta",
            "address_province": "DKI",
            "address_kecamatan": "Menteng",
            "phone_number": "08123456789"
        }
        await ac.post("/api/v1/user/", json=user_payload)

        # Setup: Create Rule (High Amount)
        rule_payload = {
            "description": "High Amount",
            "risk_point": 90,
            "rule_type": "standard",
            "field": "amount",
            "operator": ">",
            "value": 100000
        }
        rule_res = await ac.post("/api/v1/rule/standard", json=rule_payload)
        rule_id = rule_res.json()["data"]["_id"]

        # Setup: Create Policy
        policy_payload = {
            "policy_id": "proc_test_policy",
            "name": "Policy Test",
            "description": "Policy Desc",
            "rules": [rule_payload]
        }
        await ac.post("/api/v1/policy/", json=policy_payload)

        # Setup: Create Transaction
        trx_payload = {
            "id_transaction": "trx_proc_001",
            "id_user": "proc_test_user",
            "shipzip": "40000",
            "shipping_address": "Test",
            "shipping_city": "Jakarta",
            "shipping_province": "DKI Jakarta",
            "shipping_kecamatan": "Menteng",
            "payment_type": "credit_card",
            "number": "1234567890",
            "bank_name": "BCA",
            "amount": 150000.0,  # > rule threshold
            "status": "success",
            "billing_address": "Billing",
            "billing_city": "Jakarta",
            "billing_province": "DKI",
            "billing_kecamatan": "Menteng",
            "list_of_items": []
        }
        await ac.post("/api/v1/transaction/", json=trx_payload)

        # Process Transaction
        r = await ac.post("/api/v1/process/transaction", json={"id_transaction": "trx_proc_001"})
        assert r.status_code == 200
        body = r.json()["data"]
        assert body["risk_score"] >= 90
        assert body["detected_status"] == "fraud"
        assert len(body["matched_rules"]) > 0
