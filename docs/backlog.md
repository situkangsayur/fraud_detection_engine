# Product Backlog — Fraud Detection Platform v2

Format: **Epic → Story**, prioritas **P0** (wajib untuk v1), **P1** (sebaiknya ada di v1 / segera setelahnya), **P2** (roadmap).
Status: `todo` · `in-progress` · `done` · `blocked`. Update file ini setiap kali ada perubahan scope.

Terakhir diperbarui: 2026-10-21. Roadmap (fase & urutan): [`roadmap.md`](roadmap.md). Riwayat: [`CHANGELOG.md`](CHANGELOG.md).

---

## Ringkasan status v1

| Epic | Prioritas | Status |
|---|---|---|
| E0 Fondasi & desain kontrak | P0 | done |
| E1 Multi-tenant & project | P0 | done |
| E2 Pluggable data source & mapping | P0 | done |
| E3 Rule engine (5 jenis rule, formula, ruleset) | P0 | done |
| E4 Supervised ML (plugin) | P0 | done |
| E5 Unsupervised ML / anomaly & clustering (plugin) | P0 | done |
| E6 Graph engine | P0 | done |
| E7 LLM assistant (regulasi, analisis, rekomendasi) | P0 | in-progress (GPU aktif 2026-10-21; chat + 20 tools teruji live; golden set belum) |
| E8 Orkestrasi scoring & keputusan | P0 | done |
| E9 Case management & labeling | P0 | done |
| E10 UI (Nuxt) per engine | P0 | done |
| E11 Keamanan, audit, governance | P0 | done |
| E12 Deployment (docker compose) & observability | P0 | done |
| E13 Dokumentasi | P0 | done |
| E14 Kualitas & testing | P0 | done |
| E15 Roadmap pasca-v1 | P1/P2 | todo |
| E16 Multi-agent & fitur agentic (F1–F9) | P1/P2 | todo (desain selesai: `technical/multi-agent.md`) |
| E17 Operasional repo & dokumentasi terpadu | P0 | in-progress (stack dipindah + autostart systemd: done) |
| E18 AI Console: agent & konfigurasi AI (multi-model, provider, API key) | P1 | todo (PRD + TRD selesai) |
| E19 Integrasi klien: template respons, callback/webhook keluar | P1 | todo (analisis: `technical/integration-extensibility.md`) |

---

## Tahap saat ini & langkah berikutnya (per 2026-10-21)

**Tahap:** sama seperti 2026-10-13 (dokumentasi + desain, belum ada kode baru). Perubahan 2026-10-21:
- [x] Stack `fraud-platform` + `fraud-eval` + cron dipindah ke checkout deploy
      `~/own_project/deployment/fraud_detection_engine` (dulu `~/own_project/apps/fraud_detection_engine`); checkout
      dev/docs tetap `~/own_project/fraud_detection_engine`. Lihat `guides/operations-runbook.md` §9.
- [x] Autostart saat boot: systemd user unit `fraud-platform.service` (enabled, linger aktif).
- [x] PRD + TRD **AI Console** (kelola agent, model per agent, multi-model + fallback, provider Ollama/Gemini,
      API key terenkripsi): `business/prd-agent-ai-console.md`, `technical/trd-agent-ai-console.md` → epic E18.
- [ ] **Disk nvda11-gpu 100%** (sisa ±6 GB) → bersihkan sebelum pengembangan apa pun (OpenSearch blok di 95%).
- [x] Push `eval/public-datasets` ke origin (2026-10-21).
- [x] Driver GPU diperbaiki (DKMS untuk kernel 7.0.0-38): qwen3:8b 100% GPU, ≈ 62 tok/s (sebelumnya ≈ 0,5 tok/s di CPU,
      yang juga membuat seluruh server lambat saat chat berjalan). Ollama 0.6.8 → 0.40.7.
- [x] Chat LLM: riwayat tersimpan walau jawaban lambat/gagal/tab ditutup; jawaban ter-stream live (route gateway tanpa
      buffering + Ollama baru) dengan status langkah & detik; 6 tool baru untuk cek pelanggan, transaksi, kasus
      (email/telepon disamarkan).
- [x] Graph: legend jelas (warna sama dengan node, jumlah per jenis), canvas lebih besar + layar penuh, cari
      nama/ID/email/telepon/IP/device, daftar pelanggan fraud untuk memulai.
- [x] UI: menu akun di kanan atas setiap halaman (profil, tema Sistem/Terang/Gelap, bahasa, keluar), halaman
      **Profil saya**, tema terang sebagai default.
- [x] Docs: panduan pengguna dengan screenshot (tema terang) dan story per fitur, katalog dataset bank
      (`technical/bank-datasets.md`), integrasi & ekstensibilitas (`technical/integration-extensibility.md`).

Langkah berikutnya (urut): bersihkan disk → driver GPU → uji LLM live (baseline) → **E18 R1** (AI Console dasar) →
E18 R2 (agent profile + multi-model) → E16 MVP multi-agent di atas konfigurasi E18.

## Tahap 2026-10-13

**Tahap:** demo publik `fds.hendrikarisma.my.id` berjalan di nvda11-gpu dari commit `07ee765`. Fokus sekarang:
**dokumentasi terpadu + desain multi-agent** (belum ada pengembangan kode baru).

Status per 2026-10-13:
- [x] Checkout kanonik di nvda11-gpu: `~/own_project/fraud_detection_engine` = v2 `07ee765` (sama dengan yang
      ter-deploy). Sisa v1 dipindah ke `~/own_project/_archive/`.
- [x] ~~Stack `fraud-platform` + `fraud-eval` dan cron reset masih memakai `~/own_project/apps/fraud_detection_engine`~~
      (selesai 2026-10-21, ke `~/own_project/deployment/fraud_detection_engine`) → pindahkan sesuai `guides/operations-runbook.md` (butuh downtime ±5–10 menit, dijalankan oleh pemilik).
- [ ] Hapus clone tidak terpakai di p14s (`~/own_project/fraud_detection_engine`, `~/own_project/apps/fraud_detection_engine`
      + stack-nya); vault Obsidian di p14s tetap.
- [ ] Push branch `eval/public-datasets` (2 commit hasil evaluasi 5 dataset publik) ke origin.
- [x] ~~Driver GPU nvda11-gpu masih belum jalan~~ → diperbaiki 2026-10-21.
- [ ] Gemini: akun pemilik bermasalah → demo memakai Ollama lokal dulu.
- [ ] **P0 keamanan demo publik:** `DEMO_LOGIN_HINTS` di `.env` ikut menampilkan akun **platform admin** di halaman
      login publik (dan hint memakai password yang sama) → hapus hint platform admin, ganti password, pertimbangkan
      peran viewer untuk akun demo.
- [ ] **P1 keamanan:** chat LLM hanya butuh peran `viewer`, sedangkan tool `create_rule_proposal` memanggil
      rule-service dengan token internal → viewer bisa membuat proposal pending lewat chat. Batasi tool tulis ke peran
      ≥ analyst (lihat `technical/llm-agentic.md` §8).
- [ ] Disk nvda11-gpu ±94% (OpenSearch memblokir index pada 95%; snapshot master ±600 MB) → bersihkan/perluas.
- [x] ~~Ollama host versi 0.6.8~~ → 0.40.7 (2026-10-21).

Langkah berikutnya (urut):
1. Selesaikan dokumentasi terpadu (E17) dan review desain multi-agent + UX + data uji (E16).
2. Perbaiki driver GPU (blocker utama demo multi-agent).
3. Uji LLM live tuntas (chat + `recommend-rules` → proposal) sebagai baseline single-agent.
4. Quick wins deteksi: threshold per project (E3 P1), rule money mule (E14 P1).
5. Implementasi MVP demo multi-agent (E16: F1 + Critic/`verify_citation` + UI Agent Runs).

## Tahap sebelumnya (per 2026-10-07)

**Tahap:** v1 berjalan sebagai demo publik di nvda11-gpu, CI hijau, PR #1 (`revamp/rust-nuxt-stack` → `master`) terbuka.

Selesai pada 2026-10-07:
- [x] Pindah host pengembangan ke nvda11-gpu; Ollama lokal jadi opsional (compose profile `local-llm`).
- [x] Web UI selaras dengan backend (lint/typecheck/test/build hijau, crawl tanpa error API).
- [x] CI hijau: lint web, smoke test migrasi yang flaky, ekspektasi skor ruleset shadow, port acak integration test.
- [x] Bug dashboard: filter tanggal kini menerima `YYYY-MM-DD` maupun RFC 3339 (sebelumnya 400 "premature end of input").
- [x] Login gagal "premature close" karena cookie host > 16 KB → batas header web 64 KB.
- [x] Link project usang (setelah reset / sesi kedaluwarsa) diarahkan ke daftar project.
- [x] Halaman login: info akun demo (`DEMO_LOGIN_HINTS`) dan tombol bahasa ID/EN.
- [x] llm-service: strip `<think>`, `OLLAMA_THINK`, validasi argumen integer tool (bug `k:0`), minimal 4 chunk regulasi.
- [x] llm-service: provider chat **Gemini** opsional (`LLM_PROVIDER=gemini`), embedding tetap Ollama.
- [x] Simulator & post-seed login ulang saat token kedaluwarsa (seed > 1 jam).
- [x] Reset demo harian 00.00 (`deploy/demo/reset-demo.sh` + `post_seed.py`): data 60 hari s/d hari ini, regulasi
      POJK 12/2024, model `mlp_backprop` + `isolation_forest`/`hdbscan` aktif di semua project.
- [x] Dokumentasi: `technical-overview.md` (stack, arsitektur software & AI, metode), ringkasan non-teknis diperluas.

Langkah berikutnya:
1. **Uji LLM sampai tuntas**: chat dengan minimal 4 chunk, skenario `recommend-rules` → proposal pending. Bila memakai
   Gemini, perlu API key yang valid (key 2026-10-07 ditolak: 403).
2. **Perbaiki driver GPU nvda11-gpu** (DKMS nvidia 595 belum ter-build untuk kernel 7.0) agar LLM lokal cepat.
3. Review & merge PR #1.
4. P1: rekomendasi threshold per project, rule money mule, hydration mismatch UI (kosmetik, 5 halaman).
   Dataset riset 2026-10-07 menguatkan: FPR pada threshold 50 = 23% (checkout) dan 36% (promo); kalibrasi
   FPR ≤ 5% tetap recall ~100%. Money mule recall@80 hanya 0,35.
6. P2 UI: banner "dipaksa oleh … (force_review)" tampil pada keputusan *Tolak* (skor ≥ 80 menang atas force_review —
   pesan menyesatkan); catatan case menampilkan UUID user, bukan nama.
5. P1 (keamanan demo publik): rate limit login, nonaktifkan aksi destruktif bagi akun demo atau gunakan peran viewer.

---

## E0 — Fondasi & desain kontrak (done)
- [x] P0 Migrasi kode lama ke branch `legacy/python-fastapi-mongo` (sudah di-push) dan folder `legacy/`.
- [x] P0 Arsitektur target: Rust (core-api, rule-service, graph-service), Python (ml, llm, ingest), Postgres, OpenSearch, Ollama, Nuxt, Traefik.
- [x] P0 Kontrak: `architecture.md`, `multi-tenancy.md`, `rule-dsl.md`, `data-sources.md`, `feature-catalog.md`, `ml-plugins.md`, `api-contract.md`.
- [x] P0 Skema database per service + RLS + least-privilege role, tervalidasi di Postgres 16 (`db/tests/test_migrations.sh`).
- [x] P0 docker-compose + gateway routing + `.env.example`.

## E1 — Multi-tenant & project
- [x] P0 CRUD tenant (platform admin), user tenant, peran tenant_admin/member.
- [x] P0 CRUD project dengan `stage` (pre_payment, post_payment, returns, promo, account_security, payout, custom), business context, timezone, currency.
- [x] P0 Member project + peran (project_admin/approver/analyst/viewer), klaim JWT `prj`.
- [x] P0 Settings per project (threshold keputusan, bobot engine, skor graph, timeout).
- [x] P0 Template stage → bootstrap ruleset/reference list default.
- [x] P0 Isolasi tenant via RLS di semua service (set `app.tenant_id` per transaksi).
- [ ] P1 Arsip & hapus project (soft delete + retensi data).
- [ ] P2 "Linked projects": propagasi label fraud lintas project dalam satu tenant (opt-in).
- [ ] P2 Kuota & rate limit per tenant.

## E2 — Pluggable data source & mapping
- [x] P0 Jenis source: webhook (API key), file (CSV/TSV/JSON/JSONL/Parquet/XLSX), Postgres/MySQL.
- [x] P0 Inferensi skema (tipe, format tanggal, PII) + saran mapping bilingual (EN/ID).
- [x] P0 Editor mapping + preview + versi mapping + aktivasi.
- [x] P0 Semua field sumber tersedia di rule sebagai `source.*` (field catalog dinamis).
- [x] P0 Transformasi: parse tanggal, angka lokal ID, hashing PAN/rekening, normalisasi phone/email, concat, value_map, dll.
- [x] P0 Import job (mode `score` / `load_only`) + dead-letter error.
- [x] P0 Label dari dataset (kolom `is_fraud`, dsb.) untuk training.
- [ ] P1 Pull connector terjadwal (cursor-based) untuk Postgres/MySQL.
- [ ] P1 Saran mapping dibantu LLM untuk field dengan confidence rendah.
- [ ] P2 Konektor Kafka / Redpanda (streaming) dan S3/GCS.
- [ ] P2 Expression index otomatis untuk field `source.*` yang sering dipakai velocity.

## E3 — Rule engine
- [x] P0 Rule **simple**: kondisi bertingkat (all/any/not/at_least), bandingkan dengan konstanta **atau field lain**, mode skor binary/weighted.
- [x] P0 Rule **velocity**: agregasi (count/sum/avg/min/max/distinct/stddev/median/percentile) + group by + window (durasi / N event terakhir), bandingkan dengan nilai/field/formula.
- [x] P0 Velocity **statistik**: z-score, gaussian tail, percentile rank, linear trend (slope/forecast/residual), poisson tail.
- [x] P0 Rule **composite**: velocity dengan filter histori dari kondisi simple + gate event saat ini.
- [x] P0 Rule **reference**: whitelist/blacklist/watchlist/lookup (per project atau tenant-wide), mode exists/not_exists/attribute.
- [x] P0 Rule **graph**: jarak ke fraudster, jumlah tetangga fraud, shared entity, ukuran komponen, fraud rate komunitas.
- [x] P0 Operand **formula** `F(x,y,z) = 2x + 2^y / z^2` (implicit multiplication, fungsi matematika & statistik).
- [x] P0 Hasil tiga nilai: match / no_match / **trapped** (+ `on_trapped`: ignore/score/review).
- [x] P0 Ruleset: agregasi sum/max/probabilistic_or/weighted_average, bobot per rule, action force_*.
- [x] P0 Versioning immutable, maker–checker, shadow mode.
- [x] P0 Validasi, test terhadap event, backtest (precision/recall terhadap label).
- [ ] P1 Statistik rule harian (hit rate, trapped rate, precision) + alert rule "mati" / terlalu berisik.
- [ ] P1 Simulasi dampak perubahan threshold ruleset (what-if) sebelum aktivasi.
- [ ] P1 **Rekomendasi threshold keputusan per project dari data berlabel** (target FPR / biaya). Evaluasi
  end-to-end menunjukkan threshold default 50 memberi FPR 32–35% di post-payment/returns vs 5–7% di checkout/promo
  (lihat `docs/technical/evaluation.md`).
- [ ] P2 Rule scheduling (aktif pada jam/tanggal tertentu, mis. saat flash sale).
- [ ] P2 Import/export rule antar project (JSON bundle bertanda tangan).

## E4 — Supervised ML
- [x] P0 Registry algoritma berbasis **plugin** (hot reload, validasi + smoke test).
- [x] P0 Built-in: `mlp_backprop` (PyTorch, backpropagation), logistic regression, gradient boosting, random forest.
- [x] P0 Training per project, split berbasis waktu, penanganan imbalance, metrik (ROC-AUC, PR-AUC, confusion, kalibrasi).
- [x] P0 Model registry + maker–checker aktivasi + hot swap.
- [x] P0 Penjelasan per prediksi (top features).
- [ ] P1 Retraining terjadwal + perbandingan champion/challenger.
- [ ] P1 Monitoring drift skor model & alert.
- [ ] P2 Upload plugin bertanda tangan via UI (platform admin).
- [ ] P2 Export model ke ONNX untuk inference langsung di Rust (latensi lebih rendah).

## E5 — Unsupervised ML
- [x] P0 Plugin anomaly: isolation forest, LOF, autoencoder.
- [x] P0 Plugin clustering: HDBSCAN, k-means, DBSCAN, gaussian mixture.
- [x] P0 Profil cluster, fraud rate per cluster, fitur pembeda, label cluster oleh analis.
- [x] P0 Proyeksi 2-D (PCA) untuk visualisasi.
- [x] P0 Komunitas graph (Louvain) + fraud rate komunitas → dipakai rule graph.
- [ ] P1 Rekomendasi rule dari cluster anomali dengan coverage rule rendah (via LLM).
- [ ] P2 Embedding graph (node2vec) sebagai fitur ML.

## E6 — Graph engine
- [x] P0 Entity resolution: email, phone, device, IP, kartu, rekening, alamat, ref transaksi, api client.
- [x] P0 Link kemiripan: phone (beda 1 digit / suffix sama), alamat (trigram), email (local-part).
- [x] P0 Proteksi supernode (mis. IP publik) via degree cap.
- [x] P0 Metrik graph untuk scoring & rule; neighbourhood, fraud proximity (shortest path), komponen.
- [ ] P1 Cache metrik graph + invalidasi saat label berubah.
- [ ] P2 Graph temporal (bobot link berdasarkan waktu), time-travel view.

## E7 — LLM assistant
- [x] P0 Implementasi + unit/integration test (Postgres + OpenSearch asli, Ollama di-mock).
- [ ] P0 Uji live dengan Ollama sungguhan di `nvda11-gpu` (qwen) — langkah berikutnya #2.
- [ ] P0 Library regulasi/kebijakan per tenant (OJK, BI, SOP internal) + lampiran per project.
- [ ] P0 Chunking sadar struktur (BAB/Pasal/ayat), embedding (bge-m3), hybrid search (BM25 + kNN).
- [ ] P0 Deteksi perubahan regulasi (diff per pasal) + ringkasan perubahan.
- [ ] P0 Analisis: relevansi rule, kondisi fraud terkini, dampak regulasi, rekomendasi rule.
- [ ] P0 Rekomendasi rule → validasi → backtest → **proposal** (tidak pernah aktif otomatis).
- [ ] P0 Chat dengan tools (read-only + create proposal), sitasi pasal.
- [ ] P1 Evaluasi kualitas LLM (golden set pertanyaan regulasi, akurasi rekomendasi).
- [ ] P2 Pilihan provider LLM lain (Claude/OpenAI) per tenant dengan kebijakan data.

## E8 — Orkestrasi scoring & keputusan
- [x] P0 Pipeline: mapping → customer → event → graph links → fitur → graph → ML → rules → keputusan.
- [x] P0 Kombinasi skor `noisy_or` (default) / `weighted_average` per project.
- [x] P0 Degraded mode (engine gagal/timeout tidak menggagalkan keputusan) + rescore.
- [x] P0 Kombinasi skor berbobot + override action + reason codes.
- [x] P0 Simulasi (dry-run) tanpa persist.
- [ ] P1 Callback/webhook keputusan ke sistem klien.
- [ ] P1 Idempotency key untuk ingest.
- [ ] P2 Partitioning tabel `core.events` per bulan + retensi.

## E9 — Case management & labeling
- [x] P0 Case otomatis untuk review/decline, dedup per customer, assign, catatan, resolve.
- [x] P0 Label event/customer (analyst, chargeback, dataset) → feedback ke ML & graph.
- [ ] P1 SLA case, antrian prioritas, notifikasi (email/Slack/webhook).
- [ ] P1 Import chargeback massal.

## E10 — UI (Nuxt)
- [x] P0 Semua halaman dibangun; lint/typecheck/53 test/build hijau; smoke test dengan mock API; login + 22 endpoint lewat BFF terverifikasi ke backend asli.
- [ ] P0 Penyelarasan shape response dengan backend asli + halaman About/atribusi AGPL — langkah berikutnya #1.
- [ ] P0 Login, pemilihan tenant/project, dashboard.
- [ ] P0 Halaman per engine: Rule, ML Supervised, ML Unsupervised, Graph, LLM.
- [ ] P0 Data source & mapping wizard, field catalog.
- [ ] P0 Events, cases, audit, settings, members, tenant admin.
- [ ] P1 E2E test Playwright untuk alur utama.
- [ ] P2 Dashboard kustom per tenant.

## E11 — Keamanan, audit, governance
- [x] P0 JWT + refresh token rotation, BFF cookie httpOnly, RBAC per project.
- [x] P0 Maker–checker untuk rule, ruleset, model, mapping, proposal.
- [x] P0 Audit log append-only.
- [x] P0 PII: hashing kartu/rekening dengan pepper per tenant, masking.
- [ ] P1 mTLS antar service, secrets manager (Vault/SOPS).
- [ ] P1 SSO (OIDC/SAML) untuk tenant enterprise.
- [ ] P1 Kepatuhan UU PDP: retensi, hak hapus data, laporan akses data.
- [ ] P2 Penetration test & threat model formal.

## E12 — Deployment & observability
- [x] P0 docker compose full stack, healthcheck, migrate job, gateway.
- [x] P0 Logs JSON, metrics Prometheus, request-id end-to-end.
- [ ] P1 Stack observability opsional (Prometheus + Grafana + Loki) via compose profile.
- [ ] P1 OpenSearch security plugin + TLS untuk produksi.
- [ ] P1 Backup/restore Postgres & OpenSearch.
- [ ] P2 Helm chart / Kubernetes, autoscaling rule-service & graph-service.

## E13 — Dokumentasi
- [x] P0 Dokumen teknis (kontrak).
- [x] P0 Dokumen non-teknis (bisnis).
- [x] P0 Panduan: quickstart, panduan pengguna per engine, panduan developer, panduan deployment.
- [x] P0 Penjelasan struktur Rust & prinsip software engineering (untuk developer dari latar Java).
- [x] P0 Backlog (dokumen ini).

## E14 — Kualitas & testing
- [x] P0 Evaluasi deteksi end-to-end out-of-sample dengan simulator + ground truth (`evaluation.md`).
- [ ] P1 Rule khusus money mule (fan-in/fan-out pada `recipient_fingerprint`) — tipologi terlemah (recall 78%).
- [x] P0 Unit test rule-engine (parser formula, semua jenis rule, logika Kleene).
- [x] P0 Integration test Rust dengan Postgres asli.
- [x] P0 pytest untuk service Python; vitest untuk web.
- [x] P0 CI GitHub Actions (lint, test, build image).
- [ ] P1 Load test (k6) target p95 < 150 ms.
- [ ] P1 Contract test antar service (OpenAPI diff di CI).

## E15 — Roadmap pasca-v1 (ringkas)
- P1 Device fingerprinting SDK (web/mobile) untuk sinyal ATO yang lebih kuat.
- P1 Enrichment eksternal: IP geolocation/ASN, BIN database, email/phone reputation.
- P2 Federated/consortium blacklist antar tenant (opt-in, hashed).
- P2 Auto-tuning threshold berbasis biaya (cost-sensitive: nilai fraud vs friksi customer).
- P2 Lihat juga peningkatan engine deteksi pada bagian "Gap terukur" di bawah (kalibrasi skor, stacking fusion,
  graph temporal/label propagation, GNN, sequence model ATO, active learning, delayed labels, fairness check).

## E16 — Multi-agent & fitur agentic
Desain: [`technical/multi-agent.md`](technical/multi-agent.md) · UX: [`technical/ux-design.md`](technical/ux-design.md) ·
data uji: [`technical/test-data.md`](technical/test-data.md). Kondisi sekarang: **satu** tool-calling agent (20 tools) +
pipeline analisis terstruktur (lihat [`technical/llm-agentic.md`](technical/llm-agentic.md)). Governance tidak berubah:
satu-satunya aksi tulis tetap `create_rule_proposal` → maker–checker → shadow.

Fondasi (MVP demo):
- [ ] P1 Registry agent (`agent/registry.py`): prompt, tools (least privilege), model, budget per agent; `run_agent` menerima `allowed_tools`.
- [ ] P1 Run state + store: migrasi `llm.agent_runs`, `llm.agent_steps` (RLS), resume & replay.
- [ ] P1 Orchestrator deterministik (state machine Python, tanpa framework) + budget langkah/token/waktu/revisi.
- [ ] P1 Tool baru `verify_citation` (deterministik: pasal ada, kutipan cocok dengan chunk) + `get_regulation_article`.
- [ ] P1 API `agent-runs` (start, get, list, cancel) + SSE event stream; route di gateway.
- [ ] P1 UI Nuxt: halaman Agent Runs (timeline live) + jejak multi-agent di Proposal Review.
- [ ] P1 Metrik Prometheus agent (`agent_steps_total`, `agent_run_duration_seconds`, `critic_verdict_total`, …).
- [ ] P1 Test: Ollama di-mock (unit) + Postgres/OpenSearch asli (integration); skenario demo dari `test-data.md`.

Fitur:
- [ ] P1 **F1 Regulation-to-Rule**: Regulation ∥ Analytics ∥ Pattern → Designer → Simulator → Critic (↺ maks 3) → Proposer.
- [ ] P1 **F3 Case Investigation Copilot**: narasi investigasi per case (AI-generated, PII di-mask, tidak memberi label).
- [ ] P1 **F5 Threshold & Ruleset Tuning Agent**: sweep deterministik + penjelasan trade-off; proposal jenis `settings_change`.
- [ ] P1 **F2 Rule Health Review** terjadwal (rule mati/berisik/trapped/redundant) + laporan mingguan; flag rule `mandatory`.
- [ ] P2 **F4 Pattern-to-Rule** dari cluster anomali & komunitas graph (fokus money mule).
- [ ] P2 **F7 Regulation Change Watcher** (diff per pasal → rule terdampak); butuh relasi `rule ↔ citation` (traceability).
- [ ] P2 **F8 Multi-agent chat** (router ke agent spesialis; pertanyaan berat → agent run).
- [ ] P2 **F9 Compliance Gap Report** (matriks pasal × rule aktif).
- [ ] P3 **F6 Red-team vs Blue-team** (riset; event sintetis di tenant eksperimen saja).

Evaluasi & riset:
- [ ] P1 Golden set regulasi POJK 12/2024 (50–100 Q&A + pasal acuan) + 30 skenario rule; dijalankan di CI (mock) dan terjadwal (live).
- [ ] P2 Eksperimen ablation B0–M3 (≥ 5 seed) → paper kedua.

## E17 — Operasional repo & dokumentasi terpadu
- [x] P0 Checkout kanonik `~/own_project/fraud_detection_engine` di nvda11-gpu = versi ter-deploy (`07ee765`).
- [x] P0 Pindahkan stack `fraud-platform`/`fraud-eval` + cron ke checkout deploy terpisah `~/own_project/deployment/fraud_detection_engine` (2026-10-21; runbook §9).
- [x] P1 Autostart stack saat boot: systemd user unit `fraud-platform.service` (2026-10-21).
- [ ] P0 Bersihkan disk nvda11-gpu (100%, 2026-10-21).
- [ ] P0 Hapus clone tidak terpakai di p14s.
- [x] P0 Push `eval/public-datasets` (2026-10-21).
- [ ] P1 Putuskan merge `eval/public-datasets` ke `revamp/rust-nuxt-stack`.
- [x] P0 Dokumentasi terpadu: index `docs/README.md`, dokumen bisnis baru (use cases, governance, glosarium, script demo),
      dokumen teknis diverifikasi terhadap kode, `roadmap.md`, `CHANGELOG.md`, runbook operasional.
- [x] P1 Sinkron `docs/` → vault Obsidian (p14s) via `docs/sync-to-vault.sh` (manual setelah perubahan docs).
- [x] P1 PRD & TRD AI Console (2026-10-21).
- [ ] P1 Review & merge PR #1 (`revamp/rust-nuxt-stack` → `master`).

## E18 — AI Console: agent & konfigurasi AI
PRD: [`business/prd-agent-ai-console.md`](business/prd-agent-ai-console.md) · TRD:
[`technical/trd-agent-ai-console.md`](technical/trd-agent-ai-console.md). Hari ini provider/model global lewat `.env`;
`llm_config.chat_model` per project berupa teks bebas (default `qwen2.5:7b-instruct` tidak terpasang).

R1 — AI Console dasar:
- [ ] P1 Migrasi `llm.ai_providers`, `ai_credentials`, `ai_models`, `tenant_ai_policy` (+ RLS, baris platform `tenant_id NULL`).
- [ ] P1 Enkripsi API key AES-256-GCM (`LLM_SECRETS_KEY`), write-only, `last4`, rotasi, redaksi log.
- [ ] P1 `ChatClient` protocol + `OpenAICompatClient` (Gemini jadi subclass) + `ChatClientRegistry` per provider; fallback ke env bila tabel kosong.
- [ ] P1 Test connection + discovery model (Ollama `/api/tags`, OpenAI `/models`).
- [ ] P1 Resolver project → tenant → platform → env + cache TTL + `LISTEN/NOTIFY` (berlaku tanpa restart).
- [ ] P1 API §6 TRD + audit `core.audit_log` (key di-redact).
- [ ] P1 UI `/admin/ai` (platform) dan `/tenant/ai` (policy provider eksternal + key tenant).
- [ ] P1 Ganti field teks `chat_model` di `projects/new.vue` dengan pemilih model; migrasi `llm_config.chat_model`.
- [ ] P1 Seed provider platform "nvda11-gpu ollama" (`host.docker.internal:11434`) + `qwen3:8b`.

R2 — Agent profile & multi-model:
- [ ] P1 `llm.agent_profiles` (per scope × agent): model utama, fallback berurutan, params, tools (subset), kill switch.
- [ ] P1 `run_agent(agent_name)` memakai config efektif; fallback otomatis + `fallback_from`.
- [ ] P1 Pencatatan pemakaian `llm.llm_calls` (token, latensi, outcome) + metrik Prometheus; semaphore GPU per provider lokal.
- [ ] P1 UI project: bagian "AI & agents" di settings, label "Efektif: … (dari …)"; badge model di chat/laporan.
- [ ] P2 Halaman Usage + batas token bulanan per tenant.
- [ ] P2 Prompt tambahan & daftar tool agent lewat maker–checker (versi pending → approve).
- [ ] P2 Provider tambahan OpenAI-compatible (Claude/vLLM), tabel biaya.

R3 = E16 MVP multi-agent memakai konfigurasi R2.

## E19 — Integrasi klien (respons & callback)
Analisis: [`technical/integration-extensibility.md`](technical/integration-extensibility.md). Struktur data & request
klien sudah fleksibel (mapping), tetapi respons selalu `DecisionOut`.
- [ ] P1 Template respons per data source (`DecisionOut` → JSON klien, mesin mapping dibalik, versioned + maker–checker).
- [ ] P1 Webhook keluar / callback (HMAC, retry, dead-letter) saat keputusan final, case resolved, label berubah.
- [ ] P2 Kebijakan status code per source (mis. selalu 200).

## Lain-lain (temuan 2026-10-21)
- [ ] P1 Ganti password mandiri di halaman Profil, dan tombol **reset password** untuk pengguna yang sudah ada di
      halaman Pengguna tenant (API `PATCH /tenants/{tid}/users/{uid}` sudah menerima `password`, UI belum).
- [x] Detail kasus error 500 bila keputusan tanpa metrik graph (`DecisionOut.graph` null) → diperbaiki 2026-10-21.
- [ ] P2 Dashboard: label donut "Fraud per tipologi" tumpang tindih dengan legend pada lebar 1440 px.
- [ ] **P1 Dataset bank untuk simulasi & demo** (dipilih pemilik 2026-10-21; detail & lisensi: `technical/bank-datasets.md`).
      Per dataset: loader di `tools/research_seed` + mapping + tenant eksperimen + ground truth sebelum seeding +
      masuk master/restore malam + skenario kasus di `demo-script.md`. Sampel kecil (disk nvda11-gpu ±6 GB).
  - [ ] MoMTSim v1 (CC BY 4.0, e-wallet): sampel ±60 rb tx, buang kolom saldo (bocor label); kasus C7 SIM-swap ATO / refund fraud.
  - [ ] AMLSim (Apache-2.0, data dibangkitkan sendiri): ±1–5 rb akun dengan pola fan-in/fan-out/cycle/scatter-gather; kasus C1 jaringan mule (graph + LLM).
  - [ ] BankSim (CC BY-NC-SA, non-komersial): sampel ±60 rb tx; kasus C8 burst kartu curian, graph pelanggan–merchant.
  - [ ] SynthAML (CC BY 4.0): sampel alert (tanpa id klien → tanpa graph); kasus C3 triase alert ML + narasi LLM; split per kuartal.

## Gap terukur (dasar prioritas)

Sumber angka: [`technical/evaluation.md`](technical/evaluation.md) (dataset riset 2026-10-07 + 5 dataset publik).

| ID | Gap | Angka | Ditangani oleh |
|---|---|---|---|
| G1 | Threshold 50 seragam terlalu rendah | FPR checkout 23%, promo 36%, PaySim 46% | E3 P1 threshold per project, F5 |
| G2 | Money mule | recall@80 = 0,35 | E14 P1 rule fan-in/fan-out, F4 |
| G3 | Graph tanpa fraudster terkonfirmasi | AUC graph 0,5 (Sparkov/PaySim/SAML-D); coverage post-payment 0,2% | label customer di tenant riset, label propagation, E6 |
| G4 | Template rule tidak general | AUC rules SAML-D 0,5, PaySim 0,585 | F1, F2 (rule per dataset/project) |
| G5 | Cash withdrawal (SAML-D) | recall 0,06 | fitur lokasi ATM/agen atau diterima sebagai keterbatasan |
| G6 | Akun tanpa histori (PaySim) | velocity tidak bekerja | fitur cold-start, peer-group baseline |
| G7 | LLM berhalusinasi | pasal karangan (kecepatan sudah beres: GPU ≈ 62 tok/s sejak 2026-10-21) | Critic + `verify_citation` |
| G8 | Kasus returns sedikit | 7 fraud online | replikasi seed, interval kepercayaan |
