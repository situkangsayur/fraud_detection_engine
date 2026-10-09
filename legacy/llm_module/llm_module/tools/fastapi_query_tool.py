import requests

BASE_API = "http://fraud_engine:8000/api/v1"


def get_rule_stats():
    return requests.get(f"{BASE_API}/stats/rules-performance").json()


def get_policy_stats():
    return requests.get(f"{BASE_API}/stats/policies-performance").json()


def get_transactions():
    return requests.get(f"{BASE_API}/stats/transactions").json()
