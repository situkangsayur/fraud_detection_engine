"""Regulation / policy library: upload → extract → chunk → embed → index, versioning and change diff."""

from __future__ import annotations

import hashlib
import re
import uuid
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Any

from starlette.concurrency import run_in_threadpool

from llm_service import prompts
from llm_service import repository as repo
from llm_service.clients.ollama import OllamaClient
from llm_service.clients.vector_store import ChunkDoc, VectorStore
from llm_service.config import Settings
from llm_service.db import tenant_session
from llm_service.errors import ProblemError, not_found
from llm_service.jobs import JobRunner
from llm_service.logging import get_logger
from llm_service.metrics import REGULATIONS_INDEXED
from llm_service.regulations.chunker import chunk_document, section_texts
from llm_service.regulations.diff import diff_sections
from llm_service.regulations.extract import SUPPORTED_EXTENSIONS, ExtractionError, extract_text

log = get_logger(__name__)
_SAFE_NAME = re.compile(r"[^A-Za-z0-9._-]+")


@dataclass(frozen=True)
class UploadMeta:
    code: str
    title: str
    doc_type: str
    issuer: str
    effective_date: date | None
    supersedes_id: uuid.UUID | None


class RegulationService:
    def __init__(self, settings: Settings, store: VectorStore, ollama: OllamaClient, jobs: JobRunner) -> None:
        self._s = settings
        self._store = store
        self._ollama = ollama
        self._jobs = jobs

    # ------------------------------------------------------------------ upload
    async def upload(
        self, tenant_id: uuid.UUID, data: bytes, file_name: str, meta: UploadMeta, actor: uuid.UUID | None
    ) -> dict[str, Any]:
        ext = Path(file_name).suffix.lower()
        if ext not in SUPPORTED_EXTENSIONS:
            raise ProblemError(415, "Unsupported Media Type", f"supported: {sorted(SUPPORTED_EXTENSIONS)}")
        if len(data) > self._s.max_upload_mb * 1024 * 1024:
            raise ProblemError(413, "Payload Too Large", f"max {self._s.max_upload_mb} MB")
        if not data:
            raise ProblemError(422, "Validation failed", "empty file")
        sha = hashlib.sha256(data).hexdigest()
        reg_id = uuid.uuid4()
        safe = _SAFE_NAME.sub("_", Path(file_name).name)[:120] or f"document{ext}"
        path = Path(self._s.regulation_dir) / str(tenant_id) / str(reg_id) / safe

        def _insert() -> dict[str, Any]:
            with tenant_session(tenant_id) as s:
                existing = repo.find_regulation_by_sha(s, tenant_id, sha)
                if existing:
                    raise ProblemError(
                        409,
                        "Conflict",
                        f"identical document already uploaded as "
                        f"{existing['code']} v{existing['version']} ({existing['id']})",
                    )
                if meta.supersedes_id is not None:
                    prev = repo.get_regulation(s, meta.supersedes_id)
                    if prev is None:
                        raise not_found("superseded regulation")
                    if prev["status"] != "indexed":
                        raise ProblemError(409, "Conflict", "superseded regulation is not indexed")
                version = repo.next_regulation_version(s, tenant_id, meta.code)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                return repo.insert_regulation(
                    s,
                    reg_id=reg_id,
                    tenant_id=tenant_id,
                    code=meta.code,
                    title=meta.title,
                    doc_type=meta.doc_type,
                    issuer=meta.issuer,
                    version=version,
                    effective_date=meta.effective_date,
                    supersedes_id=meta.supersedes_id,
                    file_name=safe,
                    sha=sha,
                    file_path=str(path),
                    uploaded_by=actor,
                )

        row = await run_in_threadpool(_insert)
        self._jobs.submit(f"index-regulation-{reg_id}", lambda: self.process(tenant_id, reg_id))
        return {"regulation_id": str(reg_id), "status": "processing", "version": row["version"]}

    # ------------------------------------------------------------------ processing job
    async def process(self, tenant_id: uuid.UUID, reg_id: uuid.UUID, language: str = "id") -> None:
        reg = await run_in_threadpool(self._get, tenant_id, reg_id)
        try:
            data = await run_in_threadpool(Path(reg["file_path"]).read_bytes)
            text = await run_in_threadpool(extract_text, data, reg["file_name"])
            chunks = chunk_document(text)
            if not chunks:
                raise ExtractionError("no chunks produced")
            vectors = await self._ollama.embed([f"{c.section}\n{c.text}" for c in chunks])
            eff = reg["effective_date"].isoformat() if reg["effective_date"] else None
            docs = [
                ChunkDoc(
                    chunk_id=f"{reg_id}:{c.ordinal}",
                    regulation_id=str(reg_id),
                    code=reg["code"],
                    version=int(reg["version"]),
                    doc_type=reg["doc_type"],
                    issuer=reg["issuer"],
                    effective_date=eff,
                    section=c.section,
                    title=c.title,
                    text=c.text,
                    ordinal=c.ordinal,
                    embedding=v,
                )
                for c, v in zip(chunks, vectors, strict=True)
            ]
            await self._store.index_chunks(tenant_id, docs)
            await run_in_threadpool(
                self._update, tenant_id, reg_id, status="indexed", chunk_count=len(docs), error=None
            )
            REGULATIONS_INDEXED.inc()
            log.info("regulation_indexed", regulation_id=str(reg_id), chunks=len(docs))
        except Exception as exc:
            log.exception("regulation_index_failed", regulation_id=str(reg_id))
            await run_in_threadpool(self._update, tenant_id, reg_id, status="failed", error=str(exc)[:2000])
            return

        # Best-effort enrichments: failures are logged but never un-index the document.
        try:
            summary = await self._summarise(reg, text, language)
            await run_in_threadpool(self._update, tenant_id, reg_id, summary=summary)
        except Exception:
            log.exception("regulation_summary_failed", regulation_id=str(reg_id))
        if reg["supersedes_id"]:
            try:
                await self._record_change(tenant_id, reg, chunks, language)
            except Exception:
                log.exception("regulation_diff_failed", regulation_id=str(reg_id))

    async def _summarise(self, reg: dict[str, Any], text: str, language: str) -> str:
        prompt = prompts.render(
            "regulation_summary.md.j2",
            language=language,
            code=reg["code"],
            title=reg["title"],
            issuer=reg["issuer"],
            text=text[:24000],
        )
        msg = await self._ollama.chat([{"role": "user", "content": prompt}])
        return str(msg.get("content", "")).strip()

    async def _record_change(
        self, tenant_id: uuid.UUID, reg: dict[str, Any], new_chunks: list[Any], language: str
    ) -> None:
        prev = await run_in_threadpool(self._get, tenant_id, reg["supersedes_id"])
        old_bytes = await run_in_threadpool(Path(prev["file_path"]).read_bytes)
        old_text = await run_in_threadpool(extract_text, old_bytes, prev["file_name"])
        changes = diff_sections(section_texts(chunk_document(old_text)), section_texts(new_chunks))
        summary: str | None = None
        if changes:
            prompt = prompts.render(
                "regulation_change_summary.md.j2",
                language=language,
                changes=[c.as_dict() for c in changes[:40]],
                old_code=prev["code"],
                old_version=prev["version"],
                new_code=reg["code"],
                new_version=reg["version"],
            )
            summary = str((await self._ollama.chat([{"role": "user", "content": prompt}])).get("content", ""))
        else:
            summary = (
                "Tidak ada perubahan substantif pada tingkat Pasal."
                if language == "id"
                else "No substantive article-level changes."
            )

        def _save() -> None:
            with tenant_session(tenant_id) as s:
                repo.insert_change(
                    s,
                    tenant_id=tenant_id,
                    regulation_id=reg["id"],
                    previous_id=prev["id"],
                    changed_sections=[c.as_dict() for c in changes],
                    diff_summary=summary,
                )
                repo.update_regulation(s, prev["id"], status="superseded")
                repo.move_attachments(s, prev["id"], reg["id"])

        await run_in_threadpool(_save)

    # ------------------------------------------------------------------ helpers
    @staticmethod
    def _get(tenant_id: uuid.UUID, reg_id: uuid.UUID) -> dict[str, Any]:
        with tenant_session(tenant_id) as s:
            row = repo.get_regulation(s, reg_id)
        if row is None:
            raise not_found("regulation")
        return row

    @staticmethod
    def _update(tenant_id: uuid.UUID, reg_id: uuid.UUID, **fields: Any) -> None:
        with tenant_session(tenant_id) as s:
            repo.update_regulation(s, reg_id, **fields)

    async def delete(self, tenant_id: uuid.UUID, reg_id: uuid.UUID) -> None:
        reg = await run_in_threadpool(self._get, tenant_id, reg_id)
        await self._store.delete_regulation(tenant_id, reg_id)

        def _del() -> None:
            with tenant_session(tenant_id) as s:
                repo.delete_regulation(s, reg_id)

        await run_in_threadpool(_del)
        try:
            await run_in_threadpool(Path(reg["file_path"]).unlink, missing_ok=True)
        except OSError:
            log.warning("regulation_file_delete_failed", path=reg["file_path"])
