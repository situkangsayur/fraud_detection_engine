"""Prometheus metrics."""

from prometheus_client import Counter, Histogram

HTTP_LATENCY = Histogram(
    "ingest_http_request_duration_seconds", "HTTP latency", ["method", "route", "status"]
)
INSPECTIONS = Counter("ingest_inspections_total", "Schema inspections", ["kind"])
JOBS = Counter("ingest_jobs_total", "Finished ingest jobs", ["status"])
ROWS = Counter("ingest_rows_total", "Rows sent to core-api", ["result"])
POLLS = Counter("ingest_connector_polls_total", "Pull connector polls", ["result"])
