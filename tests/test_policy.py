import pytest
from httpx import AsyncClient
from app.main import app

policy_payload = {
    "policy_id": "policy_test_001",
    "name": "Test Policy",
    "description": "Policy for testing",
    "rules": [],
}


import os

@pytest.mark.asyncio
async def test_policy_crud():
    print(f"USE_MOCK in test: {os.getenv('USE_MOCK')}")
    async with AsyncClient(app=app, base_url="http://test") as ac:
        # Create
        r = await ac.post("/api/v1/policy/", json=policy_payload)
        assert r.status_code == 200
        assert r.json()["success"]

        # Get
        r = await ac.get("/api/v1/policy/policy_test_001")
        assert r.status_code == 200
        assert r.json()["data"]["policy_id"] == "policy_test_001"

        # Update
        updated_payload = policy_payload.copy()
        updated_payload["description"] = "Updated Description"
        r = await ac.put("/api/v1/policy/policy_test_001", json=updated_payload)
        assert r.status_code == 200
        assert r.json()["message"] == "Policy updated"

        # List
        r = await ac.get("/api/v1/policy/")
        assert r.status_code == 200
        assert isinstance(r.json()["data"], list)

        # Delete
        r = await ac.delete("/api/v1/policy/policy_test_001")
        assert r.status_code == 200
        assert r.json()["message"] == "Policy deleted"
