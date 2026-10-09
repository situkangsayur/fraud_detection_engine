# Product Backlog — Fraud Detection Platform v2

Format: **Epic → Story**, prioritas **P0** (wajib untuk v1), **P1** (sebaiknya ada di v1 / segera setelahnya), **P2** (roadmap).
Status: `todo` · `in-progress` · `done` · `blocked`. Update file ini setiap kali ada perubahan scope.

Terakhir diperbarui: 2026-10-07.

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
| E7 LLM assistant (regulasi, analisis, rekomendasi) | P0 | in-progress (uji live: retrieval OK, kualitas jawaban qwen3:8b di CPU kurang) |
| E8 Orkestrasi scoring & keputusan | P0 | done |
| E9 Case management & labeling | P0 | done |
| E10 UI (Nuxt) per engine | P0 | done |
| E11 Keamanan, audit, governance | P0 | done |
| E12 Deployment (docker compose) & observability | P0 | done |
| E13 Dokumentasi | P0 | done |
| E14 Kualitas & testing | P0 | done |
| E15 Roadmap pasca-v1 | P1/P2 | todo |

---

## Tahap saat ini & langkah berikutnya (per 2026-10-07)

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
