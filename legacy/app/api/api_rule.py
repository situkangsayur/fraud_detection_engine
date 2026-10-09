# api_rule.py
from fastapi import APIRouter
from app.models.base import StandardRule, VelocityRule, ResponseModel
from app.services import service_rule

router = APIRouter()


@router.post("/standard", response_model=ResponseModel)
async def create_standard_rule(rule: StandardRule):
    return await service_rule.create_standard_rule_service(rule)


@router.post("/velocity", response_model=ResponseModel)
async def create_velocity_rule(rule: VelocityRule):
    return await service_rule.create_velocity_rule_service(rule)


@router.get("/", response_model=ResponseModel)
async def list_rules():
    return await service_rule.list_rules_service()


@router.get("/{rule_id}", response_model=ResponseModel)
async def get_rule(rule_id: str):
    return await service_rule.get_rule_service(rule_id)


@router.delete("/{rule_id}", response_model=ResponseModel)
async def delete_rule(rule_id: str):
    return await service_rule.delete_rule_service(rule_id)
