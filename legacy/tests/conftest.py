import os
import pytest
from httpx import AsyncClient, ASGITransport

os.environ["USE_MOCK"] = "true"

from app.main import app
from app.db.mongo import db


@pytest.fixture
async def client():
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        yield ac


@pytest.fixture(autouse=True)
async def clean_db():
    """Clean all collections before each test."""
    for col_name in ["users", "transactions", "policies", "rules"]:
        await db[col_name].delete_many({})
    yield
    for col_name in ["users", "transactions", "policies", "rules"]:
        await db[col_name].delete_many({})


# === Shared test data fixtures ===

@pytest.fixture
def user_payload():
    return {
        "id_user": "test_user_001",
        "nama_lengkap": "Test User",
        "email": "test@example.com",
        "domain_email": "example.com",
        "address": "Jl. Test 123",
        "address_zip": "12345",
        "address_city": "Jakarta",
        "address_province": "DKI",
        "address_kecamatan": "Menteng",
        "phone_number": "081234567890",
    }


@pytest.fixture
def transaction_payload():
    return {
        "id_transaction": "trx_test_001",
        "id_user": "test_user_001",
        "shipzip": "40000",
        "shipping_address": "Jl. Shipping",
        "shipping_city": "Jakarta",
        "shipping_province": "DKI Jakarta",
        "shipping_kecamatan": "Menteng",
        "payment_type": "credit_card",
        "number": "1234567890",
        "bank_name": "BCA",
        "amount": 200000.0,
        "status": "success",
        "billing_address": "Jl. Billing",
        "billing_city": "Jakarta",
        "billing_province": "DKI Jakarta",
        "billing_kecamatan": "Menteng",
        "list_of_items": [{"item_id": "item1", "qty": 2, "price": 100000}],
    }


@pytest.fixture
def standard_rule_payload():
    return {
        "description": "Amount > 100000",
        "risk_point": 50,
        "rule_type": "standard",
        "field": "amount",
        "operator": ">",
        "value": 100000,
    }


@pytest.fixture
def velocity_rule_payload():
    return {
        "description": "High frequency transactions",
        "risk_point": 40,
        "rule_type": "velocity",
        "field": "id_user",
        "time_range": "1h",
        "aggregation_function": "count",
        "threshold": 5,
    }


@pytest.fixture
def policy_payload(standard_rule_payload):
    return {
        "policy_id": "policy_test_001",
        "name": "Test Policy",
        "description": "Policy for testing",
        "rules": [standard_rule_payload],
    }
