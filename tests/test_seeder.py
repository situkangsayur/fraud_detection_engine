import pytest
from app.seeder.seeder import seed_data
from app.db.mongo import db


@pytest.mark.asyncio
class TestSeeder:
    async def test_seed_data(self):
        await seed_data()

        users = await db["users"].find().to_list(length=100)
        assert len(users) == 30

        transactions = await db["transactions"].find().to_list(length=1000)
        assert len(transactions) == 450  # 30 users * 15 transactions

        rules = await db["rules"].find().to_list(length=100)
        assert len(rules) == 6

        policies = await db["policies"].find().to_list(length=100)
        assert len(policies) == 4

    async def test_seed_data_idempotent(self):
        """Running seed twice should still result in correct counts (deletes first)."""
        await seed_data()
        await seed_data()

        users = await db["users"].find().to_list(length=100)
        assert len(users) == 30
