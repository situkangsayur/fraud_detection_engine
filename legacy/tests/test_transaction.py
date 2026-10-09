import pytest


@pytest.mark.asyncio
class TestTransactionAPI:
    async def test_create_transaction(self, client, transaction_payload):
        r = await client.post("/api/v1/transaction/", json=transaction_payload)
        assert r.status_code == 200
        assert r.json()["success"] is True
        assert r.json()["data"]["id_transaction"] == "trx_test_001"

    async def test_get_transaction(self, client, transaction_payload):
        await client.post("/api/v1/transaction/", json=transaction_payload)
        r = await client.get("/api/v1/transaction/trx_test_001")
        assert r.status_code == 200
        assert r.json()["data"]["amount"] == 200000.0

    async def test_get_transaction_not_found(self, client):
        r = await client.get("/api/v1/transaction/nonexistent")
        assert r.status_code == 200
        assert r.json()["success"] is False
        assert r.json()["message"] == "Transaction not found"

    async def test_list_transactions(self, client, transaction_payload):
        await client.post("/api/v1/transaction/", json=transaction_payload)
        r = await client.get("/api/v1/transaction/")
        assert r.status_code == 200
        assert len(r.json()["data"]) >= 1

    async def test_list_transactions_empty(self, client):
        r = await client.get("/api/v1/transaction/")
        assert r.status_code == 200
        assert r.json()["data"] == []

    async def test_update_transaction(self, client, transaction_payload):
        await client.post("/api/v1/transaction/", json=transaction_payload)
        transaction_payload["status"] = "pending"
        r = await client.put("/api/v1/transaction/trx_test_001", json=transaction_payload)
        assert r.status_code == 200
        assert r.json()["message"] == "Transaction updated"

    async def test_update_transaction_not_found(self, client, transaction_payload):
        r = await client.put("/api/v1/transaction/nonexistent", json=transaction_payload)
        assert r.status_code == 200
        assert r.json()["success"] is False

    async def test_delete_transaction(self, client, transaction_payload):
        await client.post("/api/v1/transaction/", json=transaction_payload)
        r = await client.delete("/api/v1/transaction/trx_test_001")
        assert r.status_code == 200
        assert r.json()["message"] == "Transaction deleted"

    async def test_delete_transaction_not_found(self, client):
        r = await client.delete("/api/v1/transaction/nonexistent")
        assert r.status_code == 200
        assert r.json()["success"] is False
