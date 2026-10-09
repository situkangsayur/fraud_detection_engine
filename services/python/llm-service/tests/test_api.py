from __future__ import annotations

from collections.abc import AsyncIterator, Iterator
from typing import Any

import pytest
from fastapi.testclient import TestClient

from llm_service.clients.platform import PlatformClient
from llm_service.container import Container
from llm_service.main import create_app
from tests.conftest import PROJECT, TENANT, auth, internal_headers
from tests.fakes import FakeOllama


class FakeStore:
    async def ping(self) -> bool:
        return True


@pytest.fixture
def ollama() -> FakeOllama:
    return FakeOllama()


@pytest.fixture
def client(settings: Any, ollama: FakeOllama) -> Iterator[TestClient]:
    container = Container.build(
        settings,
        ollama=ollama,
        store=FakeStore(),  # type: ignore[arg-type]
        platform=PlatformClient(settings),
    )
    with TestClient(create_app(settings, container)) as c:
        yield c


def test_health_and_metrics(client: TestClient) -> None:
    assert client.get("/health/live").json() == {"status": "ok"}
    r = client.get("/health/live", headers={"x-request-id": "abc"})
    assert r.headers["x-request-id"] == "abc"
    assert "llm_http_requests_total" in client.get("/metrics").text
    assert client.get("/openapi.json").status_code == 200


def test_unauthenticated_is_problem_json(client: TestClient) -> None:
    r = client.post(f"/api/v1/projects/{PROJECT}/llm/chat", json={"message": "hai"})
    assert r.status_code == 401
    assert r.headers["content-type"].startswith("application/problem+json")
    assert r.json()["status"] == 401


def test_role_enforcement(client: TestClient, settings: Any) -> None:
    r = client.post(
        f"/api/v1/projects/{PROJECT}/llm/analysis/recommend-rules", json={}, headers=auth(settings, role="viewer")
    )
    assert r.status_code == 403
    r = client.put(
        f"/api/v1/projects/{PROJECT}/llm/regulations",
        json={"regulation_ids": []},
        headers=auth(settings, role="analyst"),
    )
    assert r.status_code == 403
    r = client.get(f"/api/v1/tenants/{'2' * 8}-2222-2222-2222-222222222222/regulations", headers=auth(settings))
    assert r.status_code == 403
    r = client.post(f"/api/v1/projects/{PROJECT}/llm/chat", json={"message": "x"}, headers=auth(settings, role=None))
    assert r.status_code == 404


def test_validation_errors(client: TestClient, settings: Any) -> None:
    r = client.post(f"/api/v1/projects/{PROJECT}/llm/analysis/regulation-impact", json={}, headers=auth(settings))
    assert r.status_code == 422 and r.json()["errors"][0]["field"].endswith("regulation_id")


def test_upload_rejects_unsupported_type(client: TestClient, settings: Any) -> None:
    r = client.post(
        f"/api/v1/tenants/{TENANT}/regulations",
        headers=auth(settings),
        files={"file": ("virus.exe", b"MZ", "application/octet-stream")},
        data={"code": "X-1", "title": "Test", "issuer": "OJK"},
    )
    assert r.status_code == 415


def test_mapping_suggest_internal_only_and_sanitized(client: TestClient, settings: Any, ollama: FakeOllama) -> None:
    body = {
        "fields": [
            {"path": "jumlah", "inferred_type": "number", "sample_values": [1000]},
            {"path": "nominal_bayar", "inferred_type": "number"},
            {"path": "tgl", "inferred_type": "datetime"},
        ],
        "canonical_fields": ["amount", "occurred_at", "customer_external_id"],
    }
    assert client.post("/v1/mapping/suggest", json=body, headers=auth(settings)).status_code == 403
    ollama.push(
        {
            "json": {
                "suggestions": [
                    {"source_path": "jumlah", "target": "amount", "confidence": 0.9, "reason": "jumlah = amount"},
                    {"source_path": "nominal_bayar", "target": "amount", "confidence": 0.7, "reason": "nominal"},
                    {"source_path": "tgl", "target": "occurred_at", "confidence": 0.95, "reason": "tanggal"},
                    {"source_path": "hallucinated", "target": "amount", "confidence": 1.0, "reason": "?"},
                    {"source_path": "tgl", "target": "not_canonical", "confidence": 0.4, "reason": "?"},
                ]
            }
        }
    )
    r = client.post("/v1/mapping/suggest", json=body, headers=internal_headers(settings))
    assert r.status_code == 200
    s = r.json()["suggestions"]
    assert {x["source_path"] for x in s} == {"jumlah", "nominal_bayar", "tgl"}
    targets = {(x["source_path"], x["target"]) for x in s}
    assert ("jumlah", "amount") in targets and ("nominal_bayar", None) in targets
    assert ("tgl", "occurred_at") in targets and ("tgl", None) in targets


def test_chat_stream_sse_format(client: TestClient, settings: Any) -> None:
    async def fake_stream(*_: Any, **__: Any) -> AsyncIterator[dict[str, Any]]:
        yield {"type": "conversation", "conversation_id": "c-1"}
        yield {"type": "token", "content": "Halo"}
        yield {"type": "tool", "name": "list_rules", "ok": True}
        yield {"type": "done", "answer": "Halo", "citations": []}

    client.app.state.container.chat.chat_stream = fake_stream  # type: ignore[attr-defined]
    with client.stream(
        "POST",
        f"/api/v1/projects/{PROJECT}/llm/chat/stream",
        json={"message": "hai"},
        headers=auth(settings, role="viewer"),
    ) as r:
        assert r.headers["content-type"].startswith("text/event-stream")
        text = "".join(r.iter_text())
    assert 'event: token\ndata: {"content": "Halo"}\n\n' in text
    assert text.index("event: tool") < text.index("event: done")
