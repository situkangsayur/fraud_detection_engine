# service_transaction.py
from app.db.mongo import db
from app.models.base import Transaction, ResponseModel

collection = db["transactions"]

async def create_transaction_service(transaction: Transaction):
    await collection.insert_one(transaction.dict())
    return ResponseModel(success=True, message="Transaction created", data=transaction.dict())

async def get_transaction_service(id_transaction: str):
    trx = await collection.find_one({"id_transaction": id_transaction})
    if trx:
        return ResponseModel(success=True, message="Transaction fetched", data=trx)
    return ResponseModel(success=False, message="Transaction not found")

async def list_transactions_service():
    trxs = await collection.find().to_list(length=100)
    return ResponseModel(success=True, message="Transaction list", data=trxs)

async def update_transaction_service(id_transaction: str, transaction: Transaction):
    result = await collection.update_one({"id_transaction": id_transaction}, {"$set": transaction.dict()})
    if result.modified_count:
        return ResponseModel(success=True, message="Transaction updated", data=transaction.dict())
    return ResponseModel(success=False, message="Transaction not found or unchanged")

async def delete_transaction_service(id_transaction: str):
    result = await collection.delete_one({"id_transaction": id_transaction})
    if result.deleted_count:
        return ResponseModel(success=True, message="Transaction deleted")
    return ResponseModel(success=False, message="Transaction not found")
