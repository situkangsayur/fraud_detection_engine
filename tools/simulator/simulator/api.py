"""Thin client for the platform's PUBLIC gateway API (docs/technical/api-contract.md).

The simulator never touches databases or internal service ports — it behaves like a real client system
plus an operator using the UI.
"""

from __future__ import annotations

import time
from typing import Any

import httpx


class ApiError(Exception):
    def __init__(self, status: int, body: str, method: str, path: str) -> None:
        super().__init__(f"{method} {path} → {status}: {body[:400]}")
        self.status = status


class Gateway:
    def __init__(
        self, base_url: str, timeout: float = 120.0, retries: int = 6, client: httpx.Client | None = None
    ) -> None:
        self.http = client or httpx.Client(base_url=base_url.rstrip("/"), timeout=timeout)
        self.retries = retries
        self.token: str | None = None

    # ------------------------------------------------------------------ plumbing
    def request(
        self,
        method: str,
        path: str,
        *,
        json: Any = None,
        params: dict[str, Any] | None = None,
        headers: dict[str, str] | None = None,
        auth: bool = True,
        ok: tuple[int, ...] = (),
    ) -> Any:
        hdrs = dict(headers or {})
        if auth and self.token:
            hdrs["Authorization"] = f"Bearer {self.token}"
        delay = 1.0
        for attempt in range(1, self.retries + 1):
            try:
                resp = self.http.request(method, path, json=json, params=params, headers=hdrs)
            except httpx.TransportError:
                # flaky networks (e.g. IPv6 route issues) and services still starting
                if attempt == self.retries:
                    raise
            else:
                if resp.status_code < 300 or resp.status_code in ok:
                    return resp.json() if resp.content else None
                if resp.status_code not in (429, 502, 503, 504) or attempt == self.retries:
                    raise ApiError(resp.status_code, resp.text, method, path)
            time.sleep(delay)
            delay = min(delay * 2, 20.0)
        raise RuntimeError("unreachable")  # pragma: no cover

    @staticmethod
    def items(page: Any) -> list[dict[str, Any]]:
        if isinstance(page, dict) and "items" in page:
            return list(page["items"])
        return list(page or [])

    # ------------------------------------------------------------------ auth / tenancy
    def login(self, email: str, password: str) -> dict[str, Any]:
        data = self.request(
            "POST", "/api/v1/auth/login", json={"email": email, "password": password}, auth=False
        )
        self.token = data["access_token"]
        return dict(data)

    def find_tenant(self, slug: str) -> dict[str, Any] | None:
        for t in self.items(self.request("GET", "/api/v1/tenants", params={"page_size": 200})):
            if t.get("slug") == slug:
                return t
        return None

    def create_tenant(self, slug: str, name: str, admin: dict[str, str]) -> dict[str, Any]:
        return dict(
            self.request("POST", "/api/v1/tenants", json={"slug": slug, "name": name, "admin": admin})
        )

    def ensure_tenant_admin(self, tenant_id: str, email: str, password: str) -> None:
        body = {
            "email": email,
            "full_name": "Simulator Tenant Admin",
            "password": password,
            "tenant_role": "tenant_admin",
        }
        try:
            self.request("POST", f"/api/v1/tenants/{tenant_id}/users", json=body)
        except ApiError as e:
            if e.status != 409:
                raise
            for u in self.items(
                self.request("GET", f"/api/v1/tenants/{tenant_id}/users", params={"page_size": 200})
            ):
                if u.get("email") == email:
                    self.request(
                        "PATCH",
                        f"/api/v1/tenants/{tenant_id}/users/{u['id']}",
                        json={"password": password, "tenant_role": "tenant_admin", "is_active": True},
                    )

    # ------------------------------------------------------------------ projects / sources
    def find_project(self, slug: str) -> dict[str, Any] | None:
        for p in self.items(self.request("GET", "/api/v1/projects", params={"page_size": 200})):
            if p.get("slug") == slug:
                return p
        return None

    def create_project(self, body: dict[str, Any]) -> dict[str, Any]:
        return dict(self.request("POST", "/api/v1/projects", json=body))

    def find_source(self, pid: str, slug: str) -> dict[str, Any] | None:
        for s in self.items(
            self.request("GET", f"/api/v1/projects/{pid}/data-sources", params={"page_size": 200})
        ):
            if s.get("slug") == slug:
                return s
        return None

    def create_source(self, pid: str, body: dict[str, Any]) -> dict[str, Any]:
        return dict(self.request("POST", f"/api/v1/projects/{pid}/data-sources", json=body))

    def rotate_key(self, pid: str, sid: str) -> str:
        return str(self.request("POST", f"/api/v1/projects/{pid}/data-sources/{sid}/rotate-key")["api_key"])

    def list_mappings(self, pid: str, sid: str) -> list[dict[str, Any]]:
        return self.items(self.request("GET", f"/api/v1/projects/{pid}/data-sources/{sid}/mappings"))

    def create_mapping(self, pid: str, sid: str, mapping: dict[str, Any]) -> dict[str, Any]:
        return dict(
            self.request(
                "POST", f"/api/v1/projects/{pid}/data-sources/{sid}/mappings", json={"mapping": mapping}
            )
        )

    def activate_mapping(self, pid: str, sid: str, version: int) -> None:
        self.request("POST", f"/api/v1/projects/{pid}/data-sources/{sid}/mappings/{version}/activate")

    def count_events(self, pid: str, source_id: str) -> int:
        page = self.request(
            "GET", f"/api/v1/projects/{pid}/events", params={"source_id": source_id, "page_size": 1}
        )
        return int(page.get("total", 0)) if isinstance(page, dict) else 0

    # ------------------------------------------------------------------ data
    def ingest_batch(
        self, slug: str, api_key: str, records: list[dict[str, Any]], mode: str
    ) -> dict[str, Any]:
        return dict(
            self.request(
                "POST",
                f"/api/v1/ingest/{slug}/batch",
                json={"records": records, "mode": mode},
                headers={"X-Api-Key": api_key},
                auth=False,
            )
        )

    def find_event_id(self, pid: str, external_id: str) -> str | None:
        items = self.items(
            self.request("GET", f"/api/v1/projects/{pid}/events", params={"q": external_id, "page_size": 5})
        )
        for e in items:
            if e.get("external_id") == external_id:
                return str(e.get("id") or e.get("event_id"))
        return None

    def find_customer_id(self, pid: str, external_id: str) -> str | None:
        items = self.items(
            self.request(
                "GET", f"/api/v1/projects/{pid}/customers", params={"q": external_id, "page_size": 5}
            )
        )
        for c in items:
            if c.get("external_id") == external_id:
                return str(c["id"])
        return None

    def count_labels(self, pid: str) -> int:
        page = self.request("GET", f"/api/v1/projects/{pid}/labels", params={"page_size": 1})
        return int(page.get("total", 0)) if isinstance(page, dict) else 0

    def post_label(self, pid: str, body: dict[str, Any]) -> None:
        self.request("POST", f"/api/v1/projects/{pid}/labels", json=body)
