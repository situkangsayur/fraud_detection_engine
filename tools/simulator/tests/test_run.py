from __future__ import annotations

import json
from datetime import UTC, datetime

import httpx
import respx

from simulator.api import Gateway
from simulator.run import Settings, run
from simulator.scenarios import generate_all

GW = "http://gw.test"
TID, PID, SID = "t-1", "p-1", "s-1"


def _settings() -> Settings:
    return Settings(
        gateway_url=GW,
        admin_email="admin@x",
        admin_password="pw",
        tenant="demo",
        tenant_admin_email="sim-admin@demo.local",
        tenant_admin_password="pw",
        seed=3,
    )


@respx.mock
def test_full_flow_against_mock_gateway(monkeypatch) -> None:
    monkeypatch.setattr("simulator.api.time.sleep", lambda s: None)
    batches: list[dict] = []
    labels: list[dict] = []
    logins = {"n": 0}

    def login(req: httpx.Request) -> httpx.Response:
        body = json.loads(req.content)
        logins["n"] += 1
        if body["email"] == "sim-admin@demo.local" and logins["n"] == 1:
            return httpx.Response(401, json={"title": "Unauthorized"})  # TA does not exist yet
        return httpx.Response(200, json={"access_token": f"tok-{body['email']}"})

    def ingest(req: httpx.Request) -> httpx.Response:
        assert req.headers["x-api-key"] == "key-123"
        assert "authorization" not in req.headers
        b = json.loads(req.content)
        batches.append(b)
        return httpx.Response(
            200,
            json={
                "accepted": len(b["records"]),
                "rejected": 0,
                "errors": [],
                "decisions": [
                    {
                        "external_id": r["no_ref"],
                        "event_id": f"e-{r['no_ref']}",
                        "decision": "approve",
                        "final_score": 1.0,
                    }
                    for r in b["records"]
                ]
                if b["mode"] == "score"
                else [],
            },
        )

    def label(req: httpx.Request) -> httpx.Response:
        labels.append(json.loads(req.content))
        return httpx.Response(201, json={"id": "l"})

    respx.post(f"{GW}/api/v1/auth/login").mock(side_effect=login)
    respx.get(f"{GW}/api/v1/tenants").mock(return_value=httpx.Response(200, json={"items": [], "total": 0}))
    respx.post(f"{GW}/api/v1/tenants").mock(
        return_value=httpx.Response(201, json={"id": TID, "slug": "demo"})
    )
    respx.get(f"{GW}/api/v1/projects").mock(return_value=httpx.Response(200, json={"items": []}))
    respx.post(f"{GW}/api/v1/projects").mock(
        return_value=httpx.Response(201, json={"id": PID, "slug": "checkout"})
    )
    src_list = respx.get(f"{GW}/api/v1/projects/{PID}/data-sources").mock(
        side_effect=[
            httpx.Response(200, json={"items": []}),
            httpx.Response(200, json={"items": [{"id": SID, "slug": "sim-webhook"}]}),
        ]
    )
    respx.post(f"{GW}/api/v1/projects/{PID}/data-sources").mock(
        return_value=httpx.Response(201, json={"id": SID, "slug": "sim-webhook", "api_key": "key-123"})
    )
    respx.get(f"{GW}/api/v1/projects/{PID}/data-sources/{SID}/mappings").mock(
        return_value=httpx.Response(200, json={"items": []})
    )
    respx.post(f"{GW}/api/v1/projects/{PID}/data-sources/{SID}/mappings").mock(
        return_value=httpx.Response(201, json={"version": 1, "status": "draft"})
    )
    act = respx.post(f"{GW}/api/v1/projects/{PID}/data-sources/{SID}/mappings/1/activate").mock(
        return_value=httpx.Response(200, json={})
    )
    respx.get(f"{GW}/api/v1/projects/{PID}/events").mock(
        side_effect=lambda r: httpx.Response(
            200, json={"items": [{"id": "e-x", "external_id": r.url.params.get("q")}], "total": 0}
        )
    )
    respx.get(f"{GW}/api/v1/projects/{PID}/customers").mock(
        side_effect=lambda r: httpx.Response(
            200, json={"items": [{"id": "c-x", "external_id": r.url.params.get("q")}]}
        )
    )
    respx.get(f"{GW}/api/v1/projects/{PID}/labels").mock(
        return_value=httpx.Response(200, json={"items": [], "total": 0})
    )
    respx.post(f"{GW}/api/v1/ingest/sim-webhook/batch").mock(side_effect=ingest)
    respx.post(f"{GW}/api/v1/projects/{PID}/labels").mock(side_effect=label)

    ds = generate_all(120, 20, 3, datetime(2026, 9, 20, tzinfo=UTC), ["checkout"])
    reports = run(Gateway(GW), ds, _settings())
    rep = reports[0]
    assert act.called and src_list.call_count == 2
    sent = [r for b in batches for r in b["records"]]
    assert len(sent) == len(ds[0].events) == rep.accepted
    assert all(len(b["records"]) <= 500 for b in batches)
    modes = [b["mode"] for b in batches]
    assert modes == sorted(modes)  # all load_only first, then score ("load_only" < "score")
    assert rep.sent["load_only"] > rep.sent["score"] > 0
    assert rep.decisions["approve"] == rep.sent["score"]
    assert labels and all(lb["subject_type"] in ("event", "customer") for lb in labels)
    assert any(lb["label"] == "fraud" and lb.get("fraud_type") for lb in labels)
    assert rep.event_labels + rep.customer_labels == len(labels)


@respx.mock
def test_expired_token_triggers_one_relogin() -> None:
    logins = respx.post("http://gw.test/api/v1/auth/login").mock(
        side_effect=[
            httpx.Response(200, json={"access_token": "old"}),
            httpx.Response(200, json={"access_token": "new"}),
        ]
    )
    labels = respx.get("http://gw.test/api/v1/projects/p/labels").mock(
        side_effect=lambda req: (
            httpx.Response(200, json={"items": []})
            if req.headers["authorization"] == "Bearer new"
            else httpx.Response(401, json={"detail": "invalid token: ExpiredSignature"})
        )
    )
    gw = Gateway("http://gw.test", retries=2)
    gw.login("a@b", "pw")
    assert gw.request("GET", "/api/v1/projects/p/labels") == {"items": []}
    assert logins.call_count == 2 and labels.call_count == 2
