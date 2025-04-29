import pytest
from httpx import AsyncClient
from app.main import app

@pytest.mark.asyncio
async def test_rule_crud():
    async with AsyncClient(app=app, base_url="http://test") as ac:
        # Create standard rule
        standard_payload = {
            "description": "Amount > 10000",
            "risk_point": 10,
            "rule_type": "standard",
            "field": "amount",
            "operator": ">",
            "value": 10000
        }
        r = await ac.post("/api/v1/rule/standard", json=standard_payload)
        assert r.status_code == 200
        rule_id = r.json()["data"]["_id"]

        # Get rule
        r = await ac.get(f"/api/v1/rule/{rule_id}")
        assert r.status_code == 200
        assert r.json()["data"]["description"] == "Amount > 10000"

        # List rules
        r = await ac.get("/api/v1/rule/")
        assert r.status_code == 200
        assert isinstance(r.json()["data"], list)

        # Delete rule
        r = await ac.delete(f"/api/v1/rule/{rule_id}")
        assert r.status_code == 200
        assert r.json()["message"] == "Rule deleted"
