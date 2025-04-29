def build_standard_rule_recommendation(
    description, field, operator, value, risk_point=10
):
    return {
        "rule_type": "standard",
        "description": description,
        "field": field,
        "operator": operator,
        "value": value,
        "risk_point": risk_point,
    }


def build_velocity_rule_recommendation(
    description, field, time_range, aggregation, threshold, risk_point=20
):
    return {
        "rule_type": "velocity",
        "description": description,
        "field": field,
        "time_range": time_range,
        "aggregation_function": aggregation,
        "threshold": threshold,
        "risk_point": risk_point,
    }
