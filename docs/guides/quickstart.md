# Quickstart

Menjalankan seluruh platform di satu mesin, lengkap dengan data demo, dalam ± 20–40 menit (sebagian besar waktu
dipakai untuk build image dan mengunduh model LLM).

## 1. Prasyarat

| Kebutuhan | Minimal | Catatan |
|---|---|---|
| Docker Engine + Compose v2 | 24+ | `docker compose version` |
| RAM | 12 GB (16 GB disarankan) | OpenSearch 1 GB heap, Ollama ± 5 GB untuk model 7B |
| Disk | 20 GB | image ± 4 GB, model LLM ± 6 GB, data |
| GPU (opsional) | NVIDIA + container toolkit | mempercepat LLM (lihat `deployment.md`) |

Semua Dockerfile kompatibel dengan builder klasik maupun BuildKit, jadi `docker buildx` tidak wajib.

## 2. Konfigurasi

```bash
git clone git@github.com:situkangsayur/fraud_detection_engine.git
cd fraud_detection_engine
cp .env.example .env
```

Ganti **semua** nilai `CHANGE_ME` di `.env`. Contoh untuk membuat secret acak:

```bash
openssl rand -hex 32
```

Variabel yang paling sering diubah:

| Variabel | Fungsi |
|---|---|
| `ADMIN_EMAIL`, `ADMIN_PASSWORD` | akun platform admin pertama |
| `DEMO_USER_PASSWORD` | password `analyst@demo.local` dan `approver@demo.local` (tenant demo) |
| `SEED_DEMO` | `true` membuat tenant demo + 4 project dengan template rule |
| `ALLOWED_SOURCE_RANGES` | jaringan yang boleh mengakses UI/API (lihat bagian 5) |
| `WEB_HOST_PORT`, `GATEWAY_HOST_PORT` | port UI (default 3000) dan API (default 8080) |
| `OLLAMA_CHAT_MODEL`, `OLLAMA_EMBED_MODEL` | model LLM (default `qwen2.5:7b-instruct`, `bge-m3`) |

## 3. Jalankan

```bash
docker compose up -d --build        # build + start (migrasi DB berjalan otomatis lewat service "migrate")
docker compose ps                   # tunggu semua "healthy"
docker compose up ollama-pull       # unduh model LLM (sekali saja)
```

## 4. Data demo (opsional)

```bash
docker compose --profile seed run --rm simulator
# lebih kecil/cepat:
docker compose --profile seed run --rm -e SIM_CUSTOMERS=300 -e SIM_DAYS=30 simulator
```

Simulator membuat data sintetis untuk 4 project (checkout, post-payment, returns, promo). Datanya berisi pola fraud
nyata (carding, ATO, sistem dibobol, pengambilalihan rekening, money mule, abuse retur, abuse promo) dan dikirim
lewat webhook dengan struktur record "milik klien", sehingga fitur mapping ikut teruji. Setelah itu:

1. Login sebagai `analyst@demo.local`, buka **ML → Supervised** lalu **Train**, kemudian **Submit**.
2. Login sebagai `approver@demo.local` dan **Approve** model tersebut. Lakukan hal yang sama di **ML → Unsupervised**.
3. Event berikutnya dinilai oleh kelima engine.

## 5. Akses

| Apa | URL |
|---|---|
| Web UI | `http://<ip-server>:<WEB_HOST_PORT>` |
| API | `http://<ip-server>:<GATEWAY_HOST_PORT>/api/v1` |
| OpenAPI | `http://<ip-server>:8080/api/openapi/core.json` (juga `rule`, `graph`, `ml`, `llm`, `ingest`) |

UI dan API hanya bisa diakses dari jaringan yang tercantum di `ALLOWED_SOURCE_RANGES`. Nilai default-nya:

```
10.100.21.0/24      # jaringan VPN / kantor
192.168.1.0/24      # LAN
172.31.250.0/24     # internal docker (web → gateway)   ← jangan dihapus
172.31.251.1/32     # akses dari server itu sendiri      ← jangan dihapus
```

Klien dari jaringan lain mendapat HTTP **403**. Setelah mengubah daftar ini, jalankan
`docker compose up -d gateway`.

> Jika port 3000 sudah dipakai aplikasi lain di server, set `WEB_HOST_PORT` ke port lain (mis. `3080`).

## 6. Cek cepat

```bash
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:${WEB_HOST_PORT:-3000}/login     # 200
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8080/api/v1/me                    # 401 (butuh login)
docker compose logs -f core-api                                                            # log JSON
```

## 7. Berhenti / reset

```bash
docker compose down        # stop (data tetap ada di volume)
docker compose down -v     # stop + HAPUS semua data (database, model, index)
```
