import pytest


@pytest.mark.asyncio
class TestStatsAPI:
    async def _seed_data(self, client):
        """Seed user, transaction, rule, and policy for stats tests."""
        user = {
            "id_user": "stats_user",
            "nama_lengkap": "Stats User",
            "email": "stats@example.com",
            "domain_email": "example.com",
            "address": "Jl. Stats",
            "address_zip": "12345",
            "address_city": "Jakarta",
            "address_province": "DKI",
            "address_kecamatan": "Menteng",
            "phone_number": "081234567890",
        }
        await client.post("/api/v1/user/", json=user)

        rule = {
            "description": "Amount > 100000",
            "risk_point": 50,
            "rule_type": "standard",
            "field": "amount",
            "operator": ">",
            "value": 100000,
        }
        await client.post("/api/v1/rule/standard", json=rule)

        policy = {
            "policy_id": "stats_policy",
            "name": "Stats Policy",
            "description": "For stats testing",
            "rules": [rule],
        }
        await client.post("/api/v1/policy/", json=policy)

        trx = {
            "id_transaction": "trx_stats_001",
            "id_user": "stats_user",
            "shipzip": "12345",
            "shipping_address": "Jl. Test",
            "shipping_city": "Jakarta",
            "shipping_province": "DKI",
            "shipping_kecamatan": "Menteng",
            "payment_type": "credit_card",
            "number": "1234567890",
            "bank_name": "BCA",
            "amount": 200000.0,
            "status": "success",
            "billing_address": "Jl. Billing",
            "billing_city": "Jakarta",
            "billing_province": "DKI",
            "billing_kecamatan": "Menteng",
            "list_of_items": [],
        }
        await client.post("/api/v1/transaction/", json=trx)
        await client.post(
            "/api/v1/process/transaction",
            json={"id_transaction": "trx_stats_001"},
        )

    async def test_users_statistics(self, client):
        await self._seed_data(client)
        r = await client.get("/api/v1/stats/users")
        assert r.status_code == 200
        assert r.json()["success"] is True
        data = r.json()["data"]
        assert len(data) >= 1
        assert "avg_risk" in data[0]

    async def test_users_statistics_empty(self, client):
        r = await client.get("/api/v1/stats/users")
        assert r.status_code == 200
        assert r.json()["data"] == []

    async def test_single_user_statistics(self, client):
        await self._seed_data(client)
        r = await client.get("/api/v1/stats/user/stats_user")
        assert r.status_code == 200
        assert r.json()["data"]["id_user"] == "stats_user"
        assert r.json()["data"]["avg_risk"] >= 0

    async def test_single_user_statistics_no_transactions(self, client):
        r = await client.get("/api/v1/stats/user/no_user")
        assert r.status_code == 200
        assert r.json()["data"]["avg_risk"] == 0

    async def test_transactions_statistics(self, client):
        await self._seed_data(client)
        r = await client.get("/api/v1/stats/transactions")
        assert r.status_code == 200
        data = r.json()["data"]
        assert data["total_transactions"] >= 1
        assert "avg_risk" in data

    async def test_transactions_statistics_empty(self, client):
        r = await client.get("/api/v1/stats/transactions")
        assert r.status_code == 200
        data = r.json()["data"]
        assert data["total_transactions"] == 0
        assert data["avg_risk"] == 0

    async def test_policies_performance(self, client):
        await self._seed_data(client)
        r = await client.get("/api/v1/stats/policies-performance")
        assert r.status_code == 200
        data = r.json()["data"]
        assert len(data) >= 1
        assert "total_matched_transactions" in data[0]

    async def test_policies_performance_empty(self, client):
        r = await client.get("/api/v1/stats/policies-performance")
        assert r.status_code == 200
        assert r.json()["data"] == []

    async def test_rules_performance(self, client):
        await self._seed_data(client)
        r = await client.get("/api/v1/stats/rules-performance")
        assert r.status_code == 200
        data = r.json()["data"]
        assert len(data) >= 1
        assert "times_matched" in data[0]
        assert "average_risk_point" in data[0]

    async def test_rules_performance_empty(self, client):
        r = await client.get("/api/v1/stats/rules-performance")
        assert r.status_code == 200
        assert r.json()["data"] == []
