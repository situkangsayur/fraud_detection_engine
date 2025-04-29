# api_stats.py
from fastapi import APIRouter
from app.services import service_stats
from app.models.base import ResponseModel

router = APIRouter()


from typing import List, Dict

@router.get("/users", response_model=ResponseModel[List[Dict]])
async def get_user_stats():
    return await service_stats.get_users_statistics_service()


@router.get("/user/{id_user}", response_model=ResponseModel[Dict])
async def get_single_user_stats(id_user: str):
    return await service_stats.get_user_statistics_service(id_user)


@router.get("/transactions", response_model=ResponseModel[Dict])
async def get_transaction_stats():
    return await service_stats.get_transactions_statistics_service()


@router.get("/policies-performance", response_model=ResponseModel[List[Dict]])
async def get_policies_performance():
    return await service_stats.get_policies_performance_service()


@router.get("/rules-performance", response_model=ResponseModel[List[Dict]])
async def get_rules_performance():
    return await service_stats.get_rules_performance_service()
