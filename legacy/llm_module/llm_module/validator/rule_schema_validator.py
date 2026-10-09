import json


def jalidate_rule_structure(json_text: str) -> (bool, str):
    try:
        rule = json.loads(json_text)

        if "type" not in rule:
            return False, "Missing 'type' field."

        if rule["type"] not in ["StandardRule", "VelocityRule"]:
            return False, f"Invalid 'type': {rule['type']}"

        if "description" not in rule or "field" not in rule:
            return False, "Missing 'description' or 'field'."

        if rule["type"] == "StandardRule":
            if "operator" not in rule or "value" not in rule:
                return False, "StandardRule must have 'operator' and 'value'."

        if rule["type"] == "VelocityRule":
            if "aggregation_function" not in rule or "threshold" not in rule:
                return (
                    False,
                    "VelocityRule must have 'aggregation_function' and 'threshold'.",
                )

        return True, "Valid rule structure."
    except Exception as e:
        return False, f"Parsing error: {e}"
