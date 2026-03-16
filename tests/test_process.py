import pytest


@pytest.mark.asyncio
class TestProcessTransaction:
    async def _setup_full_scenario(self, client, user_payload, transaction_payload, standard_rule_payload):
        """Helper: create user, rule, policy, and transaction."""
        await client.post("/api/v1/user/", json=user_payload)
        rule_r = await client.post("/api/v1/rule/standard", json=standard_rule_payload)
        rule_id = rule_r.json()["data"]["_id"]
        policy = {
            "policy_id": "proc_policy",
            "name": "Process Policy",
            "description": "For process testing",
            "rules": [standard_rule_payload],
        }
        await client.post("/api/v1/policy/", json=policy)
        await client.post("/api/v1/transaction/", json=transaction_payload)
        return rule_id

    async def test_process_transaction_fraud(self, client, user_payload, transaction_payload, standard_rule_payload):
        """Transaction with amount > threshold should be flagged as fraud."""
        standard_rule_payload["risk_point"] = 90
        await self._setup_full_scenario(client, user_payload, transaction_payload, standard_rule_payload)

        r = await client.post(
            "/api/v1/process/transaction",
            json={"id_transaction": "trx_test_001"},
        )
        assert r.status_code == 200
        body = r.json()["data"]
        assert body["risk_score"] >= 90
        assert body["detected_status"] == "fraud"
        assert len(body["matched_rules"]) > 0

    async def test_process_transaction_normal(self, client, user_payload, standard_rule_payload):
        """Transaction with amount below threshold should be normal."""
        await client.post("/api/v1/user/", json=user_payload)
        standard_rule_payload["risk_point"] = 10
        standard_rule_payload["value"] = 500000
        await client.post("/api/v1/rule/standard", json=standard_rule_payload)

        trx = {
            "id_transaction": "trx_normal",
            "id_user": "test_user_001",
            "shipzip": "12345",
            "shipping_address": "Jl. Test",
            "shipping_city": "Jakarta",
            "shipping_province": "DKI",
            "shipping_kecamatan": "Menteng",
            "payment_type": "credit_card",
            "number": "1234567890",
            "bank_name": "BCA",
            "amount": 50000.0,
            "status": "success",
            "billing_address": "Jl. Billing",
            "billing_city": "Jakarta",
            "billing_province": "DKI",
            "billing_kecamatan": "Menteng",
            "list_of_items": [],
        }
        await client.post("/api/v1/transaction/", json=trx)
        r = await client.post(
            "/api/v1/process/transaction",
            json={"id_transaction": "trx_normal"},
        )
        assert r.status_code == 200
        body = r.json()["data"]
        assert body["detected_status"] == "normal"

    async def test_process_transaction_suspect(self, client, user_payload):
        """Transaction with moderate risk (40-85) should be suspect."""
        await client.post("/api/v1/user/", json=user_payload)

        rule = {
            "description": "Moderate amount",
            "risk_point": 50,
            "rule_type": "standard",
            "field": "amount",
            "operator": ">",
            "value": 10000,
        }
        await client.post("/api/v1/rule/standard", json=rule)

        trx = {
            "id_transaction": "trx_suspect",
            "id_user": "test_user_001",
            "shipzip": "12345",
            "shipping_address": "Jl. Test",
            "shipping_city": "Jakarta",
            "shipping_province": "DKI",
            "shipping_kecamatan": "Menteng",
            "payment_type": "credit_card",
            "number": "1234567890",
            "bank_name": "BCA",
            "amount": 50000.0,
            "status": "success",
            "billing_address": "Jl. Billing",
            "billing_city": "Jakarta",
            "billing_province": "DKI",
            "billing_kecamatan": "Menteng",
            "list_of_items": [],
        }
        await client.post("/api/v1/transaction/", json=trx)
        r = await client.post(
            "/api/v1/process/transaction",
            json={"id_transaction": "trx_suspect"},
        )
        assert r.status_code == 200
        body = r.json()["data"]
        assert body["detected_status"] == "suspect"

    async def test_process_transaction_not_found(self, client):
        r = await client.post(
            "/api/v1/process/transaction",
            json={"id_transaction": "nonexistent"},
        )
        assert r.status_code == 200
        assert r.json()["success"] is False
        assert r.json()["message"] == "Transaction not found"

    async def test_process_with_velocity_rule(self, client, user_payload):
        """Velocity rules always match (mocked), contributing risk points."""
        await client.post("/api/v1/user/", json=user_payload)
        velocity_rule = {
            "description": "High frequency",
            "risk_point": 45,
            "rule_type": "velocity",
            "field": "id_user",
            "time_range": "1h",
            "aggregation_function": "count",
            "threshold": 5,
        }
        await client.post("/api/v1/rule/velocity", json=velocity_rule)

        trx = {
            "id_transaction": "trx_velocity",
            "id_user": "test_user_001",
            "shipzip": "12345",
            "shipping_address": "Jl. Test",
            "shipping_city": "Jakarta",
            "shipping_province": "DKI",
            "shipping_kecamatan": "Menteng",
            "payment_type": "credit_card",
            "number": "1234567890",
            "bank_name": "BCA",
            "amount": 50000.0,
            "status": "success",
            "billing_address": "Jl. Billing",
            "billing_city": "Jakarta",
            "billing_province": "DKI",
            "billing_kecamatan": "Menteng",
            "list_of_items": [],
        }
        await client.post("/api/v1/transaction/", json=trx)
        r = await client.post(
            "/api/v1/process/transaction",
            json={"id_transaction": "trx_velocity"},
        )
        assert r.status_code == 200
        body = r.json()["data"]
        assert body["risk_score"] >= 45
        assert len(body["matched_rules"]) >= 1

    async def test_process_no_rules(self, client, user_payload):
        """Transaction with no rules should be normal with 0 risk."""
        await client.post("/api/v1/user/", json=user_payload)
        trx = {
            "id_transaction": "trx_no_rules",
            "id_user": "test_user_001",
            "shipzip": "12345",
            "shipping_address": "Jl. Test",
            "shipping_city": "Jakarta",
            "shipping_province": "DKI",
            "shipping_kecamatan": "Menteng",
            "payment_type": "credit_card",
            "number": "1234567890",
            "bank_name": "BCA",
            "amount": 50000.0,
            "status": "success",
            "billing_address": "Jl. Billing",
            "billing_city": "Jakarta",
            "billing_province": "DKI",
            "billing_kecamatan": "Menteng",
            "list_of_items": [],
        }
        await client.post("/api/v1/transaction/", json=trx)
        r = await client.post(
            "/api/v1/process/transaction",
            json={"id_transaction": "trx_no_rules"},
        )
        assert r.status_code == 200
        body = r.json()["data"]
        assert body["risk_score"] == 0
        assert body["detected_status"] == "normal"
        assert body["matched_rules"] == []
