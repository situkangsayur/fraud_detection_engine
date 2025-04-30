from llm_module.tools.fastapi_query_tool import get_rule_stats, get_policy_stats


def analyze_rule_effectiveness():
    rules = get_rule_stats().get("data", [])
    ineffective_rules = [r for r in rules if r.get("hit_count", 0) < 3]
    return ineffective_rules


def suggest_rule_cleanup():
    rules = analyze_rule_effectiveness()
    return f"Rules to consider deactivating: {[r['name'] for r in rules]}"
