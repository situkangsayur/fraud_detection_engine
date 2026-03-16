import pytest


@pytest.mark.asyncio
class TestHealthAPI:
    async def test_healthcheck(self, client):
        r = await client.get("/api/v1/health")
        assert r.status_code == 200
        body = r.json()
        assert body["status"] == "ok"
        assert "db" in body
