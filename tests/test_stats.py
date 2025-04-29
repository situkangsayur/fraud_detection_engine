
import pytest
from httpx import AsyncClient
from app.main import app

@pytest.mark.asyncio
async def test_stats_endpoints():
    async with AsyncClient(app=app, base_url="http://test") as ac:
        # Call all available stats
        endpoints = [
            "/api/v1/stats/users",
            "/api/v1/stats/transactions",
            "/api/v1/stats/policies-performance",
            "/api/v1/stats/rules-performance"
        ]

        for ep in endpoints:
            r = await ac.get(ep)
            assert r.status_code == 200
            assert r.json()["success"]

        # Test user stats by ID
        r = await ac.get("/api/v1/stats/user/proc_test_user")
        assert r.status_code == 200
        assert r.json()["data"]["id_user"] == "proc_test_user"
