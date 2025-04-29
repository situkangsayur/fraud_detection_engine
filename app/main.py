# main.py
from fastapi import FastAPI
from app.api import (
    api_user,
    api_transaction,
    api_policy,
    api_rule,
    api_process,
    api_stats,
    api_health,
)

app = FastAPI(title="Fraud Policy Engine", version="1.0")

app.include_router(api_health.router, prefix="/api/v1", tags=["Health"])
app.include_router(api_user.router, prefix="/api/v1/user", tags=["User"])
app.include_router(
    api_transaction.router, prefix="/api/v1/transaction", tags=["Transaction"]
)
app.include_router(api_policy.router, prefix="/api/v1/policy", tags=["Policy"])
app.include_router(api_rule.router, prefix="/api/v1/rule", tags=["Rule"])
app.include_router(api_process.router, prefix="/api/v1/process", tags=["Process"])
app.include_router(api_stats.router, prefix="/api/v1/stats", tags=["Statistics"])
