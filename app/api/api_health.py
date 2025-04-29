from fastapi import APIRouter
from app.db.mongo import db

router = APIRouter()

@router.get("/health", tags=["Health"])
async def healthcheck():
    try:
        await db.command("ping")  # test MongoDB connection
        return {"status": "ok", "db": "connected"}
    except Exception:
        return {"status": "ok", "db": "disconnected"}
