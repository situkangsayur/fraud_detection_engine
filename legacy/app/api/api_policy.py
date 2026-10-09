# api_policy.py
from typing import List
from fastapi import APIRouter
from app.models.base import Policy, ResponseModel
from app.services import service_policy

router = APIRouter()


@router.post("/", response_model=ResponseModel[Policy])
async def create_policy(policy: Policy):
    return await service_policy.create_policy_service(policy)


@router.get("/", response_model=ResponseModel[List[Policy]])
async def list_policies():
    return await service_policy.list_policies_service()


@router.get("/{policy_id}", response_model=ResponseModel[Policy])
async def get_policy(policy_id: str):
    return await service_policy.get_policy_service(policy_id)


@router.put("/{policy_id}", response_model=ResponseModel[Policy])
async def update_policy(policy_id: str, policy: Policy):
    return await service_policy.update_policy_service(policy_id, policy)


@router.delete("/{policy_id}", response_model=ResponseModel[dict]) # Assuming delete returns a simple success message
async def delete_policy(policy_id: str):
    return await service_policy.delete_policy_service(policy_id)
