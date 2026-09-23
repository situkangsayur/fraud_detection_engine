"""Composition root: builds long-lived clients/services once per process (explicit DI, no globals in logic)."""

from __future__ import annotations

from dataclasses import dataclass

from fastapi import Request

from llm_service.agent.chat import ChatService
from llm_service.analysis.proposals import ProposalPipeline
from llm_service.analysis.service import AnalysisService
from llm_service.clients.ollama import OllamaClient
from llm_service.clients.platform import PlatformClient
from llm_service.clients.vector_store import VectorStore
from llm_service.config import Settings
from llm_service.jobs import JobRunner
from llm_service.project_context import ProjectContextLoader
from llm_service.regulations.service import RegulationService
from llm_service.retrieval.search import RegulationSearch


@dataclass
class Container:
    settings: Settings
    ollama: OllamaClient
    store: VectorStore
    platform: PlatformClient
    jobs: JobRunner
    search: RegulationSearch
    regulations: RegulationService
    proposals: ProposalPipeline
    contexts: ProjectContextLoader
    chat: ChatService
    analysis: AnalysisService

    @classmethod
    def build(
        cls,
        settings: Settings,
        *,
        ollama: OllamaClient | None = None,
        store: VectorStore | None = None,
        platform: PlatformClient | None = None,
    ) -> Container:
        ollama = ollama or OllamaClient(settings)
        store = store or VectorStore(settings)
        platform = platform or PlatformClient(settings)
        jobs = JobRunner(max_concurrency=2)
        search = RegulationSearch(store, ollama)
        proposals = ProposalPipeline(platform, ollama, settings.rule_repair_attempts)
        contexts = ProjectContextLoader(settings, platform)
        return cls(
            settings=settings,
            ollama=ollama,
            store=store,
            platform=platform,
            jobs=jobs,
            search=search,
            regulations=RegulationService(settings, store, ollama, jobs),
            proposals=proposals,
            contexts=contexts,
            chat=ChatService(settings, ollama, platform, search, proposals, contexts),
            analysis=AnalysisService(settings, ollama, platform, search, proposals, contexts, jobs),
        )

    async def aclose(self) -> None:
        await self.jobs.shutdown()
        await self.ollama.aclose()
        await self.platform.aclose()


def get_container(request: Request) -> Container:
    return request.app.state.container  # type: ignore[no-any-return]
