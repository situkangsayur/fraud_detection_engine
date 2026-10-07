# Panduan Eksperimen (riset & paper)

Panduan ini menjelaskan cara menyiapkan, menjalankan, dan **mengulang** eksperimen deteksi fraud di platform ini,
baik dengan data sintetis (simulator) maupun dataset publik dari internet. Hasilnya berupa dataset riset siap
analisis (Parquet/CSV + metrik + kamus data) untuk penulisan paper.

## 1. Konsep: satu platform, banyak tenant

| Tenant | Isi | Dipakai untuk |
|---|---|---|
| `demo` — *Demo Marketplace* | simulator sintetis, 4 project (checkout, post-payment, promo, returns), 60 hari | demo publik + eksperimen sintetis |
| `exp-sparkov` | Sparkov (transaksi kartu, AS) | eksperimen *card fraud* |
| `exp-paysim` | PaySim (mobile money) | eksperimen *account takeover → cash-out* |
| `exp-saml-d` | SAML-D (transfer AML, 28 tipologi) | eksperimen *money laundering / mule* |

Semua tenant ada di satu instalasi (compose project `fraud-platform`) dan memakai **protokol yang sama**:

```
history (70% pertama, load_only) + label tertunda 7 hari
   → latih & approve model (maker–checker): mlp_backprop + isolation_forest/hdbscan
   → online (30% sisanya, di-score oleh semua engine)
   → aktivitas analis (case, cluster, komunitas graph, blacklist, [LLM untuk demo])
   → ekspor dataset riset + ground truth
```

Seluruh tenant lalu disimpan sebagai **master snapshot**. Setiap pukul 00.00 WIB, platform di-restore dari master
dengan timestamp digeser ke "sekarang". Apa pun yang diubah pengunjung hilang, sedangkan hasil eksperimen tetap konsisten.

## 2. Prasyarat

* Stack berjalan (`docker compose up -d`, lihat [quickstart.md](quickstart.md)); `.env` berisi `ADMIN_PASSWORD`
  dan `DEMO_USER_PASSWORD`.
* `uv` (Python 3.12) dan ±20 GB disk kosong (dataset publik + snapshot).
* Dataset publik di `~/datasets/fraud-public/<nama>/`:

```bash
B=~/datasets/fraud-public; mkdir -p $B/{paysim,sparkov,saml-d}
curl -L -o $B/paysim/paysim.csv \
  https://huggingface.co/datasets/theman10/paysim/resolve/main/paysim.csv
curl -L -o $B/sparkov/credit_card_fraud_transactions.zip \
  https://huggingface.co/datasets/santosh3110/credit_card_fraud_transactions/resolve/main/credit_card_fraud_transactions.zip
curl -L -o $B/saml-d/SAML-D.zip \
  https://huggingface.co/datasets/LordNR/AMLGraphX-SAML-D/resolve/main/SAML-D.zip
```

Perlakukan folder dataset sebagai data tak tepercaya: hanya dibaca sebagai CSV/ZIP, jangan menjalankan apa pun dari sana.

## 3. Perintah

```bash
deploy/seed.sh demo                    # hapus semua data, seed tenant demo (±2 jam)
deploy/seed.sh research sparkov        # tambah/isi tenant exp-sparkov tanpa menghapus tenant lain (±30–60 menit)
deploy/seed.sh research                # ketiga dataset publik
deploy/seed.sh master                  # demo + semua dataset publik, lalu snapshot master (±4–5 jam)
deploy/seed.sh restore                 # kembalikan master (yang dijalankan cron 00.00)
deploy/demo/master.sh snapshot         # jadikan kondisi saat ini sebagai master baru
deploy/demo/master.sh info             # lihat master aktif (tenant, jumlah event, commit git)
deploy/demo/wipe-tenant.sh exp-paysim  # hapus SATU tenant eksperimen (tenant lain tidak tersentuh), lalu seed ulang
```

Ground truth (`ground_truth.jsonl`) ditulis **sebelum** seeding dimulai. Kalau run terputus, truth tetap ada; bisa juga
dibangkitkan ulang tanpa seeding: `uv run python -m research_seed --dataset sparkov --truth-only`.

Opsi mode research (langsung lewat tool):

```bash
cd tools/research_seed
uv run python -m research_seed --dataset saml-d --target 60000 --seed 42 --tenant exp-saml-d
```

| Opsi | Default | Arti |
|---|---|---|
| `--target` | 60000 | jumlah event yang disampel dari dataset |
| `--seed` | 42 | seed sampling (deterministik) |
| `--tenant` | `exp-<dataset>` | tenant tujuan |
| `--out` | `~/datasets/fraud-research` | folder hasil ekspor |

Jadwal cron (di nvda11-gpu dipasang setelah master pertama dibuat):

```cron
0 0 * * * /home/hendri/own_project/apps/fraud_detection_engine/deploy/seed.sh restore >> ~/.local/state/fraud-demo-reset.log 2>&1
```

## 4. Strategi sampling per dataset

| Dataset | Sampling | Ground truth | Catatan validitas |
|---|---|---|---|
| Sparkov | jendela waktu terakhir untuk **semua** kartu (histori per kartu tetap utuh) | `is_fraud` → `card_fraud` | mirror HF pernah lewat Excel: `cc_num` dibulatkan 6 digit dan detik hilang. Waktu memakai `unix_time` (presisi detik); identitas kartu dibangun ulang dari nomor terbulatkan + nama + tanggal lahir (910 kartu) |
| PaySim | semua tipe transaksi; fraud ±2% dari sampel | `isFraud` → `ato_transfer` / `ato_cash_out` | akun asal hampir selalu unik (tidak ada histori per customer); graph lewat rekening tujuan. Aktivitas PaySim menumpuk di awal timeline, dan batas history dihitung berdasarkan **waktu**, sehingga porsi online hanya ±15% event |
| SAML-D | per akun pengirim; ±30% baris dari akun yang melakukan pencucian uang (semua transaksinya disertakan) | `Laundering_type` (tipologi per transaksi) | fraud rate per event tetap rendah (±0,4%) karena pelaku juga bertransaksi normal |

Timeline setiap dataset digeser agar berakhir pada waktu seeding; jarak antar-event tidak berubah. Batas history/online
adalah 70% **rentang waktu** (bukan 70% jumlah event), sama seperti tenant demo.

## 5. Hasil

```
~/datasets/fraud-research/<tenant>/          # mode research
~/datasets/fraud-live-demo/<YYYY-MM-DD>/     # tenant demo (setiap seed demo)
   README.md            kamus data + cuplikan metrik + batasan
   manifest.json        jumlah baris + SHA-256 setiap file + parameter seed
   events.parquet       satu baris per event: skor tiap engine, keputusan, label, ground truth
   rule_hits.parquet    hit per rule      features.parquet  vektor fitur
   models/clusters/cases/labels/proposals/llm_reports/...   artefak lain
   metrics/             engine_auc, operating_points, typology_recall, threshold_sweep, dataset_summary
   ground_truth.jsonl   (research) label asli dataset per external_id
   source.json          (research) URL, lisensi, parameter sampling
```

Detail skema: [research-dataset.md](../technical/research-dataset.md).

## 6. Mengulang eksperimen

* **Ulang persis:** dataset dan sampling deterministik (`--seed`). Jalankan ulang `deploy/seed.sh research <dataset>`
  di tenant baru (`--tenant exp-<dataset>-r2`) atau di instalasi bersih. Model ML dilatih ulang, sehingga angka bisa
  sedikit berbeda karena inisialisasi bobot.
* **Replikasi untuk confidence interval:** jalankan dengan beberapa `--seed` (mis. 42, 43, 44) ke tenant berbeda, lalu
  hitung rata-rata ± CI dari `metrics/*.csv`. Untuk tenant demo, `deploy/seed.sh demo` memakai seed tanggal.
* **Bandingkan konfigurasi:** ubah bobot engine/threshold di Settings project, lalu ekspor ulang dengan
  `tools/research_export` ke folder lain.

## 7. Lisensi dataset

| Dataset | Lisensi | Konsekuensi |
|---|---|---|
| PaySim (`theman10/paysim`) | MIT | bebas |
| Sparkov (`santosh3110/…`) | Apache-2.0 (tag HF); asli Kaggle *kartik2112* | sebutkan sumber |
| SAML-D (`LordNR/AMLGraphX-SAML-D`) | CC BY-NC-SA 4.0 | non-komersial, *share-alike*; aman untuk paper, sebutkan sumber |

Sitasi: PaySim (Lopez-Rojas, Elmir & Axelsson, EMSS 2016), SAML-D (Oztas et al., IEEE ICEBE 2023), Sparkov
(B. Harris, *Sparkov Data Generation*, github.com/namebrandon/Sparkov_Data_Generation; dataset Kaggle oleh kartik2112).
