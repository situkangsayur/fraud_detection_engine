from typing import List
from fastapi import APIRouter
from app.models.base import Transaction, ResponseModel
from app.services import service_transaction

router = APIRouter()


@router.post("/", response_model=ResponseModel[Transaction])
async def create_transaction(transaction: Transaction):
    return await service_transaction.create_transaction_service(transaction)


@router.get("/", response_model=ResponseModel[List[Transaction]])
async def list_transactions():
    return await service_transaction.list_transactions_service()


@router.get("/{id_transaction}", response_model=ResponseModel[Transaction])
async def get_transaction(id_transaction: str):
    return await service_transaction.get_transaction_service(id_transaction)


@router.put("/{id_transaction}", response_model=ResponseModel[Transaction])
async def update_transaction(id_transaction: str, transaction: Transaction):
    return await service_transaction.update_transaction_service(
        id_transaction, transaction
    )


@router.delete("/{id_transaction}", response_model=ResponseModel[dict]) # Assuming delete returns a simple success message
async def delete_transaction(id_transaction: str):
    return await service_transaction.delete_transaction_service(id_transaction)
