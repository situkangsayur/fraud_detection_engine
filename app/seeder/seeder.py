import asyncio
import uuid
import random
from app.db.mongo import db
from app.models.base import User, Transaction, Policy, StandardRule, VelocityRule


async def seed_data():
    await db["users"].delete_many({})
    await db["transactions"].delete_many({})
    await db["rules"].delete_many({})
    await db["policies"].delete_many({})

    # Seed Users
    users = []
    for i in range(30):
        users.append(
            User(
                id_user=f"user{i}",
                nama_lengkap=f"User {i}",
                email=f"user{i}@email.com",
                domain_email="email.com",
                address="Jalan Testing",
                address_zip="12345",
                address_city="Jakarta",
                address_province="DKI",
                address_kecamatan="Kec Test",
                phone_number=f"081234567{i}",
            ).dict()
        )
    await db["users"].insert_many(users)

    # Seed Transactions
    trxs = []
    for user in users:
        for j in range(15):
            trxs.append(
                Transaction(
                    id_transaction=str(uuid.uuid4()),
                    id_user=user["id_user"],
                    shipzip="12345",
                    shipping_address="Jalan Shipping",
                    shipping_city="Jakarta",
                    shipping_province="DKI",
                    shipping_kecamatan="Menteng",
                    payment_type="credit_card",
                    number=str(random.randint(1000000000, 9999999999)),
                    bank_name="BCA",
                    amount=random.uniform(10000, 250000),
                    status="success",
                    billing_address="Jalan Billing",
                    billing_city="Jakarta",
                    billing_province="DKI",
                    billing_kecamatan="Menteng",
                    list_of_items=[],
                ).dict()
            )
    await db["transactions"].insert_many(trxs)

    # Seed Rules
    rules = [
        StandardRule(
            description="Amount > 100000",
            risk_point=30,
            field="amount",
            operator=">",
            value=100000,
        ),
        StandardRule(
            description="Zip code mismatch",
            risk_point=20,
            field="shipzip",
            operator="!=",
            value="12345",
        ),
        VelocityRule(
            description="High frequency transactions",
            risk_point=40,
            field="id_user",
            time_range="1h",
            aggregation_function="count",
            threshold=5,
        ),
        VelocityRule(
            description="Daily amount > 1M",
            risk_point=30,
            field="amount",
            time_range="1d",
            aggregation_function="sum",
            threshold=1000000,
        ),
        StandardRule(
            description="Province mismatch",
            risk_point=25,
            field="shipping_province",
            operator="!=",
            value="DKI",
        ),
        VelocityRule(
            description="More than 10 trx/week",
            risk_point=15,
            field="id_user",
            time_range="1w",
            aggregation_function="count",
            threshold=10,
        ),
    ]
    await db["rules"].insert_many([r.dict() for r in rules])

    # Seed Policies
    policies = [
        Policy(
            policy_id="policy1", name="Policy A", description="Fraud A", rules=rules[:2]
        ),
        Policy(
            policy_id="policy2",
            name="Policy B",
            description="Fraud B",
            rules=rules[2:4],
        ),
        Policy(
            policy_id="policy3", name="Policy C", description="Fraud C", rules=rules[4:]
        ),
        Policy(
            policy_id="policy4", name="Policy D", description="Fraud D", rules=rules
        ),
    ]
    await db["policies"].insert_many([p.dict() for p in policies])

    print("✅ Production seeding completed.")


if __name__ == "__main__":
    asyncio.run(seed_data())
