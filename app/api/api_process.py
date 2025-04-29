# api_process.py
from typing import List, Dict, TypedDict, Optional
from fastapi import APIRouter
from app.services import service_process
from app.models.base import ResponseModel, ProcessedTransactionData

router = APIRouter()

@router.post("/transaction", response_model=ResponseModel[ProcessedTransactionData])
async def process_transaction(payload: dict):
    id_transaction = payload.get("id_transaction")
    result = await service_process.process_transaction_service(id_transaction)
    return result
