import pytest


@pytest.mark.asyncio
class TestRuleAPI:
    async def test_create_standard_rule(self, client, standard_rule_payload):
        r = await client.post("/api/v1/rule/standard", json=standard_rule_payload)
        assert r.status_code == 200
        assert r.json()["success"] is True
        assert r.json()["message"] == "Standard Rule created"
        assert "_id" in r.json()["data"]

    async def test_create_velocity_rule(self, client, velocity_rule_payload):
        r = await client.post("/api/v1/rule/velocity", json=velocity_rule_payload)
        assert r.status_code == 200
        assert r.json()["success"] is True
        assert r.json()["message"] == "Velocity Rule created"

    async def test_get_rule(self, client, standard_rule_payload):
        create_r = await client.post("/api/v1/rule/standard", json=standard_rule_payload)
        rule_id = create_r.json()["data"]["_id"]
        r = await client.get(f"/api/v1/rule/{rule_id}")
        assert r.status_code == 200
        assert r.json()["data"]["description"] == "Amount > 100000"

    async def test_get_rule_not_found(self, client):
        r = await client.get("/api/v1/rule/000000000000000000000000")
        assert r.status_code == 200
        assert r.json()["success"] is False
        assert r.json()["message"] == "Rule not found"

    async def test_list_rules(self, client, standard_rule_payload, velocity_rule_payload):
        await client.post("/api/v1/rule/standard", json=standard_rule_payload)
        await client.post("/api/v1/rule/velocity", json=velocity_rule_payload)
        r = await client.get("/api/v1/rule/")
        assert r.status_code == 200
        assert len(r.json()["data"]) == 2

    async def test_list_rules_empty(self, client):
        r = await client.get("/api/v1/rule/")
        assert r.status_code == 200
        assert r.json()["data"] == []

    async def test_delete_rule(self, client, standard_rule_payload):
        create_r = await client.post("/api/v1/rule/standard", json=standard_rule_payload)
        rule_id = create_r.json()["data"]["_id"]
        r = await client.delete(f"/api/v1/rule/{rule_id}")
        assert r.status_code == 200
        assert r.json()["message"] == "Rule deleted"

    async def test_delete_rule_not_found(self, client):
        r = await client.delete("/api/v1/rule/000000000000000000000000")
        assert r.status_code == 200
        assert r.json()["success"] is False
