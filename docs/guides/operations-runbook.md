# Runbook Operasional (nvda11-gpu)

Panduan harian untuk instalasi demo publik di host **nvda11-gpu**. Konsep, konfigurasi, dan checklist produksi ada di
[deployment.md](deployment.md); seeding dan master snapshot di [experiments.md](experiments.md).

> Semua perintah dijalankan dari **checkout deploy** (`cd ~/own_project/deployment/fraud_detection_engine`) kecuali
> disebut lain. Pengembangan kode dan docs dilakukan di checkout terpisah `~/own_project/fraud_detection_engine`
> (lihat §9); jangan deploy dari checkout dev.
> Jangan pernah menampilkan isi `.env` (secret). Untuk memeriksa satu nilai non-rahasia:
> `grep -E '^OLLAMA_CHAT_MODEL=' .env`.

## 0. Peta lingkungan

| Hal | Nilai |
|---|---|
| Checkout deploy | `~/own_project/deployment/fraud_detection_engine` (punya `.env`, `.venv`, worktree eval), branch v2 `revamp/rust-nuxt-stack`; sebelum 2026-10-21 berada di `~/own_project/apps/fraud_detection_engine` |
| Checkout dev & docs | `~/own_project/fraud_detection_engine` (clone bersih; tempat menulis kode/docs, bukan untuk deploy) |
| Autostart | systemd user unit `~/.config/systemd/user/fraud-platform.service` (enabled, linger aktif): `docker compose up -d` untuk `fraud-platform` + `fraud-eval` saat boot; container juga `restart: unless-stopped` |
| Reset malam | crontab `0 0 * * * ~/own_project/deployment/fraud_detection_engine/deploy/seed.sh restore` |
| Stack demo | compose project `fraud-platform`: gateway `:8080` (API), `:3080` (UI), Postgres `127.0.0.1:5433` |
| URL publik | `https://fds.hendrikarisma.my.id` (reverse proxy TLS di luar host → UI `:3080`) |
| Stack evaluasi | compose project `fraud-eval`: `docker-compose.yml` repo + `~/fraud-eval/compose.eval.yml`, env `~/fraud-eval/.env.eval`; gateway `127.0.0.1:18090`, Postgres `127.0.0.1:5437`; worktree `~/fraud-eval/repo` (branch `eval/public-datasets`) |
| LLM | Ollama di host (`ollama.service`), dipanggil lewat `OLLAMA_URL=http://host.docker.internal:11434`; `qwen3:8b` (chat) + `bge-m3` (embedding), `OLLAMA_NUM_CTX=20480`, `OLLAMA_THINK=false`, `OLLAMA_TIMEOUT_S=900`; Gemini opsional (`LLM_PROVIDER=gemini`) |
| GPU | RTX 3060 Ti 8 GB; **driver belum termuat** (2026-10-13), model berjalan di CPU dan sangat lambat |
| Reset malam | cron `0 0 * * *` → `deploy/seed.sh restore`, log `~/.local/state/fraud-demo-reset.log` |
| Master snapshot | `~/fraud-demo-master/<UTC timestamp>/`, `current` → yang dipakai restore |
| Dataset | `~/datasets/fraud-public/` (input publik), `~/datasets/fraud-research/`, `~/datasets/fraud-live-demo/` (ekspor) |
| State reset | `~/.local/state/fraud-demo/last-run.env`, lock `/tmp/fraud-demo-reset.lock` |

Volume data (tidak pernah dihapus oleh `down` tanpa `-v`): `fraud-platform_{pg_data,os_data,ml_models,regulations,uploads}`
dan `fraud-eval_pg_data` dst. Nama project compose ditetapkan di `docker-compose.yml`, jadi tidak bergantung pada nama
direktori.

## 1. Health check harian

```bash
cd ~/own_project/deployment/fraud_detection_engine
systemctl --user status fraud-platform.service --no-pager | head -3   # active (exited) = stack sudah dinaikkan
docker compose ps --format 'table {{.Service}}\t{{.Status}}'      # semua "(healthy)"; migrate tidak tampil (sudah exit)
docker compose -p fraud-eval ps --format 'table {{.Service}}\t{{.Status}}'

curl -s -o /dev/null -w "ping %{http_code}\n"  http://127.0.0.1:8080/ping          # 200 (Traefik)
curl -s -o /dev/null -w "api  %{http_code}\n"  http://127.0.0.1:8080/api/v1/me     # 401 (butuh login) = sehat
curl -s -o /dev/null -w "ui   %{http_code}\n"  http://127.0.0.1:3080/login         # 200
curl -s -o /dev/null -w "pub  %{http_code}\n"  https://fds.hendrikarisma.my.id/login   # 200
curl -s -o /dev/null -w "eval %{http_code}\n"  http://127.0.0.1:18090/ping         # 200

tail -n 3 ~/.local/state/fraud-demo-reset.log    # baris terakhir: "master: restored and time-shifted"
df -h /                                          # OpenSearch memblok index di ≥ 95% (flood stage)
curl -s localhost:11434/api/version; ollama ps   # Ollama hidup? model di GPU atau CPU?
```

Health detail per service (dari dalam container, tanpa lewat gateway):

```bash
docker compose exec core-api /app/service healthcheck http://127.0.0.1:8080/health/ready && echo ok   # rule-service :8081, graph-service :8082
docker compose exec ml-service python -c "import urllib.request as u; print(u.urlopen('http://127.0.0.1:8001/health/ready').status)"
# llm-service :8002, ingest-service :8003
```

> Disk host sudah 94% pada 2026-10-13. Setiap master ±600 MB dan setiap ekspor dataset harian menambah isi
> `~/datasets/`. Hapus master lama yang bukan `current` (`deploy/demo/master.sh info`) bila perlu.

## 2. Log

```bash
docker compose logs --since 30m -f core-api            # log JSON, rotasi 10 MB × 3 per container
docker compose logs --since 1h gateway | grep -v '"RequestPath":"/ping"'   # access log Traefik (ClientHost, status)
docker compose logs core-api rule-service | grep <x-request-id>            # lacak satu request lintas service
docker compose logs --since 1h llm-service | grep -E 'analysis_prompt|error'
journalctl -u ollama --since "1 hour ago" | grep -E 'truncating|error'      # prompt terpotong / error Ollama
tail -f ~/.local/state/fraud-demo-reset.log                                # reset/restore malam
docker compose -p fraud-eval logs --since 1h core-api                     # stack evaluasi
```

Log level: `LOG_LEVEL` di `.env` (mengisi juga `RUST_LOG`), lalu `docker compose up -d <service>`.

## 3. Restart satu service

```bash
docker compose restart llm-service          # restart cepat, konfigurasi tidak dibaca ulang
docker compose up -d llm-service            # recreate: membaca ulang .env / docker-compose.yml
docker compose up -d gateway                # setelah mengubah ALLOWED_SOURCE_RANGES atau deploy/gateway/*
```

* `deploy/gateway/dynamic.yml` di-*watch* Traefik, tetapi nilai `ALLOWED_SOURCE_RANGES` dibaca dari env gateway, jadi
  perubahan `.env` butuh `up -d gateway`.
* Restart `postgres` membuat service lain kehilangan koneksi sebentar; lebih aman `docker compose up -d` setelahnya.
* Jangan restart saat reset/restore sedang berjalan (cek `pgrep -af 'seed.sh|master.sh|reset-demo'`).

## 4. Rebuild setelah pull kode

```bash
cd ~/own_project/deployment/fraud_detection_engine
git status                                   # harus bersih: kode/docs ditulis di checkout dev, lalu di-push
git pull
docker compose build                         # atau hanya yang berubah: docker compose build web llm-service
docker compose up -d --wait --wait-timeout 900   # "migrate" menjalankan migrasi baru lebih dulu
docker compose ps
```

* Jalankan di luar jam 00.00 (restore memakai `docker compose stop/up`).
* Perubahan simulator: `docker compose --profile seed build simulator`.
* Perubahan `tools/*`: `uv run` menyinkronkan `.venv` otomatis saat seeding berikutnya.
* `fraud-eval` memakai image yang sama (`fraud-platform/<service>:dev`) tetapi tetap jalan dengan image lama sampai
  container-nya di-recreate. Recreate hanya bila evaluasi memang butuh kode baru.
* Master snapshot berisi data dari commit lama (`~/fraud-demo-master/current/git-commit.txt`). Restore malam tetap
  aman karena `migrate` menerapkan migrasi baru saat stack naik. Bila fitur baru butuh data seed baru, buat master
  baru (§6).

## 5. Reset malam & cara memverifikasinya

Cron 00.00 menjalankan `deploy/seed.sh restore` → `deploy/demo/master.sh restore`:

1. `docker compose stop`;
2. ekstrak `pg_data`, `os_data`, `ml_models`, `regulations`, `uploads` dari `~/fraud-demo-master/current/`;
3. `docker compose up -d --wait --wait-timeout 900`;
4. jalankan `deploy/demo/shift-time.sql`: semua kolom `timestamptz` di schema platform digeser sebesar
   (sekarang − `demo_master.meta.snapshot_at`), `snapshot_at` di-set ke sekarang, dan semua refresh token dihapus
   (semua pengguna harus login ulang).

Downtime ±3–5 menit. Verifikasi pagi hari:

```bash
crontab -l | grep fraud                                   # path harus menunjuk ke repo yang benar
grep -E 'master: (restoring|restored)|NOTICE|error|no master' ~/.local/state/fraud-demo-reset.log | tail -n 4
#   ... master: restoring 20261009T092144Z
#   NOTICE:  timestamps shifted by …
#   ... master: restored and time-shifted
docker compose ps --format 'table {{.Service}}\t{{.Status}}'
deploy/demo/master.sh info                                # master aktif + jumlah event per tenant
```

Cek bahwa pergeseran waktu terjadi: `snapshot_at` harus sama dengan waktu restore terakhir (00.00 WIB = 17.00 UTC),
dan event terbaru berjarak tetap dari situ (jarak antara akhir seeding dan pembuatan master, bisa beberapa jam):

```bash
docker compose exec -T postgres sh -c 'psql -U "$POSTGRES_USER" -d "${POSTGRES_DB:-fraud}" -tA -c "
  SELECT value FROM demo_master.meta WHERE key = '"'"'snapshot_at'"'"';
  SELECT max(occurred_at) FROM core.events;"'
```

Lalu login di `https://fds.hendrikarisma.my.id` dan buka dashboard tenant demo.

Restore gagal? Lihat §8 (baris "Restore malam gagal"). Menjalankan restore manual aman diulang:

```bash
deploy/seed.sh restore >> ~/.local/state/fraud-demo-reset.log 2>&1
```

## 6. Backup & restore

**Demo/eksperimen: master snapshot** (semua tenant, semua volume data):

```bash
deploy/demo/master.sh info        # daftar master + tenant di master aktif
deploy/demo/master.sh snapshot    # simpan kondisi SEKARANG sebagai master baru (stack di-stop ±2–5 menit)
deploy/seed.sh master             # seed ulang demo + 3 dataset publik lalu snapshot (±4–5 jam)
```

* Snapshot langsung dipakai restore malam berikutnya (`current` dipindah). Hanya snapshot ketika kondisi platform
  memang ingin dipertahankan.
* Kembali ke master sebelumnya: `ln -sfn ~/fraud-demo-master/<folder-lama> ~/fraud-demo-master/current`, lalu
  `deploy/seed.sh restore`.
* File `*.tar.gz` di folder master dimiliki root (dibuat dari container); hapus master lama dengan `sudo rm -r`.

**Backup logis Postgres** (untuk data yang tidak boleh hilang, juga sebelum eksperimen berisiko):

```bash
mkdir -p ~/backup/fraud
docker compose exec -T postgres pg_dump -U postgres -Fc fraud > ~/backup/fraud/fraud_$(date +%F).dump
# restore (menimpa isi database):
docker compose exec -T postgres pg_restore -U postgres -d fraud --clean < ~/backup/fraud/fraud_YYYY-MM-DD.dump
```

Volume lain (`ml_models`, `regulations`, `uploads`): lihat [deployment.md](deployment.md) §5. Dataset di
`~/datasets/` tidak disentuh restore dan tidak termasuk master.

## 7. Ollama

```bash
systemctl status ollama --no-pager | head -5
curl -s localhost:11434/api/version                       # 2026-10-13: 0.6.8
curl -s localhost:11434/api/tags | python3 -c "import json,sys; print([m['name'] for m in json.load(sys.stdin)['models']])"
                                                          # harus ada qwen3:8b dan bge-m3
ollama ps                                                 # kolom PROCESSOR: idealnya ≥ 90% GPU
nvidia-smi                                                # gagal = driver tidak termuat
cat /etc/systemd/system/ollama.service.d/*.conf           # OLLAMA_FLASH_ATTENTION=1, OLLAMA_KV_CACHE_TYPE=q8_0
docker compose exec llm-service python -c "import urllib.request as u; print(u.urlopen('http://host.docker.internal:11434/api/version').read())"
```

* Model hilang: `ollama pull qwen3:8b` / `ollama pull bge-m3` (di host, bukan container).
* Ollama 0.6.8 belum mengenal parameter `think`; llm-service tetap menambahkan `/no_think` untuk qwen3, dan blok
  `<think>` selalu dibuang.
* **Driver GPU (tertunda):** modul DKMS `nvidia/595.84` baru dibangun untuk kernel 7.0.0-34, sedangkan kernel
  berjalan 7.0.0-38. Perbaikan (butuh sudo, lakukan di luar jam 00.00):

  ```bash
  dkms status
  sudo apt install linux-headers-$(uname -r)       # bila belum ada
  sudo dkms install nvidia/595.84 -k $(uname -r)
  sudo modprobe nvidia && nvidia-smi
  sudo systemctl restart ollama
  ```

  Setelah GPU aktif, `OLLAMA_TIMEOUT_S` boleh diturunkan (mis. 180–300) dengan `docker compose up -d llm-service`.
* Butuh LLM cepat sebelum GPU beres: `LLM_PROVIDER=gemini` + `GEMINI_API_KEY` di `.env`, lalu
  `docker compose up -d llm-service` (data prompt dikirim ke Google; embedding tetap Ollama).

## 8. Insiden umum & perbaikan

| Gejala | Penyebab | Perbaikan |
|---|---|---|
| Login gagal "Bad Request" / "premature close" | header `Cookie` terlalu besar: banyak aplikasi lain di host yang sama (cookie berlaku per host, bukan per port) | web sudah menerima header s/d 64 KB; bila masih terjadi, hapus cookie browser untuk host tersebut |
| Aksi di UI (latih model, chat, formula) gagal 403 `cross-origin request` lewat `https://fds…` | image web lama (cek origin membandingkan skema) | `docker compose build web && docker compose up -d web` |
| UI/API 403 polos | IP sumber tidak ada di `ALLOWED_SOURCE_RANGES` | `docker compose logs gateway` → field `ClientHost`; tambah CIDR di `.env`, `docker compose up -d gateway` |
| Seeding berhenti `401 ExpiredSignature` | token akses (`JWT_TTL_MINUTES=60`) kedaluwarsa di tengah seed panjang; versi lama tidak login ulang | versi baru simulator/`post_seed.py` login ulang otomatis; rebuild simulator (`docker compose --profile seed build simulator`) bila masih terjadi |
| `post_seed.py` melihat 502/503/504 saat model dilatih | ml-service sibuk training sehingga health check gateway gagal sementara | tunggu; versi baru terus polling (timeout training 30 menit). Cek `docker compose logs -f ml-service` |
| Simulator `skipped: ingest failed … 408` | batch ingest melewati `REQUEST_TIMEOUT_SECS` (30 s) core-api | versi baru memakai batch kecil dan keluar kode 1, `reset-demo.sh` mengulang sekali; ulangi seed bila tetap gagal |
| Restore malam gagal | lihat log: `no master at …` (link `current` rusak), `another reset/snapshot is running` (run lain masih memegang lock), atau stack tidak healthy dalam 900 s | cek `pgrep -af 'seed.sh\|master.sh\|reset-demo'` dan `ls -l ~/fraud-demo-master/current`; `docker compose ps`; jalankan ulang `deploy/seed.sh restore`. Restore berhenti di tengah bisa meninggalkan stack ter-stop; menjalankan ulang aman |
| Cron tidak jalan sama sekali setelah repo dipindah | path di crontab masih lama | perbarui crontab (§9) |
| Upload regulasi gagal `index_create_block_exception` | disk ≥ 95% (flood stage OpenSearch) | kosongkan disk (master lama, `docker image prune`, ekspor dataset lama); blok lepas otomatis |
| Chat/analisis LLM timeout atau sangat lambat | model di CPU (driver GPU), atau `OLLAMA_TIMEOUT_S` terlalu kecil | §7; sementara pakai Gemini |
| Laporan LLM ngawur / tanpa proposal | prompt terpotong (`truncating input prompt` di `journalctl -u ollama`) atau proposal ditolak validator DSL | naikkan `OLLAMA_NUM_CTX` (maks ±20k agar tetap di VRAM 8 GB); lihat `structured.proposals[].validation` di laporan |
| Halaman project 404 setelah reset | ID project berubah | buka `/projects` |
| Pencarian event berdasarkan ID timeout | migrasi `0012` belum diterapkan | `docker compose up -d` (service `migrate` jalan) |
| Docker gagal pull image saat reset (DNS/IPv6) | jaringan host | `reset-demo.sh` mengulang sekali; ulangi manual bila perlu |
| `degraded: ["supervised"]` di keputusan | ml-service down/timeout | `docker compose ps ml-service`, `logs ml-service`; keputusan tetap keluar dari engine lain |

## 9. Layout direktori & autostart (dijalankan 2026-10-21)

Dua checkout dengan peran tetap (keputusan 2026-10-21, menggantikan rencana "satu direktori"):

| Checkout | Peran | Catatan |
|---|---|---|
| `~/own_project/deployment/fraud_detection_engine` | **produksi/demo**: sumber `fraud-platform` + `fraud-eval`, cron restore, worktree `~/fraud-eval/repo` | hanya `git pull` + rebuild (§4); `.env` dan `.venv` ada di sini |
| `~/own_project/fraud_detection_engine` | **dev & docs** | edit, commit, push; `docs/sync-to-vault.sh` dijalankan dari sini |

Alasan: `docker-compose.yml` mem-bind-mount `./deploy/...` dan `./plugins`, sehingga edit di checkout yang sama
langsung memengaruhi container produksi. Dengan checkout terpisah, perubahan baru sampai ke produksi lewat `git pull`.

**Yang dijalankan pada 2026-10-21** (downtime ±4 menit):

```bash
OLD=~/own_project/apps/fraud_detection_engine
NEW=~/own_project/deployment/fraud_detection_engine
docker compose -p fraud-eval --env-file ~/fraud-eval/.env.eval -f $OLD/docker-compose.yml -f ~/fraud-eval/compose.eval.yml down
docker compose --project-directory $OLD -f $OLD/docker-compose.yml down        # tanpa -v: volume tetap
mkdir -p ~/own_project/deployment && mv $OLD $NEW
crontab -l | sed 's#own_project/apps/fraud_detection_engine#own_project/deployment/fraud_detection_engine#g' | crontab -
sed -i 's#own_project/apps/fraud_detection_engine#own_project/deployment/fraud_detection_engine#g' ~/fraud-eval/compose.eval.yml
git -C $NEW worktree repair ~/fraud-eval/repo
cd $NEW && rm -rf tools/*/.venv services/python/llm-service/.venv && for d in tools/simulator tools/research_seed tools/research_export; do uv sync --directory $d; done
docker compose up -d --wait --wait-timeout 900                                  # lalu fraud-eval seperti §0
systemctl --user daemon-reload && systemctl --user enable --now fraud-platform.service
```

Verifikasi: `docker compose ls` menunjuk ke `$NEW`, semua container fraud `healthy`,
`https://fds.hendrikarisma.my.id/login` 200, `:18090/ping` 200, `deploy/demo/master.sh info` membaca master.

**Autostart.** Unit `fraud-platform.service` (Type=oneshot, RemainAfterExit) menjalankan `docker compose up -d --wait`
untuk `fraud-platform`, lalu `up -d` untuk `fraud-eval`; `systemctl --user stop fraud-platform` menghentikan keduanya
(`stop`, bukan `down`). Butuh `loginctl enable-linger hendri` (sudah aktif). Ollama berjalan sebagai unit sistem
`ollama.service` (enabled).

```bash
systemctl --user status fraud-platform.service --no-pager
systemctl --user restart fraud-platform.service      # naikkan ulang kedua stack (idempotent)
journalctl --user -u fraud-platform.service -n 50
```

**Sisa pekerjaan:**
* Branch `eval/public-datasets` sudah di-push ke origin (2026-10-21); tinggal keputusan merge.
* Folder lain di `~/own_project/apps/` milik project lain; jangan dihapus.

**Rollback:** `systemctl --user stop fraud-platform`, `mv $NEW $OLD`, kembalikan path di crontab, `compose.eval.yml`
dan `WorkingDirectory`/path di unit, `git -C $OLD worktree repair ~/fraud-eval/repo`, buat ulang `.venv`, `up -d` dari `$OLD`.
