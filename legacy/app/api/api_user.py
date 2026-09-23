# api_user.py
from typing import List
from fastapi import APIRouter
from app.services import service_user
from app.models.base import User, ResponseModel

router = APIRouter()


@router.post("/", response_model=ResponseModel[User])
async def create_user(user: User):
    return await service_user.create_user_service(user)


@router.get("/{id_user}", response_model=ResponseModel[User])
async def get_user(id_user: str):
    return await service_user.get_user_service(id_user)


@router.get("/", response_model=ResponseModel[List[User]])
async def list_users():
    return await service_user.list_users_service()


@router.put("/{id_user}", response_model=ResponseModel[User])
async def update_user(id_user: str, user: User):
    return await service_user.update_user_service(id_user, user)


@router.delete("/{id_user}", response_model=ResponseModel[dict]) # Assuming delete returns a simple success message
async def delete_user(id_user: str):
    return await service_user.delete_user_service(id_user)
