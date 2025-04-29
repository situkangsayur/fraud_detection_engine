import pytest
from httpx import AsyncClient
from app.main import app

@pytest.mark.asyncio
async def test_user_crud():
    async with AsyncClient(app=app, base_url="http://test") as ac:
        payload = {
            "id_user": "unit_test_user",
            "nama_lengkap": "Unit Test",
            "email": "unit@example.com",
            "domain_email": "example.com",
            "address": "Jl. Unit",
            "address_zip": "12345",
            "address_city": "Jakarta",
            "address_province": "DKI",
            "address_kecamatan": "Unit",
            "phone_number": "081234567890"
        }

        # Create
        r = await ac.post("/api/v1/user/", json=payload)
        assert r.status_code == 200
        assert r.json()["success"]

        # List
        r = await ac.get("/api/v1/user/")
        assert r.status_code == 200
        assert any(u["id_user"] == "unit_test_user" for u in r.json()["data"])

        # Update
        payload["nama_lengkap"] = "Updated Name"
        r = await ac.put("/api/v1/user/unit_test_user", json=payload)
        assert r.status_code == 200
        assert r.json()["message"] == "User updated"

        # Get
        r = await ac.get("/api/v1/user/unit_test_user")
        assert r.status_code == 200
        assert r.json()["data"]["nama_lengkap"] == "Updated Name"

        # Delete
        r = await ac.delete("/api/v1/user/unit_test_user")
        assert r.status_code == 200
        assert r.json()["message"] == "User deleted"
