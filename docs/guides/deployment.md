# Deployment & Operasional

## 1. Topologi

```
Klien (10.100.21.0/24, 192.168.1.0/24)
        │  :3000 (UI)   :8080 (API)
        ▼
  gateway (Traefik) ── IP allow-list ── routing per path ──┐
        │                                                  │
   web (Nuxt BFF)          core-api · rule-service · graph-service · ml-service · llm-service · ingest-service
                                          │
                     postgres · opensearch · ollama   (jaringan internal, tidak di-publish)
```

* Hanya **gateway** yang di-publish, dengan dua entrypoint: UI (`WEB_HOST_PORT`, default 3000) dan API
  (`GATEWAY_HOST_PORT`, default 8080). Container web tidak di-publish langsung, jadi semua akses UI juga melewati
  allow-list.
* Postgres (`127.0.0.1:5433`) dan Ollama (`127.0.0.1:11434`) hanya di-publish ke localhost untuk kebutuhan admin.
* Jaringan docker `frontend` (`172.31.250.0/24`) dan `backend` (`172.31.251.0/24`) memakai subnet tetap agar bisa
  dimasukkan ke allow-list.

## 2. Akses jaringan (allow-list)

`.env`:

```dotenv
BIND_ADDRESS=0.0.0.0
ALLOWED_SOURCE_RANGES=10.100.21.0/24,192.168.1.0/24,172.31.250.0/24,172.31.251.1/32
```

| Entri | Arti |
|---|---|
| `10.100.21.0/24` | jaringan VPN/kantor |
| `192.168.1.0/24` | LAN |
| `172.31.250.0/24` | internal: web BFF → gateway (**wajib**) |
| `172.31.251.1/32` | akses dari server itu sendiri, masuk lewat bridge docker (**wajib** jika perlu akses lokal) |

* Tambah jaringan baru dengan menambahkan CIDR, lalu jalankan `docker compose up -d gateway`.
* Allow-list melihat **IP sumber koneksi TCP**. Header `X-Forwarded-For` tidak dipercaya.
* Jika ada reverse proxy/load balancer di depan gateway, IP sumber yang terlihat adalah IP proxy tersebut. Masukkan
  IP proxy ke allow-list dan batasi akses di proxy.
* Sebagai pertahanan berlapis, batasi juga di firewall host. Contoh ufw:

```bash
sudo ufw allow from 10.100.21.0/24 to any port 3000,8080 proto tcp
sudo ufw allow from 192.168.1.0/24 to any port 3000,8080 proto tcp
```

> Docker menulis aturan iptables sendiri untuk port yang di-publish dan bisa melewati ufw. Karena itu allow-list di
> gateway tetap menjadi kontrol utama. Untuk mengikat hanya ke satu interface, set `BIND_ADDRESS` ke IP interface
> tersebut (mis. IP WireGuard).

## 3. Checklist produksi

| Area | Tindakan |
|---|---|
| Secret | Semua `CHANGE_ME` diganti dengan nilai acak ≥ 32 karakter. `.env` hanya bisa dibaca root (`chmod 600`). Idealnya pakai secrets manager (backlog). |
| `PII_PEPPER` | Jangan pernah diubah setelah data masuk: hash kartu/rekening lama tidak akan cocok lagi. Backup nilainya. |
| TLS | Pasang sertifikat di Traefik (tambah entrypoint `websecure` + `tls`), set `SESSION_COOKIE_SECURE=true`, dan aktifkan HSTS (`stsSeconds` di `deploy/gateway/dynamic.yml`). |
| OpenSearch | Aktifkan security plugin + TLS (hapus `DISABLE_SECURITY_PLUGIN`), lalu set kredensial di llm-service. |
| Postgres | Password role unik per service (sudah dipisah). Aktifkan backup (bagian 5). Pertimbangkan instance terkelola. |
| Model LLM | Tarik model sebelum go-live (`docker compose up ollama-pull`). Setelah itu jaringan `backend` boleh diset `internal: true` (tanpa egress). |
| Demo | `SEED_DEMO=false` di produksi. |
| Admin | Ganti password admin setelah login pertama. Buat tenant admin, lalu platform admin tidak dipakai untuk kerja harian (platform admin memang tidak punya akses ke data project). |
| Resource | OpenSearch heap (`OPENSEARCH_HEAP`), pool DB per service (`*_DB_POOL`), `ML_TRAINING_WORKERS`. |
| Replika | rule-service & graph-service stateless dan bisa diskalakan horizontal. ml-service: training cukup 1 replika. web: multi-replika butuh sticky session. |

## 4. LLM: Ollama lokal atau server GPU eksternal

Secara default llm-service memakai container `ollama` lokal. Jika sudah ada server Ollama ber-GPU (mis.
**nvda11-gpu** yang sudah menjalankan model qwen), arahkan ke sana lewat `.env`:

```dotenv
OLLAMA_URL=http://nvda11-gpu:11434
OLLAMA_CHAT_MODEL=<nama model qwen di server itu>     # cek: curl http://nvda11-gpu:11434/api/tags
OLLAMA_EMBED_MODEL=bge-m3                              # harus tersedia juga di server itu (ollama pull bge-m3)
```

Lalu jalankan `docker compose up -d llm-service`. Container llm-service harus bisa me-resolve dan menjangkau host
tersebut (DNS/`/etc/hosts` atau pakai IP-nya). Model embedding harus **sama** untuk seluruh index: kalau model
embedding diganti, dokumen regulasi harus di-upload ulang (dimensi vektor bisa berbeda).

### GPU lokal

```bash
docker compose -f docker-compose.yml -f docker-compose.gpu.yml up -d
```

Butuh NVIDIA Container Toolkit di host. Tanpa GPU, model 7B tetap berjalan di CPU, tetapi respons chat dan analisis
lebih lambat (hitungan puluhan detik). Untuk server kecil, pakai model yang lebih ringan, misalnya
`OLLAMA_CHAT_MODEL=qwen2.5:3b-instruct`.

## 5. Backup & restore

```bash
# Postgres (semua schema)
docker compose exec -T postgres pg_dump -U postgres -Fc fraud > backup/fraud_$(date +%F).dump
docker compose exec -T postgres pg_restore -U postgres -d fraud --clean < backup/fraud_YYYY-MM-DD.dump

# Artefak model ML, file regulasi, upload
docker run --rm -v fraud-platform_ml_models:/v -v "$PWD/backup":/b alpine tar czf /b/ml_models.tgz -C /v .
docker run --rm -v fraud-platform_regulations:/v -v "$PWD/backup":/b alpine tar czf /b/regulations.tgz -C /v .
```

Index OpenSearch bisa dibangun ulang dari file regulasi (upload ulang). Untuk restore yang cepat, pakai snapshot
repository OpenSearch.

## 6. Upgrade

```bash
git pull
docker compose build
docker compose up -d        # "migrate" menjalankan migrasi baru sebelum service lain start
```

Migrasi bersifat maju saja (forward-only). Selalu backup sebelum upgrade.

## 7. Monitoring

* **Health:** `docker compose ps` (semua healthcheck). Gateway juga memeriksa `/health/ready` tiap service.
* **Metrics:** `/metrics` (Prometheus) di setiap service dan di gateway. Stack Prometheus + Grafana opsional ada di
  backlog.
* **Log:** JSON ke stdout; rotasi otomatis 10 MB × 3 per container. Lacak lintas service dengan `x-request-id`.
* **Metrik yang perlu dipantau:**
  * latensi scoring p95 (< 150 ms);
  * `degraded_rate` di dashboard (engine yang gagal/timeout);
  * rasio trapped per rule;
  * drift PSI;
  * jumlah case terbuka.

## 8. Troubleshooting

| Gejala | Penyebab umum |
|---|---|
| UI/API 403 | IP klien tidak ada di `ALLOWED_SOURCE_RANGES` (cek `docker compose logs gateway`, field `ClientHost`) |
| Port 3000 "already allocated" | port dipakai aplikasi lain, ganti `WEB_HOST_PORT` |
| `migrate` gagal | kredensial `MIGRATOR_DB_PASSWORD` berbeda dengan saat volume Postgres pertama dibuat. Role dibuat sekali saat init volume. |
| Chat/analisis AI lambat atau timeout | model belum ditarik (`ollama-pull`) atau tanpa GPU. Gunakan model lebih kecil. |
| Keputusan selalu "approve" di project baru | belum ada model aktif dan threshold belum dikalibrasi. Lihat `evaluation.md` dan bagian Settings di panduan pengguna. |
| `degraded: ["supervised"]` | ml-service down/timeout. Keputusan tetap keluar dari engine lain. |
