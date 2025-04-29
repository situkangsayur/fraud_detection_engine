import pytest
from httpx import AsyncClient
from app.main import app

transaction_payload = {
    "id_transaction": "trx_test_001",
    "id_user": "test_user",
    "shipzip": "40000",
    "shipping_address": "Test Address",
    "shipping_city": "Jakarta",
    "shipping_province": "DKI Jakarta",
    "shipping_kecamatan": "Menteng",
    "payment_type": "credit_card",
    "number": "1234567890",
    "bank_name": "BCA",
    "amount": 200000.0,
    "status": "success",
    "billing_address": "Billing Address",
    "billing_city": "Jakarta",
    "billing_province": "DKI Jakarta",
    "billing_kecamatan": "Menteng",
    "list_of_items": [{"item_id": "1", "qty": 1, "price": 200000}]
}

@pytest.mark.asyncio
async def test_transaction_crud():
    async with AsyncClient(app=app, base_url="http://test") as ac:
        # Create
        r = await ac.post("/api/v1/transaction/", json=transaction_payload)
        assert r.status_code == 200
        assert r.json()["success"]

        # Get
        r = await ac.get("/api/v1/transaction/trx_test_001")
        assert r.status_code == 200
        assert r.json()["data"]["id_transaction"] == "trx_test_001"

        # Update
        updated_payload = transaction_payload.copy()
        updated_payload["status"] = "pending"
        r = await ac.put("/api/v1/transaction/trx_test_001", json=updated_payload)
        assert r.status_code == 200
        assert r.json()["message"] == "Transaction updated"

        # List
        r = await ac.get("/api/v1/transaction/")
        assert r.status_code == 200
        assert isinstance(r.json()["data"], list)

        # Delete
        r = await ac.delete("/api/v1/transaction/trx_test_001")
        assert r.status_code == 200
        assert r.json()["message"] == "Transaction deleted"
