"""HTTP client for core-api internal batch ingest (retries with exponential backoff on 5xx/network)."""

from __future__ import annotations

import time
import uuid
from dataclasses import dataclass, field
from typing import Any

import httpx

from app.config import Settings
from app.logging import log


class CoreApiError(Exception):
    def __init__(self, status: int | None, message: str) -> None:
        super().__init__(message)
        self.status = status


@dataclass
class BatchResult:
    accepted: int
    rejected: int
    errors: list[dict[str, Any]] = field(default_factory=list)


class CoreClient:
    def __init__(self, settings: Settings, client: httpx.Client | None = None) -> None:
        self.s = settings
        self.http = client or httpx.Client(base_url=settings.core_api_url, timeout=settings.core_timeout_s)

    def _headers(self, tenant_id: uuid.UUID, project_id: uuid.UUID, actor: str | None) -> dict[str, str]:
        h = {
            "Authorization": f"Bearer {self.s.internal_api_token}",
            "X-Tenant-Id": str(tenant_id),
            "X-Project-Id": str(project_id),
            "X-Request-Id": str(uuid.uuid4()),
        }
        if actor:
            h["X-Actor"] = actor
        return h

    def send_batch(
        self,
        *,
        tenant_id: uuid.UUID,
        project_id: uuid.UUID,
        source_id: uuid.UUID,
        records: list[dict[str, Any]],
        mode: str,
        job_id: uuid.UUID | None,
        actor: str | None = None,
    ) -> BatchResult:
        path = f"/v1/internal/projects/{project_id}/sources/{source_id}/batch"
        body: dict[str, Any] = {"records": records, "mode": mode}
        if job_id:
            body["job_id"] = str(job_id)
        delay = 1.0
        last: Exception | None = None
        for attempt in range(1, self.s.core_max_retries + 1):
            try:
                resp = self.http.post(path, json=body, headers=self._headers(tenant_id, project_id, actor))
            except httpx.TransportError as e:
                last = e
                log.warning("core_batch_transport_error", attempt=attempt, error=str(e))
            else:
                if resp.status_code < 300:
                    data = resp.json()
                    return BatchResult(
                        int(data.get("accepted", 0)),
                        int(data.get("rejected", 0)),
                        list(data.get("errors", [])),
                    )
                if resp.status_code == 422:
                    # whole batch rejected by validation → count as rejected, keep going
                    detail = resp.json().get("detail", resp.text) if resp.content else ""
                    return BatchResult(0, len(records), [{"index": None, "reason": str(detail)}])
                if resp.status_code < 500 and resp.status_code != 429:
                    raise CoreApiError(
                        resp.status_code, f"core-api rejected batch: {resp.status_code} {resp.text[:300]}"
                    )
                last = CoreApiError(resp.status_code, f"core-api {resp.status_code}")
                log.warning("core_batch_retryable", attempt=attempt, status=resp.status_code)
            if attempt < self.s.core_max_retries:
                time.sleep(delay)
                delay = min(delay * 2, 30.0)
        raise CoreApiError(getattr(last, "status", None), f"core-api unavailable after retries: {last}")

    def close(self) -> None:
        self.http.close()
