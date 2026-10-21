# Dokumentasi — Fraud Detection Platform v2

Platform deteksi fraud multi-tenant: **5 engine** (rule, ML supervised, ML unsupervised, graph, LLM) menghasilkan
**satu keputusan** (approve / review / decline) dengan alasan, case management, dan governance maker–checker.

**Status (2026-10-21):** stack demo berjalan dari checkout deploy `~/own_project/deployment/fraud_detection_engine` dengan autostart systemd; LLM di GPU (qwen3:8b, Ollama 0.40.7), chat bisa mengecek pelanggan/transaksi/kasus; PRD/TRD AI Console ditulis.
**Status (2026-10-13):** v2 berjalan sebagai demo publik di `fds.hendrikarisma.my.id` (host nvda11-gpu, commit
`07ee765`). LLM memakai Ollama lokal (qwen3:8b). Lapisan **multi-agent** sudah didesain, belum diimplementasi.
Lihat [roadmap](roadmap.md).

> Konvensi: dokumen **bisnis & panduan** berbahasa Indonesia, dokumen **teknis** berbahasa Inggris.
> Dokumen teknis adalah kontrak: jika kode dan dokumen berbeda, perbaiki salah satunya di perubahan yang sama.

## Mulai dari mana?

| Saya … | Baca |
|---|---|
| manajemen / bisnis, ingin tahu nilai & status | [Ringkasan non-teknis](business/overview.md) → [Use case](business/use-cases.md) → [Roadmap](roadmap.md) |
| compliance / auditor | [Governance & kepatuhan](business/governance-compliance.md) |
| presenter demo | [Script demo](business/demo-script.md) |
| analis / approver | [Panduan pengguna](guides/user-guide.md) |
| engineer baru | [Technical overview](technical/technical-overview.md) → [Architecture](technical/architecture.md) → [Developer guide](guides/developer-guide.md) |
| ops / yang menjaga demo | [Deployment](guides/deployment.md) → [Runbook operasional](guides/operations-runbook.md) |
| peneliti | [Evaluation](technical/evaluation.md) → [Experiments](guides/experiments.md) → [Test data](technical/test-data.md) |

## Peta dokumen

### Non-teknis (`business/`)
| Dokumen | Isi |
|---|---|
| [overview.md](business/overview.md) | Ringkasan eksekutif: masalah, solusi, nilai, angka kunci, batasan AI |
| [use-cases.md](business/use-cases.md) | Kapabilitas per stage project dan per persona |
| [governance-compliance.md](business/governance-compliance.md) | Maker–checker, audit, PII, UU PDP, POJK, tata kelola AI |
| [demo-script.md](business/demo-script.md) | Skenario demo langkah demi langkah |
| [glossary.md](business/glossary.md) | Glosarium istilah |
| [prd-agent-ai-console.md](business/prd-agent-ai-console.md) | **PRD** Multi-agent & AI Console (kelola agent, model, provider, API key) + status yang sudah/belum |

### Panduan (`guides/`)
| Dokumen | Isi |
|---|---|
| [quickstart.md](guides/quickstart.md) | Menjalankan stack secara lokal |
| [user-guide.md](guides/user-guide.md) | Panduan pengguna per fitur: story, langkah, screenshot (tema terang) |
| [developer-guide.md](guides/developer-guide.md) | Struktur repo, build, test, konvensi |
| [deployment.md](guides/deployment.md) | Deploy dengan docker compose, profile GPU/LLM |
| [operations-runbook.md](guides/operations-runbook.md) | Operasional harian, reset demo, insiden umum, konsolidasi direktori |
| [experiments.md](guides/experiments.md) | Seeding riset, dataset, reproduksi eksperimen |

### Teknis (`technical/`)
| Dokumen | Isi |
|---|---|
| [technical-overview.md](technical/technical-overview.md) | Tur satu dokumen: stack, arsitektur, AI, metode |
| [architecture.md](technical/architecture.md) | Kontrak arsitektur, service, alur scoring |
| [api-contract.md](technical/api-contract.md) | Endpoint & konvensi API |
| [multi-tenancy.md](technical/multi-tenancy.md) | Tenant, project, RLS, peran |
| [rule-dsl.md](technical/rule-dsl.md) | Spesifikasi Rule DSL |
| [feature-catalog.md](technical/feature-catalog.md) | Katalog fitur untuk rule & ML |
| [data-sources.md](technical/data-sources.md) | Sumber data, mapping, ingest |
| [ml-plugins.md](technical/ml-plugins.md) | Kontrak plugin ML |
| [llm-agentic.md](technical/llm-agentic.md) | LLM assistant & agent saat ini (RAG, tools, proposal) |
| [multi-agent.md](technical/multi-agent.md) | **Desain** lapisan multi-agent (F1–F9) |
| [trd-agent-ai-console.md](technical/trd-agent-ai-console.md) | **TRD** AI Console: data model, resolver, multi-model, enkripsi key, API, UI |
| [ux-design.md](technical/ux-design.md) | **Desain** UX & alur untuk fitur agentic |
| [bank-datasets.md](technical/bank-datasets.md) | **Dataset bank publik** berreferensi: karakter, lisensi, kasus demo/riset |
| [integration-extensibility.md](technical/integration-extensibility.md) | Plugin & integrasi: struktur data/request/response klien, apa yang bisa dikonfigurasi |
| [test-data.md](technical/test-data.md) | Katalog data uji publik per kasus & golden set |
| [evaluation.md](technical/evaluation.md) | Hasil evaluasi deteksi |
| [research-dataset.md](technical/research-dataset.md) | Skema dataset riset hasil ekspor |
| [rust-codebase-guide.md](technical/rust-codebase-guide.md) | Struktur kode Rust untuk developer |
| [gap-analysis.md](technical/gap-analysis.md) | (Legacy) analisis kekurangan v1 |

### Perencanaan
| Dokumen | Isi |
|---|---|
| [backlog.md](backlog.md) | Epic E0–E17, story, status, gap terukur G1–G8 |
| [roadmap.md](roadmap.md) | Fase, milestone, blocker, matriks prioritas |
| [CHANGELOG.md](CHANGELOG.md) | Riwayat perubahan |
| [plans/](plans/) | Plan kerja per sesi |

## Lokasi

| Apa | Di mana |
|---|---|
| Repo | `github.com/situkangsayur/fraud_detection_engine`, branch `revamp/rust-nuxt-stack` (v2) |
| Checkout (nvda11-gpu) | deploy `~/own_project/deployment/fraud_detection_engine` (stack + cron + systemd `fraud-platform.service`); dev & docs `~/own_project/fraud_detection_engine` |
| Demo publik | `https://fds.hendrikarisma.my.id` (reset harian 00:00) |
| Salinan dokumentasi | Vault Obsidian (p14s) `MyNotes/nvda11/fraud-project/dokumentasi/` — disinkronkan dengan `docs/sync-to-vault.sh` |
| Ide & spec awal | Vault `fraud-project/pengembangan/`, paper di `fraud-project/paper-fraud-detection/` |
