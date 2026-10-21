---
title: Plan — Dokumentasi Terpadu, Backlog, Roadmap & Demo Multi-Agent
tags:
  - fraud-detection
  - plan
  - dokumentasi
  - multi-agent
created: 2026-10-13
status: tahap-A-selesai (dokumentasi), tahap-0 sebagian
---

# Plan — Dokumentasi Terpadu, Backlog, Roadmap & Demo Multi-Agent

Terkait: [[00 Index Pengembangan]] · [[02 Spec Arsitektur Multi-Agent]] · [[03 Spec Fitur Agentic]] · [[06 Roadmap dan Prioritas]]

## 0. Konteks & keputusan dasar

| Hal | Keputusan |
|---|---|
| Versi acuan | **v2**: branch `revamp/rust-nuxt-stack` (HEAD `07ee765`, 2026-10-10) = versi terbaru = versi yang ter-deploy di `fds.hendrikarisma.my.id`. v1 (`master`, Python/Mongo) hanya dicatat sebagai legacy. |
| Host deploy & dev utama | **nvda11-gpu**: stack compose `fraud-platform` (demo publik) + `fraud-eval` (evaluasi dataset publik) |
| Host vault / dev kedua | **p14s** (`10.100.21.66:1313`, alias `ssh p14s`) |
| Bukan host proyek ini | nvda11svr (`10.100.21.206:1313` = `192.168.11.11:1313`): tidak ada repo fraud |
| Direktori repo (target) | **satu saja per host: `~/own_project/fraud_detection_engine`** (lihat Tahap 0) |
| Dokumen di repo | Branch `docs/unified-docs` dari `revamp/rust-nuxt-stack`. `docs/` adalah sumber utama (canonical). |
| Dokumen di vault | `fraud-project/dokumentasi/` di p14s: salinan dari repo + index Obsidian (wikilink, callout). Disinkronkan dengan skrip `rsync`, bukan ditulis dua kali. |
| Sumber fakta | Kode v2 + `docs/` repo + `pengembangan/` + `paper-fraud-detection/`. Setiap klaim dicek ke kode (docs terakhir 2026-10-07, kode 2026-10-10). |

## Tahap 0 — Konsolidasi direktori repo (satu direktori per host)

### Kondisi sekarang (dicek 2026-10-13)

| Host | Direktori | Isi | Catatan |
|---|---|---|---|
| nvda11-gpu | `~/own_project/fraud_detection_engine` | **v1 legacy** (`master` 03731ae), 131 MB, ada `data/mongo_dev`, `.env.dev` | tidak dipakai; kode v1 aman di GitHub (`master`, `legacy/python-fastapi-mongo`) |
| nvda11-gpu | `~/own_project/apps/fraud_detection_engine` | **v2 terbaru** (`07ee765`), 5,8 GB, `.env`, `.venv` | **yang ter-deploy**: compose `fraud-platform` + `fraud-eval`, cron `00:00 deploy/seed.sh restore`, worktree `~/fraud-eval/repo` (branch `eval/public-datasets`, **2 commit belum di-push**) |
| p14s | `~/own_project/fraud_detection_engine` | v2 terbaru (`07ee765`), clone bersih 15 MB (remote https) | kandidat utama di p14s |
| p14s | `~/own_project/apps/fraud_detection_engine` | v2 tertinggal 3 commit (`e365cb1`), 23 GB, 1 stash ("p14s WIP 2026-10-07… superseded") | menjalankan stack `fraud-platform` + ollama lokal |

### Langkah usulan

**nvda11-gpu** (downtime ±5–10 menit, jangan dekat jam 00:00):
1. Push `eval/public-datasets` (2 commit) ke origin sebagai backup.
2. Arsipkan v1: `mv ~/own_project/fraud_detection_engine ~/own_project/_archive/fraud_detection_engine-v1` (data Mongo milik root ikut terbawa; hapus nanti setelah dipastikan tidak perlu).
3. `docker compose down` untuk `fraud-eval` lalu `fraud-platform` (named volume Postgres/OpenSearch **tidak** dihapus; nama project compose tetap `fraud-platform`).
4. `mv ~/own_project/apps/fraud_detection_engine ~/own_project/fraud_detection_engine`.
5. Perbaiki referensi path: crontab restore, `~/fraud-eval/compose.eval.yml`, `git worktree repair`, buat ulang `.venv` (path absolut), cek bind mount di `docker-compose.yml`.
6. `docker compose up -d` dari direktori baru → cek semua container healthy + `fds.hendrikarisma.my.id` login & dashboard + `fraud-eval`.
7. Opsional: symlink `~/own_project/apps/fraud_detection_engine → ../fraud_detection_engine` selama masa transisi.

**p14s:**
1. Simpan stash sebagai patch ke arsip (`git stash show -p > ~/own_project/_archive/p14s-wip-2026-10-07.patch`).
2. Tentukan: stack `fraud-platform` di p14s masih perlu atau tidak (data volume ±23 GB, ollama).
   - Perlu → pindahkan stack ke `~/own_project/fraud_detection_engine` (pakai `.env` lama).
   - Tidak → `docker compose down` lalu hapus `apps/fraud_detection_engine`.
3. `~/own_project/fraud_detection_engine` diselaraskan ke remote SSH yang sama dengan nvda11-gpu.

Verifikasi akhir: di kedua host hanya ada `~/own_project/fraud_detection_engine`, HEAD sama dengan `origin/revamp/rust-nuxt-stack`, demo publik berjalan normal setelah restore 00:00 berikutnya.

## 1. Tahap A — Dokumentasi terpadu (teknis + non-teknis)

Repo `docs/` sudah punya `business/overview.md`, `guides/*` (5), `technical/*` (12), `backlog.md`. Rencananya: **perbarui yang ada, tambah yang belum ada**, lalu buat index terpadu.

| Dokumen (repo) | Aksi | Isi |
|---|---|---|
| `docs/README.md` | 🆕 | index terpadu: peta dokumen per audiens, status proyek dalam 1 paragraf |
| **Non-teknis** | | |
| `business/overview.md` | update | ringkasan eksekutif: masalah, solusi, nilai, status, angka kunci |
| `business/use-cases.md` | 🆕 | kapabilitas per stage (pre/post-payment, returns, promo, account security, payout) dan per persona |
| `business/governance-compliance.md` | 🆕 | maker–checker, audit, UU PDP, POJK, batasan AI (bahasa non-teknis) |
| `business/demo-script.md` | 🆕 | skenario demo langkah demi langkah, akun demo, persiapan |
| `business/glossary.md` | 🆕 | istilah fraud, engine, ML, agent |
| **Teknis** | | |
| `technical/architecture.md`, `technical-overview.md` | update | diagram mermaid: konteks, container, alur scoring, alur LLM; selaraskan dengan kode terbaru |
| `technical/llm-agentic.md` | 🆕 | kondisi sekarang: RAG regulasi, single agent 14 tools, analysis pipeline, provider Ollama/Gemini, batas konteks 20k |
| `technical/multi-agent.md` | 🆕 | desain multi-agent final (dari [[02 Spec Arsitektur Multi-Agent]], disesuaikan hasil Tahap B) |
| `technical/ux-design.md` | 🆕 | desain UX & alur (lihat §2) |
| `technical/test-data.md` | 🆕 | katalog data uji per kasus (lihat §3) |
| `technical/*` lainnya, `guides/*` | review | perbaiki bagian yang sudah tidak sesuai kode |
| **Backlog & roadmap** | | |
| `backlog.md` | update | E0–E15 status terkini + epic baru **E16 Multi-Agent** (F1–F9) + gap G1–G8 + bug UI terbuka |
| `roadmap.md` | 🆕 | fase 0–4 (gantt), milestone **Demo Multi-Agent**, blocker, matriks prioritas |
| `CHANGELOG.md` | 🆕 | ringkasan perubahan per tanggal dari git log |

Cara kerja:
1. Baca paralel (subagent) per area: service Rust, service Python (ml/llm/ingest), web Nuxt, deploy/ops, docs & backlog. Output berupa ringkasan fakta + path file.
2. Tulis di repo (branch `docs/unified-docs`), lalu sinkron ke vault `dokumentasi/` + tautkan dari [[00 Index Pengembangan]].
3. Commit hanya setelah direview.

## 2. Desain UX & alur (intuitif)

Prinsip:
- **Satu layar, satu keputusan.** Approver melihat draft rule, sitasi, hasil simulasi, dan verdict Critic di satu halaman; approve/reject dengan satu klik + alasan.
- **Progres terlihat.** Agent run tampil sebagai timeline live (SSE): agent mana yang sedang bekerja, tool apa yang dipanggil, berapa lama.
- **Setiap klaim punya bukti.** Sitasi bisa diklik ke teks pasal; angka bisa diklik ke backtest/query asal.
- **Progressive disclosure.** Default ringkas (verdict, dampak); detail (prompt, token, tool output) di panel lipat untuk engineer/auditor.
- **AI tidak pernah mengaktifkan.** Label "AI-generated" dan status proposal selalu jelas.

Deliverable:
| # | Item | Bentuk |
|---|---|---|
| U1 | Persona & journey: analis, approver/risk manager, compliance, admin | tabel + diagram journey (mermaid) |
| U2 | Alur F1 Regulation-to-Rule dari sisi user: upload/pilih regulasi → jalankan → pantau → review → approve → shadow | flowchart + wireframe |
| U3 | Halaman **Agent Runs**: daftar run + detail timeline | wireframe ASCII/SVG |
| U4 | Halaman **Proposal Review** dengan jejak multi-agent (sitasi, simulasi, Critic) | wireframe |
| U5 | Panel **Case Copilot** (F3) di halaman case | wireframe |
| U6 | Status, error & empty state (budget habis, LLM lambat, sitasi gagal diverifikasi) | tabel |

Wireframe ditinjau dulu sebelum diimplementasi di Nuxt (Tahap B).

## 3. Data uji publik per kasus

Sudah tersedia di nvda11-gpu (`~/datasets/fraud-public/`, `~/datasets/fraud-research/`): PaySim, Sparkov, SAML-D, TabFormer, Fraud_Data, AMLworld, RBA, ULB.

| Kasus / fitur | Data uji (sudah ada) | Kandidat publik tambahan (akan diverifikasi lisensi & ketersediaan) | Catatan |
|---|---|---|---|
| Card fraud / checkout (carding, CNP) | Sparkov, TabFormer, ULB | IEEE-CIS Fraud Detection (Vesta, Kaggle) | IEEE-CIS: lisensi kompetisi, cek apakah boleh untuk demo |
| Account takeover / login | RBA | — | |
| Pembukaan akun / application fraud | — | **Bank Account Fraud (BAF)**, Feedzai NeurIPS 2022 (CC BY-NC-SA) | celah stage `account_security` |
| E-commerce sign-up → purchase | Fraud_Data | — | |
| Mobile money / payout / money mule (G2) | PaySim | — | |
| AML / money laundering, pola graph | SAML-D, AMLworld | Elliptic / Elliptic++ (bitcoin) | untuk uji graph/GNN |
| Promo abuse, returns | demo sintetis (simulator) | (data publik berlabel sangat terbatas) | tetap pakai simulator; dicatat sebagai keterbatasan |
| F1 Regulation-to-Rule, F8 chat (golden set) | POJK 12/2024, dokumen EU di `paper-fraud-detection/sources/` | — | golden set 50–100 pertanyaan dibuat dari teks pasal |
| F3 Investigation Copilot | event berlabel dari dataset di atas | — | pilih kasus yang representatif per tipologi |
| F5 Threshold tuning | semua dataset berlabel | — | angka dasar ada di [[dataset-results]] |
| F6 Red-team | simulator | — | sintetis, tenant eksperimen saja |

Output: `docs/technical/test-data.md` (dataset, sumber, lisensi, sitasi, cara load lewat `tools/research_seed`, kasus uji per fitur, metrik yang diharapkan).

## 4. Tahap B — Demo Multi-Agent (setelah Tahap A disetujui)

Scope MVP demo (subset dari [[02 Spec Arsitektur Multi-Agent]] & [[03 Spec Fitur Agentic]]):

| # | Pekerjaan | Lokasi kode (v2) |
|---|---|---|
| B1 | `agent/registry.py`: spec per agent (prompt, tools, model, budget); `loop.py` menerima `allowed_tools` | `services/python/llm-service/src/llm_service/agent/` |
| B2 | `multi_agent/state.py`, `store.py` + migrasi `llm.agent_runs`, `llm.agent_steps` (RLS) | `multi_agent/`, `db/migrations/` |
| B3 | Orchestrator deterministik + flow **F1 Regulation-to-Rule** (Regulation ∥ Analytics ∥ Pattern → Designer → Simulator → Critic ↺ → Proposer) | `multi_agent/flows/reg_to_rule.py` |
| B4 | Critic + `verify_citation` deterministik | `multi_agent/critic.py`, `agent/tools.py` |
| B5 | API `agent-runs` + SSE events + cancel | `api/agent_runs.py` (+ route gateway) |
| B6 | UI Nuxt sesuai §2: Agent Runs, Proposal Review, (Case Copilot) | `web/app/` |
| B7 | Test (Ollama di-mock) + seed data demo dari §3 + update dokumen | |
| B8 (opsional) | **F3 Investigation Copilot** sebagai fitur demo kedua | |

- Branch `feature/multi-agent` dari `revamp/rust-nuxt-stack`, di `~/own_project/fraud_detection_engine` (setelah Tahap 0); demo di nvda11-gpu (`fds.hendrikarisma.my.id`).
- **Blocker:** driver GPU (qwen3:8b di CPU ~0,5 tok/s, multi-agent tidak layak). Alternatif: provider hosted (Gemini sudah didukung; Claude bisa ditambah) untuk Designer/Critic.
- Governance tidak berubah: satu-satunya aksi tulis tetap `create_rule_proposal` → maker–checker → shadow.

## 5. Urutan kerja

1. ✅ Plan ini (repo + vault).
2. Tahap 0: konsolidasi direktori (perlu persetujuan, ada downtime singkat).
3. Tahap A: dokumentasi + backlog + roadmap (repo → vault).
4. §2 desain UX & alur → review.
5. §3 katalog data uji → verifikasi dataset tambahan (BAF, Elliptic, IEEE-CIS).
6. Tahap B: implementasi demo multi-agent.

## 6. Pertanyaan terbuka

1. Stack `fraud-platform` di p14s (23 GB, ollama) masih dipakai atau boleh dihentikan?
2. LLM untuk demo multi-agent: Ollama lokal (GPU harus beres dulu), Gemini, atau Claude API?
3. Target tanggal demo dan audiensnya (internal, klien, akademik)?
4. Bahasa dokumen: Indonesia (istilah teknis tetap Inggris), sama seperti docs sekarang?
