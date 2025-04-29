# service_user.py
from app.db.mongo import db
from app.models.base import User, ResponseModel


async def create_user_service(user: User):
    await db["users"].insert_one(user.dict())
    return ResponseModel(success=True, message="User created", data=user.dict())


async def get_user_service(id_user: str):
    user = await db["users"].find_one({"id_user": id_user})
    if user:
        return ResponseModel(success=True, message="User fetched", data=user)
    return ResponseModel(success=False, message="User not found", data=None)


async def list_users_service():
    users = await db["users"].find().to_list(length=100)
    return ResponseModel(success=True, message="All users", data=users)


async def update_user_service(id_user: str, user: User):
    result = await db["users"].update_one({"id_user": id_user}, {"$set": user.dict()})
    if result.modified_count:
        return ResponseModel(success=True, message="User updated", data=user.dict())
    return ResponseModel(success=False, message="User not found or unchanged")


async def delete_user_service(id_user: str):
    result = await db["users"].delete_one({"id_user": id_user})
    if result.deleted_count:
        return ResponseModel(success=True, message="User deleted")
    return ResponseModel(success=False, message="User not found")
