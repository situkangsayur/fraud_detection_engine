# Changelog — Fraud Detection Platform v2

Ringkasan perubahan per tanggal, disusun dari git log branch `revamp/rust-nuxt-stack`. Detail ada di commit masing-masing.

## 2026-10-21 (siang) — GPU, chat live, graph, akun & tema, docs bergambar
- **GPU & Ollama:** driver NVIDIA dibangun ulang untuk kernel 7.0.0-38 (qwen3:8b 100% GPU, ≈ 62 tok/s, sebelumnya
  ≈ 0,5 tok/s di CPU); Ollama 0.6.8 → 0.40.7; pemanasan model di `fraud-platform.service`.
- **Chat LLM** (`779b19c`, `371f712`): riwayat tersimpan walau jawaban lambat/gagal/tab ditutup; 6 tool baru
  (`search_customers`, `get_customer`, `search_events`, `get_event`, `search_cases`, `get_case`) dengan masking
  email/telepon; jawaban ter-stream live (route gateway tanpa buffering) + status langkah dan detik di UI.
- **Graph** (`d14e3c2`, `371f712`): legend jelas, canvas lebih besar + layar penuh, pencarian nama/ID/email/telepon/IP/
  device, daftar pelanggan fraud untuk memulai, skor kemiripan 2 desimal.
- **Web** (`7e3d4f4`, tema terang default): menu akun di kanan atas setiap halaman, halaman **Profil saya**, pilihan
  tema Sistem/Terang/Gelap.
- **Docs:** panduan pengguna dengan 33 screenshot (tema terang) dan story per fitur; `technical/bank-datasets.md`
  (17 dataset bank baru + 9 kasus, lisensi & referensi); `technical/integration-extensibility.md` (plugin, struktur
  data/request/response klien); backlog E19; runbook GPU/Ollama.

## 2026-10-21 — Checkout deploy terpisah, autostart, PRD/TRD AI Console
- Operasional: stack `fraud-platform` + `fraud-eval`, cron restore dan worktree eval dipindah dari
  `~/own_project/apps/fraud_detection_engine` ke `~/own_project/deployment/fraud_detection_engine`; checkout dev/docs
  tetap `~/own_project/fraud_detection_engine`. Volume data tidak berubah.
- Autostart: systemd user unit `fraud-platform.service` (enabled, linger aktif).
- Docs: `business/prd-agent-ai-console.md`, `technical/trd-agent-ai-console.md`; backlog epic E18; roadmap M2b; runbook §9
  ditulis ulang (layout direktori & autostart).

## 2026-10-13 — Dokumentasi terpadu (branch `docs/unified-docs`)
- Index dokumentasi `docs/README.md`; dokumen bisnis baru (use cases, governance & kepatuhan, glosarium, script demo).
- Dokumen teknis diverifikasi terhadap kode; dokumen baru: `llm-agentic.md`, `multi-agent.md`, `ux-design.md`, `test-data.md`.
- `roadmap.md`, `CHANGELOG.md`, `guides/operations-runbook.md`; backlog: epic E16 (multi-agent) dan E17 (operasional & dokumentasi), gap G1–G8.
- Operasional: checkout kanonik `~/own_project/fraud_detection_engine` di nvda11-gpu = `07ee765`.

## 2026-10-10
- Docs: konteks 20k membuat qwen3:8b muat penuh di GPU 8 GB (`07ee765`).

## 2026-10-09 — LLM pada konteks terbatas
- Analisis LLM disesuaikan dengan context window, panjang generasi dibatasi, JSON schema dibuka (`544c633`).
- Field teks bebas di output schema dibatasi; panduan Ollama di GPU 8 GB (`62ccb8a`).

## 2026-10-08 — Restore master
- SQL time-shift untuk restore master dipisah ke file sendiri (`1c1b5dc`).
- Docs: eksperimen bertahan setelah restore malam hanya bila disimpan sebagai master (`e365cb1`).

## 2026-10-07 — Demo publik, seeding riset, stabilisasi
- **CI hijau**: lint web, exit code test migrasi, ekspektasi skor ruleset shadow (`5b1407e`, `83c79a0`).
- **LLM**: profile Ollama lokal opsional + dukungan thinking model (`86a6815`); validasi argumen integer dari model
  (`bedec77`); provider chat Gemini opsional (`91e960c`); timeout Ollama bisa diatur untuk host CPU (`366db83`);
  laporan LLM berurutan dan qwen3 `/no_think` (`9ad7326`).
- **Web**: header request 64 KB (`94050a5`); hint akun demo, tombol ID/EN, pemulihan link project usang (`786ed0a`);
  cek origin CSRF di BFF membandingkan host (`6360886`).
- **API**: filter rentang menerima tanggal polos; dashboard mengirim RFC 3339 (`d4aa265`); pencarian event berdasarkan
  external id memakai index; training unsupervised atas seluruh histori (`e3f1f10`).
- **Gateway**: allow-list LAN 192.168.11.0/24 (`ca9a9cf`).
- **Demo**: reset harian (`7d7525c`); seed bertahap, aktivitas analis, ekspor dataset riset harian (`84d61c1`);
  seeding multi-tenant mode riset, snapshot/restore master, hint login per tenant (`17eba43`).
- **Simulator / seeding**: login ulang saat token kedaluwarsa (`5b330e3`); batch ingest kecil (`a230348`); retry error
  gateway sementara (`6499aff`); polling training model melewati 5xx (`ba64b1c`).
- **research_seed**: tipologi platform + label dataset sebagai ground truth, skrip wipe-tenant (`11ca3a2`);
  ground truth ditulis sebelum seeding, `--truth-only` (`4e39c79`); error aktivitas tidak melewatkan ekspor (`3c9e92b`).
- **graph-service**: stream body ekspor di-fuse (`dc1c451`).
- **Docs**: technical overview, tambahan non-teknis, panduan & backlog (`2bfbc83`); dataset riset & reset bertahap
  (`a1a5731`); panduan eksperimen (`6fe81a3`); screenshot demo di README (`b2059db`); temuan dataset riset (`b5596f3`).

## 2026-09-24 — Revamp v2
- Platform multi-tenant baru: Rust (core-api, rule-service, graph-service), Python (ml, llm, ingest), Postgres RLS,
  OpenSearch, Ollama, Nuxt (`cfe26d6`). Kode v1 dipindah ke folder `legacy/` dan branch `legacy/python-fastapi-mongo`.

## Branch lain
- `eval/public-datasets` (`83f889f`, `bf5deaa`): `tools/public_eval` — evaluasi end-to-end pada 5 dataset publik
  (TabFormer, Fraud_Data, AMLworld, RBA, ULB). Belum di-merge dan belum di-push.
