import pytest


@pytest.mark.asyncio
class TestPolicyAPI:
    async def test_create_policy(self, client, policy_payload):
        r = await client.post("/api/v1/policy/", json=policy_payload)
        assert r.status_code == 200
        assert r.json()["success"] is True
        assert r.json()["data"]["policy_id"] == "policy_test_001"

    async def test_create_policy_empty_rules(self, client):
        payload = {
            "policy_id": "policy_empty",
            "name": "Empty Policy",
            "description": "No rules",
            "rules": [],
        }
        r = await client.post("/api/v1/policy/", json=payload)
        assert r.status_code == 200
        assert r.json()["success"] is True

    async def test_get_policy(self, client, policy_payload):
        await client.post("/api/v1/policy/", json=policy_payload)
        r = await client.get("/api/v1/policy/policy_test_001")
        assert r.status_code == 200
        assert r.json()["data"]["name"] == "Test Policy"

    async def test_get_policy_not_found(self, client):
        r = await client.get("/api/v1/policy/nonexistent")
        assert r.status_code == 200
        assert r.json()["success"] is False
        assert r.json()["message"] == "Policy not found"

    async def test_list_policies(self, client, policy_payload):
        await client.post("/api/v1/policy/", json=policy_payload)
        r = await client.get("/api/v1/policy/")
        assert r.status_code == 200
        assert len(r.json()["data"]) >= 1

    async def test_update_policy(self, client, policy_payload):
        await client.post("/api/v1/policy/", json=policy_payload)
        policy_payload["description"] = "Updated Description"
        r = await client.put("/api/v1/policy/policy_test_001", json=policy_payload)
        assert r.status_code == 200
        assert r.json()["message"] == "Policy updated"

    async def test_update_policy_not_found(self, client, policy_payload):
        r = await client.put("/api/v1/policy/nonexistent", json=policy_payload)
        assert r.status_code == 200
        assert r.json()["success"] is False

    async def test_delete_policy(self, client, policy_payload):
        await client.post("/api/v1/policy/", json=policy_payload)
        r = await client.delete("/api/v1/policy/policy_test_001")
        assert r.status_code == 200
        assert r.json()["message"] == "Policy deleted"

    async def test_delete_policy_not_found(self, client):
        r = await client.delete("/api/v1/policy/nonexistent")
        assert r.status_code == 200
        assert r.json()["success"] is False
