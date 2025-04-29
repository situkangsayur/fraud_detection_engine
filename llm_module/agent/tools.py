from langchain.tools import tool
from llm_module.tools.fastapi_query_tool import get_rule_stats, get_policy_stats
from llm_module.analysis.recommendation_builder import (
    build_standard_rule_recommendation,
)


@tool
def fetch_rule_statistics() -> str:
    """Ambil statistik performa semua rules dari sistem utama."""
    data = get_rule_stats()
    return str(data.get("data", []))


@tool
def fetch_policy_statistics() -> str:
    """Ambil statistik performa semua policy dari sistem utama."""
    data = get_policy_stats()
    return str(data.get("data", []))


@tool
def create_sample_standard_rule(
    description: str, field: str, operator: str, value: str, risk_point: int = 10
) -> dict:
    """Buat contoh struktur Standard Rule"""
    return build_standard_rule_recommendation(
        description, field, operator, value, risk_point
    )


import requests

BASE_API = "http://fraud_engine:8000/api/v1"


@tool
def post_new_rule(rule_json: dict) -> str:
    """Post a new Standard or Velocity Rule into Fraud Engine."""
    try:
        response = requests.post(f"{BASE_API}/rule/standard", json=rule_json)
        return f"✅ Rule created: {response.json()}"
    except Exception as e:
        return f"❌ Failed to create rule: {str(e)}"
