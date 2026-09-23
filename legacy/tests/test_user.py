import pytest


@pytest.mark.asyncio
class TestUserAPI:
    async def test_create_user(self, client, user_payload):
        r = await client.post("/api/v1/user/", json=user_payload)
        assert r.status_code == 200
        body = r.json()
        assert body["success"] is True
        assert body["message"] == "User created"
        assert body["data"]["id_user"] == "test_user_001"

    async def test_get_user(self, client, user_payload):
        await client.post("/api/v1/user/", json=user_payload)
        r = await client.get("/api/v1/user/test_user_001")
        assert r.status_code == 200
        assert r.json()["data"]["nama_lengkap"] == "Test User"

    async def test_get_user_not_found(self, client):
        r = await client.get("/api/v1/user/nonexistent")
        assert r.status_code == 200
        assert r.json()["success"] is False
        assert r.json()["message"] == "User not found"

    async def test_list_users(self, client, user_payload):
        await client.post("/api/v1/user/", json=user_payload)
        r = await client.get("/api/v1/user/")
        assert r.status_code == 200
        assert len(r.json()["data"]) >= 1

    async def test_list_users_empty(self, client):
        r = await client.get("/api/v1/user/")
        assert r.status_code == 200
        assert r.json()["data"] == []

    async def test_update_user(self, client, user_payload):
        await client.post("/api/v1/user/", json=user_payload)
        user_payload["nama_lengkap"] = "Updated Name"
        r = await client.put("/api/v1/user/test_user_001", json=user_payload)
        assert r.status_code == 200
        assert r.json()["message"] == "User updated"

    async def test_update_user_not_found(self, client, user_payload):
        r = await client.put("/api/v1/user/nonexistent", json=user_payload)
        assert r.status_code == 200
        assert r.json()["success"] is False

    async def test_delete_user(self, client, user_payload):
        await client.post("/api/v1/user/", json=user_payload)
        r = await client.delete("/api/v1/user/test_user_001")
        assert r.status_code == 200
        assert r.json()["message"] == "User deleted"

    async def test_delete_user_not_found(self, client):
        r = await client.delete("/api/v1/user/nonexistent")
        assert r.status_code == 200
        assert r.json()["success"] is False
        assert r.json()["message"] == "User not found"
