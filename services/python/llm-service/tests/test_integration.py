"""End-to-end against real Postgres (migrations + RLS roles) and OpenSearch; Ollama is faked.

Run: ``uv run pytest -m integration``. Skipped automatically when docker is unavailable.
"""

from __future__ import annotations

import shutil
import socket
import subprocess
import time
import uuid
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import httpx
import psycopg
import pytest
from fastapi.testclient import TestClient
from sqlalchemy import create_engine, text

from llm_service.clients.platform import PlatformClient
from llm_service.config import Settings
from llm_service.container import Container
from llm_service.main import create_app
from tests.conftest import auth
from tests.fakes import FakeOllama, tool_call
from tests.test_chunker import REGULATION

pytestmark = pytest.mark.integration

ROOT = Path(__file__).resolve().parents[4]
PG_IMAGE = "postgres:16.4-alpine"
OS_IMAGE = "opensearchproject/opensearch:2.13.0"
TENANT_A = uuid.UUID("11111111-1111-1111-1111-111111111111")
TENANT_B = uuid.UUID("22222222-2222-2222-2222-222222222222")
PROJECT_A = uuid.UUID("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def _docker(*args: str) -> str:
    return subprocess.run(["docker", *args], check=True, capture_output=True, text=True).stdout.strip()


def _image_available(image: str) -> bool:
    return subprocess.run(["docker", "image", "inspect", image], capture_output=True).returncode == 0


@pytest.fixture(scope="module")
def infra() -> Iterator[dict[str, Any]]:
    if shutil.which("docker") is None or not (_image_available(PG_IMAGE) and _image_available(OS_IMAGE)):
        pytest.skip("docker or images unavailable")
    pg_port, os_port = _free_port(), _free_port()
    suffix = uuid.uuid4().hex[:6]
    pg, osn = f"llm-it-pg-{suffix}", f"llm-it-os-{suffix}"
    env = {
        "POSTGRES_DB": "fraud",
        "POSTGRES_PASSWORD": "pw",
        "MIGRATOR_DB_PASSWORD": "m",
        "CORE_API_DB_PASSWORD": "c",
        "RULE_SERVICE_DB_PASSWORD": "r",
        "GRAPH_SERVICE_DB_PASSWORD": "g",
        "ML_SERVICE_DB_PASSWORD": "ml",
        "LLM_SERVICE_DB_PASSWORD": "l",
        "INGEST_SERVICE_DB_PASSWORD": "i",
    }
    env_args = [a for k, v in env.items() for a in ("-e", f"{k}={v}")]
    _docker(
        "run",
        "-d",
        "--rm",
        "--name",
        pg,
        *env_args,
        "-p",
        f"127.0.0.1:{pg_port}:5432",
        "-v",
        f"{ROOT}/deploy/postgres/init:/docker-entrypoint-initdb.d:ro",
        PG_IMAGE,
    )
    _docker(
        "run",
        "-d",
        "--rm",
        "--name",
        osn,
        "-p",
        f"127.0.0.1:{os_port}:9200",
        "-e",
        "discovery.type=single-node",
        "-e",
        "DISABLE_SECURITY_PLUGIN=true",
        "-e",
        "DISABLE_INSTALL_DEMO_CONFIG=true",
        "-e",
        "OPENSEARCH_JAVA_OPTS=-Xms512m -Xmx512m",
        OS_IMAGE,
    )
    try:
        migrator = create_engine(f"postgresql+psycopg://migrator:m@127.0.0.1:{pg_port}/fraud")
        deadline = time.time() + 90
        while True:
            try:
                with migrator.connect() as c:
                    c.execute(text("SELECT 1"))
                break
            except Exception:
                if time.time() > deadline:
                    raise
                time.sleep(1)
        raw = psycopg.connect(f"postgresql://migrator:m@127.0.0.1:{pg_port}/fraud", autocommit=True)
        with raw:
            for f in sorted((ROOT / "db" / "migrations").glob("*.sql")):
                raw.execute(f.read_text().encode())  # no params → '%s' inside DO-blocks stays literal
        with migrator.begin() as c:
            c.execute(
                text("INSERT INTO core.tenants (id, slug, name) VALUES (:a,'ta','A'), (:b,'tb','B')"),
                {"a": TENANT_A, "b": TENANT_B},
            )
            c.execute(
                text(
                    "INSERT INTO core.projects (id, tenant_id, slug, name, stage) "
                    "VALUES (:p, :t, 'checkout', 'Checkout', 'pre_payment')"
                ),
                {"p": PROJECT_A, "t": TENANT_A},
            )
        deadline = time.time() + 180
        while True:
            try:
                if httpx.get(f"http://127.0.0.1:{os_port}/_cluster/health", timeout=2).status_code == 200:
                    break
            except httpx.HTTPError:
                pass
            if time.time() > deadline:
                raise TimeoutError("opensearch did not start")
            time.sleep(2)
        yield {"pg_port": pg_port, "os_port": os_port, "migrator": migrator}
    finally:
        subprocess.run(["docker", "rm", "-f", pg, osn], capture_output=True)


@pytest.fixture(scope="module")
def it_settings(infra: dict[str, Any], tmp_path_factory: pytest.TempPathFactory) -> Settings:
    return Settings(
        database_url=f"postgresql+psycopg://llm_service:l@127.0.0.1:{infra['pg_port']}/fraud",
        opensearch_url=f"http://127.0.0.1:{infra['os_port']}",
        internal_api_token="test-internal-token",
        jwt_secret="test-jwt-secret-with-enough-length",
        core_api_url="http://127.0.0.1:9",
        rule_service_url="http://127.0.0.1:9",
        graph_service_url="http://127.0.0.1:9",
        ml_service_url="http://127.0.0.1:9",
        regulation_dir=str(tmp_path_factory.mktemp("regs")),
        tool_timeout_s=2.0,
    )


@pytest.fixture(scope="module")
def ollama() -> FakeOllama:
    return FakeOllama()


@pytest.fixture(scope="module")
def client(it_settings: Settings, ollama: FakeOllama, monkeypatch_module: pytest.MonkeyPatch) -> Iterator[TestClient]:
    # the engine is cached per process; point it at the integration database
    from llm_service import db

    db.get_engine.cache_clear()
    db._session_factory.cache_clear()
    monkeypatch_module.setattr("llm_service.db.get_settings", lambda: it_settings)
    container = Container.build(it_settings, ollama=ollama, platform=PlatformClient(it_settings))  # type: ignore[arg-type]
    with TestClient(create_app(it_settings, container)) as c:
        yield c
    db.get_engine.cache_clear()
    db._session_factory.cache_clear()


@pytest.fixture(scope="module")
def monkeypatch_module() -> Iterator[pytest.MonkeyPatch]:
    mp = pytest.MonkeyPatch()
    yield mp
    mp.undo()


def _wait_status(client: TestClient, headers: dict[str, str], reg_id: str, want: str) -> dict[str, Any]:
    deadline = time.time() + 60
    while time.time() < deadline:
        body = client.get(f"/api/v1/tenants/{TENANT_A}/regulations/{reg_id}", headers=headers).json()
        if body["status"] == want:
            return body  # type: ignore[no-any-return]
        assert body["status"] != "failed", body
        time.sleep(0.3)
    raise AssertionError(f"regulation {reg_id} never reached {want}")


def test_regulation_lifecycle_search_supersede_and_isolation(
    client: TestClient, it_settings: Settings, infra: dict[str, Any]
) -> None:
    admin = auth(it_settings, role=None, tenant_role="tenant_admin", tenant=TENANT_A, project=PROJECT_A)
    analyst = auth(it_settings, role="analyst", tenant=TENANT_A, project=PROJECT_A)

    # upload v1 → indexed
    r = client.post(
        f"/api/v1/tenants/{TENANT_A}/regulations",
        headers=analyst,
        files={"file": ("pojk-99.txt", REGULATION.encode(), "text/plain")},
        data={"code": "POJK-99-2030", "title": "Pencegahan Fraud", "issuer": "OJK"},
    )
    assert r.status_code == 202, r.text
    v1 = r.json()["regulation_id"]
    body = _wait_status(client, analyst, v1, "indexed")
    assert body["chunk_count"] >= 8 and body["version"] == 1
    # duplicate content → 409
    dup = client.post(
        f"/api/v1/tenants/{TENANT_A}/regulations",
        headers=analyst,
        files={"file": ("copy.txt", REGULATION.encode(), "text/plain")},
        data={"code": "POJK-99-2030", "title": "Duplikat", "issuer": "OJK"},
    )
    assert dup.status_code == 409

    # not attached yet → project search returns nothing
    s = client.post(
        f"/api/v1/projects/{PROJECT_A}/llm/regulations/search",
        headers=analyst,
        json={"query": "memblokir kartu carding"},
    )
    assert s.json()["items"] == []
    # attach (project_admin via tenant_admin) and search: hybrid retrieval finds Pasal 3
    put = client.put(f"/api/v1/projects/{PROJECT_A}/llm/regulations", headers=admin, json={"regulation_ids": [v1]})
    assert put.status_code == 200 and put.json()["regulation_ids"] == [v1]
    s = client.post(
        f"/api/v1/projects/{PROJECT_A}/llm/regulations/search",
        headers=analyst,
        json={"query": "memblokir kartu carding", "k": 3},
    )
    top = s.json()["items"][0]
    assert top["code"] == "POJK-99-2030" and top["section"].endswith("Pasal 3")

    # new version supersedes v1 → section diff + attachment follows the new version
    v2_text = REGULATION.replace("Rp100.000.000,00", "Rp25.000.000,00 atau akumulasi harian Rp50.000.000,00")
    r = client.post(
        f"/api/v1/tenants/{TENANT_A}/regulations",
        headers=analyst,
        files={"file": ("pojk-99-v2.txt", v2_text.encode(), "text/plain")},
        data={"code": "POJK-99-2030", "title": "Pencegahan Fraud (perubahan)", "issuer": "OJK", "supersedes_id": v1},
    )
    v2 = r.json()["regulation_id"]
    assert r.json()["version"] == 2
    _wait_status(client, analyst, v2, "indexed")
    _wait_status(client, analyst, v1, "superseded")  # diff + supersede run right after indexing
    detail = client.get(f"/api/v1/tenants/{TENANT_A}/regulations/{v2}", headers=analyst).json()
    changed = detail["changes"][0]["changed_sections"]
    assert [c["section"] for c in changed] == ["Pasal 2"] and changed[0]["change"] == "modified"
    attached = client.get(f"/api/v1/projects/{PROJECT_A}/llm/regulations", headers=analyst).json()
    assert attached["regulation_ids"] == [v2]

    # tenant isolation: API and database (RLS for the llm_service role)
    other = auth(it_settings, role="analyst", tenant=TENANT_B, project=uuid.uuid4())
    assert client.get(f"/api/v1/tenants/{TENANT_A}/regulations/{v1}", headers=other).status_code == 403
    eng = create_engine(it_settings.database_url)
    with eng.begin() as c:
        c.execute(text("SELECT set_config('app.tenant_id', :t, true)"), {"t": str(TENANT_B)})
        assert c.execute(text("SELECT count(*) FROM llm.regulations")).scalar_one() == 0
    with eng.begin() as c:
        assert c.execute(text("SELECT count(*) FROM llm.regulations")).scalar_one() == 0  # no tenant → nothing
    # attaching another tenant's regulation id is rejected even though the FK would allow it
    with infra["migrator"].begin() as c:
        c.execute(
            text("INSERT INTO core.projects (id, tenant_id, slug, name) VALUES (:p, :t, 'proj-b', 'B')"),
            {"p": uuid.UUID(int=7), "t": TENANT_B},
        )
    b_admin = auth(it_settings, role=None, tenant_role="tenant_admin", tenant=TENANT_B, project=uuid.UUID(int=7))
    r = client.put(
        f"/api/v1/projects/{uuid.UUID(int=7)}/llm/regulations", headers=b_admin, json={"regulation_ids": [v2]}
    )
    assert r.status_code == 422


def test_chat_persists_conversation_with_citations(
    client: TestClient, it_settings: Settings, ollama: FakeOllama
) -> None:
    analyst = auth(it_settings, role="analyst", tenant=TENANT_A, project=PROJECT_A)
    ollama.push(
        tool_call("search_regulations", query="blokir kartu carding"),
        {"content": "Kartu terindikasi carding wajib diblokir [POJK-99-2030 §Pasal 3]."},
    )
    r = client.post(
        f"/api/v1/projects/{PROJECT_A}/llm/chat",
        headers=analyst,
        json={"message": "Apa kewajiban untuk kartu carding?"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert "Pasal 3" in body["answer"] and body["citations"]
    assert body["tool_calls"][0]["name"] == "search_regulations"
    # the regulation text reached the model only inside a <tool_result> data block
    tool_msg = ollama.requests[-1]["messages"][-1]
    assert tool_msg["role"] == "tool" and "<tool_result" in tool_msg["content"]
    convs = client.get(f"/api/v1/projects/{PROJECT_A}/llm/conversations", headers=analyst).json()
    assert convs["total"] == 1
    detail = client.get(
        f"/api/v1/projects/{PROJECT_A}/llm/conversations/{body['conversation_id']}", headers=analyst
    ).json()
    assert [m["role"] for m in detail["messages"]] == ["user", "tool", "assistant"]
    # follow-up in the same conversation carries history
    ollama.push({"content": "Ya."})
    client.post(
        f"/api/v1/projects/{PROJECT_A}/llm/chat",
        headers=analyst,
        json={"conversation_id": body["conversation_id"], "message": "Yakin?"},
    )
    roles = [m["role"] for m in ollama.requests[-1]["messages"]]
    assert roles == ["system", "user", "assistant", "user"]


def test_fraud_situation_report_degrades_when_sources_down(
    client: TestClient, it_settings: Settings, ollama: FakeOllama
) -> None:
    analyst = auth(it_settings, role="analyst", tenant=TENANT_A, project=PROJECT_A)
    ollama.push({"json": {"summary_md": "## Situasi\nData terbatas.", "key_risks": []}})
    r = client.post(f"/api/v1/projects/{PROJECT_A}/llm/analysis/fraud-situation", headers=analyst, json={})
    assert r.status_code == 202
    rid = r.json()["report_id"]
    deadline = time.time() + 30
    while True:
        rep = client.get(f"/api/v1/projects/{PROJECT_A}/llm/reports/{rid}", headers=analyst).json()
        if rep["status"] != "running" or time.time() > deadline:
            break
        time.sleep(0.3)
    assert rep["status"] == "done", rep
    assert "analytics_overview" in rep["structured"]["data_gaps"]
    listing = client.get(f"/api/v1/projects/{PROJECT_A}/llm/reports", headers=analyst).json()
    assert listing["items"][0]["id"] == rid
