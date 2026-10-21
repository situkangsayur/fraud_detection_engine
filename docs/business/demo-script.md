# Skenario Demo — Presentasi Langsung (15–20 menit)

Untuk presenter. Skenario ini memakai demo publik <https://fds.hendrikarisma.my.id> dan tidak butuh instalasi apa
pun. Ringkasan produk untuk pembuka ada di [overview.md](overview.md); istilah di [glossary.md](glossary.md).

## 1. Persiapan

### 1.1 Akun demo

Akun ada di **halaman login**, di kotak *"Akun demo & eksperimen"*. Pilih tenant, lalu klik **Pakai** untuk mengisi
form otomatis. Jangan menyalin password ke slide atau dokumen; cukup arahkan audiens ke halaman login.

| Tenant | Akun yang dipakai di skenario | Peran |
|---|---|---|
| Demo Marketplace | `analyst@demo.local` | analyst (maker) di 4 project |
| Demo Marketplace | `approver@demo.local` | approver (checker) di 4 project |
| Demo Marketplace | `sim-admin@demo.local` | tenant admin (pustaka regulasi, pengguna) |
| Experiment — Sparkov / PaySim / SAML-D | `analyst@…`, `approver@…`, `sim-admin@…` | sama, untuk dataset publik |

Tip: buka **dua jendela browser terpisah** (mis. satu jendela biasa dan satu jendela *incognito*/profil lain), satu
login sebagai analyst dan satu sebagai approver. Maker–checker jadi bisa ditunjukkan tanpa logout-login.

### 1.2 Isi demo

| Tenant | Project | Stage | Tipologi yang disimulasikan |
|---|---|---|---|
| **Demo Marketplace** | Checkout Protection | `pre_payment` | carding, account takeover, system breach |
| | Post-payment & Payout | `post_payment` | bank account takeover, money mule |
| | Returns & Refunds | `returns` | refund abuse |
| | Promo & Cashback | `promo` | promo abuse (peternakan akun) |
| Experiment — Sparkov | 1 project | — | card fraud (dataset publik) |
| Experiment — PaySim | 1 project | — | mobile money (dataset publik) |
| Experiment — SAML-D | 1 project | — | pencucian uang (dataset publik) |

Demo Marketplace berisi ±135 ribu event selama 60 hari yang berakhir "hari ini". Setiap project sudah punya model
supervised (`mlp_backprop`) dan unsupervised (`isolation_forest` + `hdbscan`) yang aktif, regulasi POJK 12/2024 yang
terlampir, case yang sebagian sudah diselesaikan, cluster yang sudah diberi nama, serta contoh laporan dan usulan dari
asisten AI.

### 1.3 Reset harian 00.00 WIB

Setiap hari pukul **00.00 WIB** demo dikembalikan ke snapshot awal (`deploy/seed.sh restore`), lalu semua waktu
digeser sehingga data berakhir "hari ini". Semua perubahan pengunjung (rule, case, model, upload) hilang.

* Proses memakan **±3 menit**; selama itu demo tidak bisa diakses. **Jangan jadwalkan demo antara 23.50 dan 00.10 WIB.**
* Karena akun demo dipakai bersama, pengunjung lain bisa saja sudah mengubah sesuatu hari itu. Cek alur sekali
  sebelum presentasi.

### 1.4 Checklist 15 menit sebelum mulai

- [ ] Login sebagai analyst dan approver di dua jendela; pilih project **Checkout Protection**.
- [ ] **Panaskan LLM:** buka *Asisten LLM → Chat LLM* dan kirim satu pertanyaan pendek. Model berjalan di GPU
      (jawaban dalam hitungan detik); pertanyaan pertama setelah lama diam sedikit lebih lambat karena model dimuat ke memori.
- [ ] Buka *Asisten LLM → Analisis & laporan* dan pastikan ada laporan yang sudah selesai (cadangan jika LLM lambat).
- [ ] Buka *Usulan rule* dan catat satu usulan yang masih pending (cadangan untuk langkah maker–checker).
- [ ] Atur bahasa (tombol ID/EN di halaman login) dan mode terang agar terbaca di proyektor.

## 2. Alur cerita (±18 menit)

Benang merah: *"Satu transaksi mencurigakan → kenapa ditahan → siapa jaringannya → bagaimana mesin belajar →
bagaimana AI membantu memperbaiki aturan tanpa melangkahi manusia."*

| # | Menit | Bagian | Login sebagai | Project |
|---|---|---|---|---|
| 1 | 0–1 | Login & konteks | analyst | — |
| 2 | 1–3 | Dashboard | analyst | Checkout |
| 3 | 3–6 | Detail event & reason code | analyst | Checkout |
| 4 | 6–8 | Case | analyst | Checkout |
| 5 | 8–10 | Graph | analyst | Promo |
| 6 | 10–12 | Machine learning | analyst | Checkout |
| 7 | 12–14 | Rule, backtest, shadow | analyst → approver | Checkout |
| 8 | 14–18 | Asisten LLM → usulan → maker–checker | analyst → approver | Checkout / Promo |
| 9 | 18–20 | Penutup & tanya jawab | — | — |

### Langkah 1 — Login & konteks (1 menit)

1. Buka halaman login. Tunjukkan kotak akun demo: *"Ada empat tenant: satu marketplace contoh dan tiga dataset publik
   untuk riset."*
2. Klik **Pakai** pada `analyst@demo.local`, lalu **Masuk**.
3. Tunjukkan daftar project: *"Satu perusahaan, empat titik perlindungan. Tiap project punya rule, model, dan
   ambang sendiri."*

### Langkah 2 — Dashboard (2 menit)

Project: **Checkout Protection** → menu **Dashboard**.

* Volume event dan tren keputusan approve/review/decline.
* Distribusi skor dan rata-rata skor per mesin: *"Lima mesin, satu keputusan."*
* Breakdown tipologi dan jumlah case terbuka.
* Tabel drift fitur: *"Kalau pola data berubah, kita tahu sebelum model menurun."*

### Langkah 3 — Detail event & reason code (3 menit)

Menu **Event & transaksi** → filter keputusan *decline* atau skor minimal 80 → buka satu event.

* Tab **Keputusan**: skor tiap mesin, skor akhir, dan daftar alasan beserta kontribusinya. Pesan kunci: *"Setiap
  keputusan bisa dijelaskan ke pelanggan, auditor, dan regulator."*
* Tab **Trace rule**: semua rule yang dievaluasi, hasil *match / no_match / trapped*, dan versi rule. Jelaskan
  *trapped* dalam satu kalimat (rule tidak bisa dievaluasi, mis. data kosong) dan bahwa itu tidak diam-diam diabaikan.
* Tab **Data sumber**: data asli dari sistem klien, bisa dipakai di rule sebagai `source.*`. Tunjukkan bahwa nomor
  kartu tampil tersamar.
* Opsional: klik **Skor ulang** untuk menunjukkan penilaian ulang dengan rule dan model saat ini.

### Langkah 4 — Case (2 menit)

Dari event tadi klik **Buka kasus**, atau menu **Kasus**.

1. **Ambil kasus**, tambahkan catatan investigasi singkat.
2. Tunjukkan opsi **Selesaikan** sebagai fraud (pilih tipologi) atau legit, dan centang **Terapkan ke pelanggan**.
3. Pesan kunci: *"Label dari analis adalah bahan belajar model ML, dan menandai pelanggan fraud langsung memengaruhi
   mesin graph."*

Tidak perlu benar-benar menyelesaikan case jika ingin menjaga data tetap rapi untuk presentasi berikutnya.

### Langkah 5 — Graph (2 menit)

Ganti project ke **Promo & Cashback** → menu **Graph explorer**.

1. Lihat daftar komponen dengan fraud rate tertinggi, atau buka graph dari sebuah case promo (**Buka di graph**).
2. Tunjukkan pelanggan yang berbagi perangkat, alamat, atau nomor HP. Aktifkan **Termasuk entitas mirip** untuk
   memperlihatkan nomor HP yang hanya beda satu digit.
3. **Kedekatan ke fraud**: jalur terpendek ke pelanggan fraud terdekat.
4. Pesan kunci: *"Setiap transaksi promo sah secara teknis. Yang membongkar adalah jaringannya."*
5. Sebut *supernode*: IP Wi-Fi publik otomatis diabaikan agar orang yang tidak berkaitan tidak ikut tersambung.

### Langkah 6 — Machine learning (2 menit)

Kembali ke **Checkout Protection**.

* **ML supervised**: model `mlp_backprop` yang aktif, kurva ROC/PR, confusion matrix, dan feature importance. Sebut
  bahwa model ini dilatih analyst dan disetujui approver (maker–checker berlaku juga untuk model).
* **ML unsupervised**: cluster dengan fraud rate tertinggi (sudah diberi nama oleh analis) dan peta proyeksi 2-D. Klik
  satu titik untuk membuka event. Pesan kunci: *"Mesin ini menemukan pola yang belum punya label."*
* Angka untuk disebut (data uji 7 Oktober 2026): AUC gabungan checkout **0,999**.

### Langkah 7 — Rule, backtest, dan shadow (2 menit)

Menu **Rule**.

1. Buka rule template, misalnya `RL-PRE-CARD-TESTING` (≥ 4 kartu berbeda dari satu perangkat dalam 1 jam). Tunjukkan
   jenis rule (velocity), versi, dan statistik hit 7 hari.
2. Klik **Backtest**: berapa event kena, precision, dan recall pada data historis.
3. Tunjukkan rule berstatus **Shadow** (contohnya rule hasil usulan AI yang sudah disetujui): *"Berjalan di lalu lintas
   nyata, dicatat, tapi tidak memengaruhi keputusan sampai disetujui lagi."*
4. Opsional, untuk maker–checker: sebagai analyst ubah sedikit sebuah rule → **Ajukan persetujuan**. Tunjukkan pesan
   *"Approver harus orang lain"*. Pindah ke jendela approver → **Setujui** dengan target **Shadow**.

### Langkah 8 — Asisten LLM: regulasi, rekomendasi, usulan, maker–checker (4 menit)

Bagian ini paling berisiko lambat. Jalankan langkah 8b di awal (sebelum langkah 7) jika ingin hasilnya siap tepat waktu.

**8a. Pertanyaan regulasi dengan sitasi** — menu **Asisten LLM → Chat LLM**.

1. Tunjukkan **Regulasi proyek**: POJK 12/2024 sudah terlampir.
2. Tanyakan misalnya: *"Menurut POJK 12/2024, apa saja pilar strategi anti fraud dan apa kewajiban terkait deteksi
   fraud?"*
3. Saat jawaban mengalir, tunjukkan daftar **alat yang dipakai** dan **Rujukan regulasi** (sitasi pasal).
4. Pesan kunci: *"Jawaban selalu menyebut pasal sumbernya, dan kutipan tetap harus diperiksa manusia. Model kecil bisa
   salah menyebut nomor pasal."*

**8b. Rekomendasi rule → usulan** — menu **Asisten LLM → Analisis & laporan**.

1. **Jalankan analisis** → pilih **Rekomendasi rule**. Laporan berjalan di latar belakang (*"LLM sedang menyusun
   laporan…"*).
2. Sambil menunggu, jelaskan alurnya: AI membaca statistik, anomali, graph, dan regulasi → menyusun draft rule →
   draft divalidasi (jika tidak sah, AI diminta memperbaiki) → di-backtest 30 hari → menjadi **usulan pending**.
3. Alternatif di chat (project **Promo & Cashback**): klik saran *"Rekomendasikan rule baru untuk abuse voucher"*.

**8c. Maker–checker pada usulan AI** — menu **Usulan rule**.

1. Buka usulan terbaru (atau usulan cadangan dari checklist). Tunjukkan **Alasan**, **Rule yang diusulkan**,
   **Validasi**, **Bukti backtest (30 hari)**, dan sitasi.
2. Sebagai analyst, tombol setujui tidak tersedia. Pindah ke jendela **approver** → **Setujui**.
3. Tunjukkan bahwa rule hasilnya berstatus **Shadow**, bukan aktif.
4. Pesan kunci: *"AI tidak pernah mengaktifkan apa pun. Ada dua persetujuan manusia sebelum usulan AI memengaruhi
   keputusan terhadap pelanggan."*

Opsional: tunjukkan **Audit log** (approver) yang mencatat persetujuan tadi beserta nilai sebelum/sesudah.

### Langkah 9 — Penutup (2 menit)

Tiga angka untuk diingat (lihat [overview.md](overview.md#0-ringkasan-eksekutif)):

1. **Ranking sangat baik**: AUC gabungan 0,957–1,000 di demo, 0,86–0,98 di dataset publik.
2. **Ambang harus dikalibrasi per project**: pada ambang default 50, FPR checkout 23% dan promo 36%; setelah
   dikalibrasi (FPR ≤ 5%) fraud tetap tertangkap ~100%.
3. **Kelemahan yang diakui**: money mule hanya 35% tertangkap pada ambang decline 80. Perbaikannya sudah di backlog.

Tutup dengan rencana multi-agent (§4) jika audiens tertarik pada AI.

## 3. Yang bisa salah, dan cara mengatasinya

| Gejala | Penyebab | Yang dilakukan |
|---|---|---|
| Jawaban chat sangat lambat (1–5 menit) atau muncul pesan kesalahan | LLM `qwen3:8b` berjalan di **CPU** (GPU server belum aktif); pertanyaan pertama juga harus memuat model | Panaskan LLM sebelum mulai; pakai pertanyaan pendek; sambil menunggu, lanjutkan ke langkah lain lalu kembali. Klik **Hentikan** jika terlalu lama. |
| Laporan analisis lama di status "sedang menyusun" | Analisis butuh beberapa panggilan LLM | Jalankan lebih awal; tunjukkan laporan dan usulan yang sudah ada dari reset harian. |
| Jawaban menyebut nomor pasal yang salah | Model kecil bisa berhalusinasi | Jadikan poin presentasi: itulah alasan sitasi ditampilkan dan usulan wajib disetujui manusia. |
| Usulan AI berstatus *invalid* atau tidak ada usulan baru | Draft rule dari LLM tidak lolos validasi walau sudah diperbaiki | Tunjukkan bahwa validasi menahan draft yang salah; pakai usulan cadangan. |
| Halaman project "tidak ditemukan" atau sesi habis | Link lama dari sebelum reset harian, atau sesi kedaluwarsa | Kembali ke daftar project atau login ulang. |
| Demo tidak bisa diakses sekitar tengah malam | Reset harian 00.00 WIB (±3 menit) | Tunggu beberapa menit; hindari jadwal demo di jam ini. |
| Data berbeda dari latihan, mis. rule sudah diubah | Akun demo dipakai bersama pengunjung lain | Cek alur sebelum presentasi; perubahan hilang di reset berikutnya. |
| Skor graph 0 pada event tertentu | Pelanggan tidak berbagi entitas dengan pelanggan fraud terkonfirmasi | Pilih contoh dari project Promo atau komponen dengan fraud rate tinggi. |
| Tenant eksperimen menunjukkan kontribusi rule kecil | Template rule dibuat untuk marketplace Indonesia, bukan skema dataset publik | Jelaskan: di dataset ini model ML yang dominan; ini temuan riset, bukan bug. |

## 4. Rencana: demo multi-agent

Asisten saat ini adalah **satu agen** dengan 20 alat (termasuk mencari dan membuka pelanggan, transaksi, dan kasus). Versi berikutnya memakai **beberapa agen** dengan tugas
terpisah: agen regulasi, analitik, pola, perancang rule, simulator, dan *Critic* yang memeriksa sitasi dan dampak
sebelum usulan sampai ke approver. Skenario demo yang direncanakan: *"Regulation-to-Rule"*, yaitu dari regulasi yang
terlampir menjadi usulan rule yang tersitasi, tervalidasi, ter-backtest, dan sudah dikritik, dengan progres tiap agen
terlihat di layar. Fitur ini **belum tersedia di demo publik**. Desain dan statusnya ada di
[multi-agent.md](../technical/multi-agent.md).
