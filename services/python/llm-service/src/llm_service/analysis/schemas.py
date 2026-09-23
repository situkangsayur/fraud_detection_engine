"""JSON schemas for structured LLM outputs (passed to Ollama ``format`` and re-validated with jsonschema).

The rule envelope schema is intentionally shallow for ``definition`` (recursive condition trees are hard for
grammar-constrained decoding); the rule-service validator is the authority and drives the repair loop.
"""

from __future__ import annotations

from typing import Any

TYPOLOGIES = [
    "carding",
    "account_takeover",
    "bank_account_takeover",
    "system_breach",
    "promo_abuse",
    "refund_abuse",
    "money_mule",
    "other",
]
RULE_KINDS = ["simple", "velocity", "composite", "reference", "graph"]

RULE_ENVELOPE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "code": {"type": "string", "pattern": "^[A-Z0-9-]{3,40}$"},
        "name": {"type": "string"},
        "description": {"type": "string"},
        "kind": {"type": "string", "enum": RULE_KINDS},
        "typologies": {"type": "array", "items": {"type": "string", "enum": TYPOLOGIES}},
        "event_types": {"type": "array", "items": {"type": "string"}},
        "risk_score": {"type": "number", "minimum": 0, "maximum": 100},
        "trapped_score": {"type": "number", "minimum": 0, "maximum": 100},
        "action": {"type": "string", "enum": ["score", "force_review", "force_decline", "force_approve"]},
        "on_trapped": {"type": "string", "enum": ["ignore", "score", "review"]},
        "missing_as_no_match": {"type": "boolean"},
        "definition": {
            "type": "object",
            "properties": {"kind": {"type": "string", "enum": RULE_KINDS}},
            "required": ["kind"],
        },
    },
    "required": [
        "code",
        "name",
        "description",
        "kind",
        "typologies",
        "event_types",
        "risk_score",
        "action",
        "definition",
    ],
}

CITATION_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {"code": {"type": "string"}, "section": {"type": "string"}, "excerpt": {"type": "string"}},
    "required": ["code", "section"],
}

RECOMMENDATION_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "proposal_type": {"type": "string", "enum": ["new_rule", "modify_rule", "retire_rule"]},
        "target_rule_code": {"type": ["string", "null"]},
        "rationale": {"type": "string"},
        "evidence": {"type": "object"},
        "citations": {"type": "array", "items": CITATION_SCHEMA},
        "rule": {"anyOf": [RULE_ENVELOPE_SCHEMA, {"type": "null"}]},
    },
    "required": ["proposal_type", "rationale", "rule"],
}

RECOMMEND_OUTPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "summary_md": {"type": "string"},
        "recommendations": {"type": "array", "items": RECOMMENDATION_SCHEMA},
    },
    "required": ["summary_md", "recommendations"],
}

RELEVANCE_OUTPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "summary_md": {"type": "string"},
        "verdicts": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "rule_code": {"type": "string"},
                    "verdict": {"type": "string", "enum": ["keep", "tune", "retire"]},
                    "rationale": {"type": "string"},
                    "suggested_change": {"type": ["string", "null"]},
                    "citations": {"type": "array", "items": CITATION_SCHEMA},
                },
                "required": ["rule_code", "verdict", "rationale"],
            },
        },
        "key_risks": {"type": "array", "items": {"type": "string"}},
    },
    "required": ["summary_md", "verdicts"],
}

SITUATION_OUTPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "summary_md": {"type": "string"},
        "key_risks": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "severity": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                    "evidence": {"type": "string"},
                    "recommended_action": {"type": "string"},
                },
                "required": ["title", "severity", "evidence"],
            },
        },
        "typology_trends": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "typology": {"type": "string"},
                    "trend": {"type": "string", "enum": ["rising", "stable", "falling", "unknown"]},
                    "note": {"type": "string"},
                },
                "required": ["typology", "trend"],
            },
        },
    },
    "required": ["summary_md", "key_risks"],
}

IMPACT_OUTPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "summary_md": {"type": "string"},
        "obligations": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "article": {"type": "string"},
                    "obligation": {"type": "string"},
                    "coverage": {"type": "string", "enum": ["covered", "partially_covered", "gap", "conflict"]},
                    "rule_codes": {"type": "array", "items": {"type": "string"}},
                },
                "required": ["article", "obligation", "coverage"],
            },
        },
        "recommendations": {"type": "array", "items": RECOMMENDATION_SCHEMA},
    },
    "required": ["summary_md", "obligations", "recommendations"],
}

MAPPING_OUTPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "suggestions": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "source_path": {"type": "string"},
                    "target": {"type": ["string", "null"]},
                    "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                    "reason": {"type": "string"},
                },
                "required": ["source_path", "target", "confidence"],
            },
        },
    },
    "required": ["suggestions"],
}
