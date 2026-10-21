---
title: PRD — Multi-Agent & AI Console
status: draft (belum diimplementasi)
created: 2026-10-21
owner: Hendri Karisma
---

# PRD — Multi-Agent & AI Console

Dokumen kebutuhan produk (Product Requirements Document) untuk dua hal yang saling terkait:

1. **Lapisan multi-agent**: beberapa agen AI dengan peran berbeda yang saling memeriksa. Desain teknisnya sudah ada di
   [multi-agent.md](../technical/multi-agent.md).
2. **AI Console**: halaman admin untuk **mengelola agen dan konfigurasi AI**, yaitu provider (Ollama lokal di
   nvda11-gpu, Gemini, dan nanti provider lain), model yang dipakai, **beberapa model sekaligus** (model berbeda per agen
   dan model cadangan), API key Gemini, serta batasan dan kill switch.

Spesifikasi teknis: [TRD — Agent & AI Console](../technical/trd-agent-ai-console.md). Story dan status: epic **E16** dan
**E18** di [backlog](../backlog.md).

## 1. Latar belakang & masalah

| Masalah sekarang | Dampak |
|---|---|
| Provider dan model LLM diatur **global lewat `.env`** (`LLM_PROVIDER`, `OLLAMA_CHAT_MODEL`, `GEMINI_API_KEY`). Mengganti model = edit file + restart container oleh orang yang punya akses server. | Admin tenant atau project tidak bisa memilih model sendiri; tiap eksperimen model butuh engineer. |
| Hanya **satu provider aktif** untuk seluruh platform. | Tidak bisa memakai Gemini untuk satu agen (misalnya Critic) dan Ollama untuk agen lain. |
| Kolom `chat_model` per project berupa **teks bebas** dengan default `qwen2.5:7b-instruct`, padahal model itu tidak terpasang di host (yang terpasang `qwen3:8b`). | Project baru bisa diam-diam memakai model yang tidak ada → chat gagal. |
| API key Gemini hanya ada di `.env` server. | Tenant tidak bisa memakai key sendiri; rotasi key butuh restart. |
| Belum ada multi-agent: satu agen dengan 14 tools mengerjakan semuanya. | Usulan rule tidak diperiksa ulang secara independen; pasal regulasi bisa salah kutip. |
| Tidak ada tampilan pemakaian AI (token, latensi, error per model). | Sulit menilai model mana yang layak dan berapa biayanya. |

## 2. Tujuan

| # | Tujuan | Ukuran keberhasilan |
|---|---|---|
| T1 | Admin bisa mengganti model/provider **tanpa edit `.env` dan tanpa restart** | perubahan berlaku ≤ 60 detik setelah disimpan |
| T2 | **Multi-model**: tiap agen bisa memakai model berbeda + model cadangan | minimal 2 provider aktif bersamaan di satu run multi-agent |
| T3 | API key tersimpan aman | key terenkripsi di database, tidak pernah tampil utuh lagi setelah disimpan, setiap perubahan tercatat di audit |
| T4 | Mencegah konfigurasi yang rusak | model yang dipilih wajib lolos "Test connection"; model yang tidak terpasang tidak bisa dipilih |
| T5 | Data tetap terkendali | data tenant hanya dikirim ke provider eksternal (Gemini) bila tenant **secara eksplisit** mengizinkan |
| T6 | Multi-agent F1 (Regulation-to-Rule) berjalan dengan konfigurasi dari AI Console | kriteria M3 di [roadmap](../roadmap.md) |

**Bukan tujuan (non-goals):** fine-tuning model; menjalankan LLM di jalur scoring real-time; agen yang mengaktifkan rule
sendiri (tetap maker–checker); marketplace model; mengganti model embedding lewat UI (butuh re-index, tetap lewat ops).

## 3. Persona & hak akses

| Persona | Peran sistem | Yang bisa dilakukan di AI Console |
|---|---|---|
| **Platform admin** (operator demo, pemilik server) | `is_platform_admin` | Mendaftarkan provider tingkat platform (Ollama nvda11-gpu, Gemini platform), menentukan model yang **boleh** dipakai, default agen tingkat platform, kill switch global, melihat pemakaian semua tenant |
| **Tenant admin** | `tenant_admin` | Mengizinkan/melarang provider eksternal untuk tenant, memasukkan API key Gemini milik tenant, override default agen untuk tenant, melihat pemakaian tenant |
| **Project admin** | `project_admin` | Memilih model per agen untuk project **dari daftar yang diizinkan**, suhu/bahasa, menyalakan/mematikan agen di project (kill switch project) |
| **Approver** | `approver` | Menyetujui perubahan yang sensitif (prompt agen, daftar tool agen), lihat §5 |
| **Analyst / viewer** | `analyst`, `viewer` | Hanya melihat agen & model mana yang dipakai (transparansi), tidak bisa mengubah |

## 4. Kebutuhan fungsional

### 4.1 Provider & kredensial
- **FR-1** Daftar provider: Ollama (URL, contoh `http://host.docker.internal:11434` di nvda11-gpu), Gemini (API key,
  base URL OpenAI-compatible). Dirancang agar provider lain yang OpenAI-compatible (misalnya Claude atau vLLM) bisa
  ditambah tanpa mengubah UI.
- **FR-2** Tombol **Test connection**: cek koneksi, versi server, daftar model, dan satu panggilan chat singkat.
  Hasil ditampilkan (OK / pesan error yang mudah dipahami).
- **FR-3** API key: hanya bisa **ditulis**, tidak bisa dibaca ulang. Setelah disimpan hanya tampil `••••abcd` (4 karakter terakhir), kapan
  dibuat, dan oleh siapa. Ada aksi **Rotate** dan **Revoke**.
- **FR-4** Tingkat kepemilikan: provider platform (dipakai semua tenant yang mengizinkan) dan provider tenant (key milik
  tenant sendiri).

### 4.2 Katalog model
- **FR-5** Model per provider diambil otomatis (Ollama `/api/tags`, Gemini model list) lalu **diaktifkan** oleh admin.
  Hanya model aktif yang muncul di pilihan.
- **FR-6** Setiap model punya kapabilitas: tool calling, JSON schema, context window, "thinking", dan perkiraan VRAM
  untuk model lokal. UI memberi **peringatan** bila model tidak muat di GPU 8 GB atau context terlalu besar.
- **FR-7** Status kesehatan model: terakhir dites, latensi rata-rata, tingkat error 24 jam.

### 4.3 Agen
- **FR-8** Daftar agen beserta perannya: Chat assistant, Analysis pipeline, dan agen multi-agent (Regulation,
  Analytics, Pattern, Designer, Critic, Reporter, Router). Setiap agen menampilkan: tujuan, model utama, model cadangan,
  tools yang dipakai, batas (iterasi, token, waktu), status aktif.
- **FR-9** **Multi-model**: setiap agen memilih model utama + **daftar model cadangan berurutan** (fallback bila timeout
  atau error). Rekomendasi bawaan: Critic memakai model berbeda dari Designer agar kesalahannya tidak sejalan.
- **FR-10** Pengaturan per agen: suhu, mode thinking, panjang konteks, maksimum iterasi tool, bahasa.
- **FR-11** **Tools per agen** hanya bisa **dipersempit** dari daftar maksimum yang ditetapkan kode (least privilege). Tool tulis
  (`create_rule_proposal`) tidak pernah bisa diberikan ke agen LLM.
- **FR-12** Prompt tambahan per agen (instruksi khusus tenant/project), dengan riwayat versi.
- **FR-13** Kill switch: global (platform), per tenant, per project, per agen. Saat dimatikan, run yang sedang
  berjalan dibatalkan dengan status yang jelas.

### 4.4 Urutan penerapan konfigurasi
- **FR-14** Konfigurasi efektif = **project → tenant → platform → `.env`** (yang paling spesifik menang). UI
  menampilkan "Efektif: `ollama/qwen3:8b` (dari: platform default)" agar asal setiap nilai jelas.
- **FR-15** Bila konfigurasi di database kosong atau tidak valid, sistem tetap jalan memakai `.env` (perilaku hari ini).

### 4.5 Pemantauan
- **FR-16** Halaman **Usage**: jumlah panggilan, token masuk/keluar, latensi p50/p95, error, per provider/model/agen/
  tenant, harian. Untuk Gemini ditampilkan perkiraan biaya.
- **FR-17** Setiap run agen mencatat model yang **benar-benar** dipakai (termasuk bila fallback terjadi). Data ini terlihat di
  timeline Agent Runs dan di laporan.

### 4.6 Multi-agent (ringkas; detail di multi-agent.md)
- **FR-18** F1 Regulation-to-Rule, Critic + `verify_citation`, halaman Agent Runs dengan timeline live, jejak
  multi-agent di Proposal Review. Semua memakai konfigurasi agen dari AI Console.

## 5. Tata kelola & keamanan

| Aturan | Alasan |
|---|---|
| Perubahan **prompt agen** dan **daftar tool agen** wajib maker–checker (dua orang) | memengaruhi perilaku agen terhadap data dan proposal rule |
| Perubahan model, suhu, kill switch: langsung berlaku tetapi **tercatat di audit** (siapa, kapan, sebelum → sesudah) | perlu cepat saat insiden; tetap bisa dilacak |
| API key terenkripsi (AES-256-GCM) dengan master key dari server; tidak pernah dikirim balik ke browser atau dicatat di log | kebocoran key = biaya & penyalahgunaan |
| Provider eksternal (Gemini) **mati secara default** per tenant; tenant admin harus mengaktifkan dan menyetujui pernyataan pemrosesan data | UU PDP: Gemini = pemroses data pihak ketiga di luar negeri |
| Data yang dikirim ke LLM tetap data yang sudah di-mask/hash (perilaku tools sekarang) | minimisasi data |
| Agen tidak pernah mengaktifkan rule, threshold, atau model | prinsip "AI memberi saran, manusia memutuskan" |

Rincian: [governance-compliance.md](governance-compliance.md).

## 6. Alur utama (user stories)

1. **Ganti model demo** — Platform admin membuka AI Console → Models, klik *Refresh* pada provider Ollama nvda11-gpu,
   mengaktifkan `qwen3:8b`, lalu menjadikannya default agen platform. Perubahan langsung berlaku, tanpa restart.
2. **Pakai Gemini untuk Critic** — Tenant admin mengizinkan provider eksternal, memasukkan API key Gemini, dan menjalankan
   *Test connection*. Project admin lalu memilih `gemini-flash` untuk agen Critic dengan cadangan `ollama/qwen3:8b`.
3. **Gemini down** — Panggilan Critic ke Gemini timeout → otomatis pindah ke `qwen3:8b`. Timeline run menandai
   "fallback". Halaman Usage memperlihatkan lonjakan error Gemini.
4. **Insiden** — Agen menghasilkan proposal yang aneh. Project admin mematikan agen Designer (kill switch), run yang berjalan
   dibatalkan, dan audit mencatat siapa yang mematikan.
5. **Ubah prompt Critic** — Project admin mengusulkan prompt baru → approver menyetujui → versi baru aktif; versi lama tetap
   bisa dilihat.

## 7. Status per 2026-10-21: yang sudah ada & yang belum

| Kemampuan | Status | Catatan |
|---|---|---|
| Client Ollama (chat, embedding, thinking, timeout, context) | ✅ ada | `clients/ollama.py` |
| Client Gemini (chat via endpoint OpenAI-compatible; embedding tetap Ollama) | ✅ ada | `clients/gemini.py`, dipilih **global** lewat `LLM_PROVIDER` |
| Satu agen tool-calling (14 tools) + pipeline analisis + RAG regulasi | ✅ ada | lihat [llm-agentic.md](../technical/llm-agentic.md) |
| `llm_config` per project (`chat_model`, suhu, bahasa, prompt tambahan) | 🟡 sebagian | tersimpan di DB dan dipakai llm-service; hanya bisa diisi saat **membuat** project, berupa teks bebas, default salah (`qwen2.5:7b-instruct`) |
| Pilih provider per project / per agen | ❌ belum | provider global |
| Multi-model + fallback | ❌ belum | — |
| Penyimpanan API key di DB (terenkripsi) | ❌ belum | key hanya di `.env` |
| Katalog model + test connection | ❌ belum | — |
| Registry agen, orchestrator, Critic, `verify_citation` | ❌ belum (desain selesai) | [multi-agent.md](../technical/multi-agent.md) |
| UI AI Console, Agent Runs, Usage | ❌ belum (desain UX Agent Runs ada) | [ux-design.md](../technical/ux-design.md) |
| Metrik token per panggilan | ❌ belum | Ollama sudah mengirim `prompt_eval_count`/`eval_count`, belum dicatat |
| Infrastruktur: Ollama di nvda11-gpu, stack autostart (systemd) | ✅ ada | GPU driver belum termuat → model jalan di CPU (blocker B1) |

## 8. Prasyarat & risiko

| # | Hal | Dampak | Mitigasi |
|---|---|---|---|
| R1 | Driver GPU nvda11-gpu belum jalan | multi-agent di CPU terlalu lambat untuk demo | perbaiki DKMS (runbook §7) sebelum Fase 2 |
| R2 | Satu GPU 8 GB: dua model lokal berbeda = model saling menggeser dari VRAM | latensi naik | UI memperingatkan; antrean LLM tunggal; sarankan model kedua dari provider cloud |
| R3 | Akun Gemini pemilik bermasalah | multi-provider tidak bisa diuji live | uji dengan mock + key tenant lain; Ollama tetap default |
| R4 | Kebocoran API key | biaya & penyalahgunaan | enkripsi, write-only, audit, rotasi |
| R5 | Disk nvda11-gpu ±100% | OpenSearch berhenti mengindeks, deploy gagal | bersihkan sebelum pengembangan |

## 9. Rilis bertahap

| Tahap | Isi | Kriteria selesai |
|---|---|---|
| **R1 — AI Console dasar** | provider + kredensial terenkripsi, katalog model, test connection, default platform/tenant/project, perbaikan field `chat_model` | ganti model tanpa restart; key tidak pernah tampil utuh |
| **R2 — Agen & multi-model** | registry agen di DB, model per agen + fallback, kill switch, pencatatan model & token per panggilan, Usage | chat & analisis memakai config agen; fallback teruji |
| **R3 — Multi-agent MVP** | F1 + Critic + Agent Runs (E16), memakai konfigurasi R2 | M3 di roadmap |
| R4 — Lanjutan | prompt agen dengan maker–checker, perkiraan biaya, provider tambahan (Claude/OpenAI-compatible) | — |
