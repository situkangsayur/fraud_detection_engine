"""Prometheus metrics."""

from __future__ import annotations

from prometheus_client import Counter, Histogram

HTTP_REQUESTS = Counter("llm_http_requests_total", "HTTP requests", ["method", "route", "status"])
HTTP_LATENCY = Histogram("llm_http_request_seconds", "HTTP latency", ["method", "route"])
LLM_CALLS = Counter("llm_ollama_calls_total", "Ollama calls", ["kind", "outcome"])
TOOL_CALLS = Counter("llm_tool_calls_total", "Agent tool calls", ["tool", "outcome"])
REPORTS = Counter("llm_reports_total", "Analysis reports", ["report_type", "outcome"])
PROPOSALS = Counter("llm_proposals_total", "Rule proposals created", ["outcome"])
REGULATIONS_INDEXED = Counter("llm_regulations_indexed_total", "Regulation documents indexed")
