from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any

import httpx
import pytest
import respx

from app.config import Settings
from app.core_client import CoreClient
from app.jobs.runner import JobRunner, sort_records
from tests.conftest import PROJECT, SOURCE, TENANT


def _settings(**kw: Any) -> Settings:
    return Settings(
        core_api_url="http://core.test",
        internal_api_token="test-internal-token",
        core_max_retries=3,
        batch_size=500,
        **kw,
    )


def _job(state: dict[str, Any], mode: str = "load_only") -> dict[str, Any]:
    jid = uuid.uuid4()
    job = {
        "id": jid,
        "tenant_id": TENANT,
        "project_id": PROJECT,
        "mode": mode,
        "status": "queued",
        "processed_rows": 0,
        "accepted_rows": 0,
        "rejected_rows": 0,
    }
    state["jobs"][jid] = job
    return job


def _csv(tmp_path: Path, n: int, reverse: bool = True) -> dict[str, Any]:
    p = tmp_path / "t.csv"
    rows = [f"T{i},2026-09-{1 + (i % 28):02d}T{i % 24:02d}:00:00Z,{i}" for i in range(n)]
    if reverse:
        rows.reverse()
    p.write_text("id,ts,amount\n" + "\n".join(rows) + "\n")
    return {"file_path": str(p), "file_format": "csv", "row_estimate": n}


URL = f"http://core.test/v1/internal/projects/{PROJECT}/sources/{SOURCE}/batch"


@respx.mock
def test_batches_sorted_and_counted(
    fake_db: dict[str, Any], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr("app.core_client.time.sleep", lambda s: None)
    seen: list[dict[str, Any]] = []

    def handler(request: httpx.Request) -> httpx.Response:
        body = json.loads(request.content)
        assert request.headers["authorization"] == "Bearer test-internal-token"
        assert request.headers["x-tenant-id"] == str(TENANT)
        assert request.headers["x-project-id"] == str(PROJECT)
        seen.append(body)
        n = len(body["records"])
        return httpx.Response(
            200,
            json={
                "accepted": n - 1,
                "rejected": 1,
                "errors": [{"index": 0, "reason": "bad"}],
                "decisions": [],
            },
        )

    respx.post(URL).mock(side_effect=handler)
    s = _settings()
    runner = JobRunner(s, client=CoreClient(s))
    job = _job(fake_db)
    status = runner.run(job, fake_db["source"], fake_db["mapping"]["mapping"], _csv(tmp_path, 1203), "u1")
    assert status == "done"
    assert [len(b["records"]) for b in seen] == [500, 500, 203]
    assert all(b["mode"] == "load_only" and b["job_id"] == str(job["id"]) for b in seen)
    all_ts = [r["ts"] for b in seen for r in b["records"]]
    assert all_ts == sorted(all_ts)  # chronological order
    j = fake_db["jobs"][job["id"]]
    assert (j["processed_rows"], j["accepted_rows"], j["rejected_rows"]) == (1203, 1200, 3)
    assert j["status"] == "done"


@respx.mock
def test_retries_5xx_then_succeeds(
    fake_db: dict[str, Any], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr("app.core_client.time.sleep", lambda s: None)
    route = respx.post(URL).mock(
        side_effect=[
            httpx.Response(503),
            httpx.ConnectError("boom"),
            httpx.Response(200, json={"accepted": 10, "rejected": 0}),
        ]
    )
    s = _settings()
    status = JobRunner(s, client=CoreClient(s)).run(
        _job(fake_db), fake_db["source"], None, _csv(tmp_path, 10), None
    )
    assert status == "done" and route.call_count == 3


@respx.mock
def test_4xx_fails_job(fake_db: dict[str, Any], tmp_path: Path) -> None:
    respx.post(URL).mock(return_value=httpx.Response(409, json={"title": "no active mapping"}))
    s = _settings()
    job = _job(fake_db)
    assert (
        JobRunner(s, client=CoreClient(s)).run(job, fake_db["source"], None, _csv(tmp_path, 10), None)
        == "failed"
    )
    assert "409" in fake_db["jobs"][job["id"]]["error"]


@respx.mock
def test_422_counts_batch_rejected(fake_db: dict[str, Any], tmp_path: Path) -> None:
    respx.post(URL).mock(return_value=httpx.Response(422, json={"detail": "mapping failed"}))
    s = _settings()
    job = _job(fake_db)
    assert (
        JobRunner(s, client=CoreClient(s)).run(job, fake_db["source"], None, _csv(tmp_path, 7), None)
        == "done"
    )
    assert fake_db["jobs"][job["id"]]["rejected_rows"] == 7


@respx.mock
def test_cooperative_cancel(fake_db: dict[str, Any], tmp_path: Path) -> None:
    job = _job(fake_db)

    def handler(request: httpx.Request) -> httpx.Response:
        fake_db["jobs"][job["id"]]["status"] = "cancelled"  # cancelled via API while running
        return httpx.Response(200, json={"accepted": 500, "rejected": 0})

    route = respx.post(URL).mock(side_effect=handler)
    s = _settings()
    assert (
        JobRunner(s, client=CoreClient(s)).run(job, fake_db["source"], None, _csv(tmp_path, 1500), None)
        == "cancelled"
    )
    assert route.call_count == 1


def test_sort_records_with_custom_format() -> None:
    recs = [{"t": "02/09/2026 10:00"}, {"t": "01/09/2026 23:00"}, {"t": None}]
    mapping = {
        "event": {
            "occurred_at": {"from": "t", "transform": [{"fn": "parse_datetime", "format": "%d/%m/%Y %H:%M"}]}
        }
    }
    assert sort_records(recs, mapping)
    assert [r["t"] for r in recs] == ["01/09/2026 23:00", "02/09/2026 10:00", None]
