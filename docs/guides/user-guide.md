# Panduan Pengguna — per engine

Untuk analis fraud, approver, dan admin project. Semua menu ada di Web UI. Menu yang tampil tergantung peran Anda.
Nama menu di bawah memakai label UI bahasa Indonesia, dengan label English dalam tanda kurung (ganti bahasa lewat
tombol **ID/EN** di halaman login atau menu user).

### Navigasi (sidebar)

Pilih project dengan **pemilih project** di bagian atas sidebar, atau dari halaman **Proyek** (*Projects*). Menu project:

| Bagian | Menu |
|---|---|
| Ringkasan (*Overview*) | **Dashboard**, **Event & transaksi** (*Events*), **Kasus** (*Cases*) |
| Rule engine | **Rule** (*Rules*), **Ruleset** (*Rulesets*), **Reference list** (*Reference lists*), **Formula playground**, **Usulan rule** (*Rule proposals*) |
| Machine learning | **ML supervised** (*Supervised ML*), **ML unsupervised** (*Unsupervised ML*), **Algoritma (plugin)** (*Algorithms (plugins)*) |
| Graph | **Graph explorer** |
| Asisten LLM (*LLM assistant*) | **Chat LLM** (*LLM chat*), **Analisis & laporan** (*Analyses & reports*), **Regulasi proyek** (*Project regulations*) |
| Data | **Sumber data** (*Data sources*), **Katalog field** (*Field catalog*) |
| Proyek (*Project*) | **Pengaturan** (*Settings*), **Anggota** (*Members*, project admin), **Audit log** (approver ke atas) |

Menu tingkat tenant (di bawah nama tenant): **Proyek**, **Pengguna tenant** (*Tenant users*) dan **Daftar tingkat
tenant** (*Tenant-wide lists*) untuk tenant admin, **Pustaka regulasi** (*Regulation library*), serta **Tenant**
(*Tenants*) khusus platform admin.

> Demo publik: akun demo dan eksperimen tercantum di halaman login. Semua perubahan hilang saat restore pukul 00.00 WIB,
> dan ID project bisa berubah (bookmark lama diarahkan ke daftar project).

## 0. Konsep dasar

* **Tenant** = perusahaan Anda. **Project** = satu titik perlindungan (mis. *Checkout*, *Retur*). Pilih project
  lewat pemilih project di header.
* Setiap **event** (transaksi, login, perubahan akun, klaim promo, refund, …) dinilai oleh 5 engine dan menghasilkan
  **approve / review / decline** beserta **alasan**. Keputusan review/decline otomatis membuka **case**.
* **Maker–checker:** perubahan penting (rule, ruleset, model, usulan AI) dibuat oleh analyst dan harus disetujui oleh
  approver lain. Anda tidak bisa menyetujui perubahan buatan Anda sendiri.

| Peran | Hak |
|---|---|
| Viewer | melihat |
| Analyst | + membuat/mengubah draft, melatih model, menangani case, memberi label, menjalankan analisis AI |
| Approver | + menyetujui/menolak, melihat audit log |
| Project admin | + anggota project, settings, data source |
| Tenant admin | + user, project, library regulasi tenant |

---

## 1. Dashboard & investigasi

* **Dashboard:** volume event, tren keputusan, distribusi skor, rata-rata skor per engine, breakdown tipologi, case
  terbuka, dan tabel *drift* fitur (PSI; "significant" berarti pola data berubah).
* **Event & transaksi:** filter berdasarkan tipe, keputusan, skor, waktu, dan pelanggan (cari ID transaksi/pelanggan).
  Halaman detail punya tab *Keputusan*, *Trace rule*, *Event kanonik*, *Data sumber*, *Fitur*, dan *Label*:
  * field kanonik, data mentah dari sumber (`source.*`), dan fitur;
  * skor per engine dan alasan (reason code) yang kontribusinya dijumlahkan menjadi skor akhir;
  * **jejak rule lengkap**: setiap rule dengan hasil match / no_match / **trapped** beserta alasan trapped
    (mis. data kosong, histori kurang);
  * tombol *Skor ulang* (*Rescore*), *Buka di graph*, dan *Tambah label*.
* **Pelanggan:** halaman detail pelanggan (dibuka dari event/kasus): profil, label risiko, dan riwayat event.
* **Kasus:** antrean review. *Ambil kasus* / tugaskan, *Mulai review*, beri catatan, lalu **Selesaikan** (*Resolve*)
  sebagai *fraud* (pilih tipologi) atau *legit*. Centang *Terapkan ke pelanggan* (*apply to customer*) untuk menandai
  pelanggannya sebagai fraud, yang langsung memengaruhi engine graph.
  Label dari case adalah bahan belajar model ML.

## 2. Rule engine

Bagian **Rule engine** di sidebar berisi **Rule**, **Ruleset**, **Reference list**, **Formula playground**, dan
**Usulan rule**. Rule baru bisa dibuat kosong atau dari template stage.

### 2.1 Jenis rule

| Jenis | Kapan dipakai | Contoh |
|---|---|---|
| **Simple** | membandingkan field dengan nilai **atau dengan field lain**, bisa beberapa kondisi (ALL/ANY/NOT/minimal N) | `amount > 5.000.000` DAN `issuer_country ≠ geo_country` |
| **Velocity** | agregasi histori per kelompok dalam jendela waktu, dibandingkan dengan nilai, field, atau formula | kartu yang sama dipakai ≥ 3 pelanggan berbeda dalam 30 hari |
| **Velocity statistik** | membandingkan nilai sekarang dengan distribusi historisnya | z-score nominal > 3; peluang gaussian < 1%; tren naik (regresi linear); lonjakan jumlah (poisson) |
| **Composite** | velocity, tapi histori **difilter** dulu dengan kondisi simple | ≥ 3 pelanggan berbeda di device yang sama memakai **kode promo yang sama** dalam 7 hari |
| **Reference** | cek ke daftar (blacklist/whitelist/watchlist/lookup) | kartu ada di `card_blacklist`, atau nominal > `max_amount` merchant di daftar lookup |
| **Graph** | hubungan dengan pelanggan fraud | jarak ke pelanggan fraud ≤ 2 langkah |

### 2.2 Formula

Di mana pun ada nilai, Anda bisa memakai formula, misalnya `F(x,y,z) = 2x + 2^y / z^2`, dan mengikat `x`, `y`, `z`
ke field atau konstanta. Coba dulu di **Formula playground**. Posisi kesalahan penulisan akan ditunjukkan.

### 2.3 Hasil "trapped"

Rule menjadi **trapped** jika tidak bisa dievaluasi, misalnya field kosong, pembagian dengan nol, histori terlalu
sedikit untuk statistik, atau reference list tidak ada. Pengaturan per rule:
* `ignore`: hanya dicatat;
* `score`: tambahkan *trapped score*;
* `review`: paksa event masuk review.

Centang *missing as no match* jika field kosong seharusnya dianggap "tidak kena".

### 2.4 Siklus hidup

```
Draft → (Submit) → Pending approval → (Approve oleh orang lain) → Active atau Shadow → Retired
```

* **Uji** (*Test*): jalankan rule terhadap satu event (ID event atau konteks yang ditempel).
* **Backtest:** lihat hit rate, precision, dan recall terhadap label pada data historis sebelum mengaktifkan.
* **Shadow:** rule/ruleset berjalan pada lalu lintas nyata tanpa memengaruhi keputusan. Bandingkan skornya dengan
  versi live (champion vs challenger).
* Mengedit rule yang sedang aktif membuat **versi baru**. Versi lama tetap berjalan sampai versi baru disetujui.

### 2.5 Ruleset

Kumpulan rule untuk satu tujuan (mis. "Carding"), dengan bobot per rule dan cara agregasi (`sum`, `max`,
`probabilistic_or`, `weighted_average`). Ruleset juga ber-versi dan harus disetujui.

### 2.6 Reference list

Dibuat kapan saja, per project (**Reference list**) atau **tenant-wide** (**Daftar tingkat tenant**, dipakai semua
project). Isi manual atau **import CSV** (kolom
pertama = key, kolom lain = atribut). Entri bisa diberi masa berlaku.

## 3. ML Supervised

Menu **ML supervised** (*Supervised ML*).

1. **Latih model** (*Train*): pilih algoritma (form parameter muncul otomatis dari plugin), atau pakai default project.
   * *Label maturity* (default 14 hari): event lebih tua dari N hari tanpa laporan fraud dianggap legit.
2. Tunggu status **ready**, lalu periksa kurva ROC/PR, confusion matrix, kalibrasi, feature importance, dan riwayat
   loss.
3. **Ajukan persetujuan** (*Submit*), lalu approver menekan **Setujui** (*Approve*). Model menjadi **active**; model lama diarsipkan. Pergantian terjadi
   tanpa downtime.

Tips: di awal, ketika label masih sedikit, andalkan rule dan unsupervised. Model supervised membaik seiring
bertambahnya case yang di-resolve.

## 4. ML Unsupervised

Menu **ML unsupervised** (*Unsupervised ML*).

* **Latih model** (*Train*): pilih algoritma anomaly (isolation forest / LOF / autoencoder) dan clustering (HDBSCAN / k-means /
  DBSCAN / GMM).
* **Cluster:** ukuran, fraud rate (dari label), profil, dan fitur pembeda. Beri nama cluster (mis. "peternakan
  promo").
* **Proyeksi 2-D (PCA)** (*Projection*): peta 2-D event berwarna per cluster atau skor anomali. Klik titik untuk membuka event.
* **Anomali teratas** (*Top anomalies*): daftar event paling aneh, sebagai kandidat investigasi atau bahan rule baru.
* **Komunitas graph** (*Graph communities*): kelompok pelanggan yang saling terhubung beserta fraud rate-nya (dipakai rule graph
  `community_fraud_rate`).

Menu **Algoritma (plugin)** menampilkan semua algoritma yang tersedia beserta parameternya. Plugin baru di folder
`plugins/` dimuat dengan tombol **Muat ulang plugin** (platform admin).

## 5. Graph

Menu **Graph explorer**.

* Cari pelanggan (ID eksternal) atau entitas (email/HP/device/IP).
* **Neighbourhood explorer:** node pelanggan dan entitas yang dipakai bersama. Pelanggan fraud disorot. Filter per
  jenis link, sertakan link "mirip", atur kedalaman, dan klik node untuk memperluas.
* **Kedekatan ke fraud** (*Fraud proximity*): jalur terpendek ke pelanggan fraud terdekat.
* **Komponen terhubung** (*Components*): kelompok terhubung dengan fraud rate tertinggi, sering berupa jaringan promo farm atau mule.
* Entitas yang terlalu umum (mis. IP Wi-Fi publik) otomatis diabaikan (*supernode*).

## 6. LLM assistant

Bagian **Asisten LLM** (*LLM assistant*) di sidebar, ditambah **Pustaka regulasi** di menu tenant.

* **Pustaka regulasi** (*Regulation library*, tenant admin/analyst): unggah PDF/DOCX regulasi OJK/BI/SOP internal dengan kode, versi, dan
  tanggal berlaku. Versi baru sebuah regulasi dibandingkan **pasal demi pasal** dengan versi sebelumnya.
* **Regulasi proyek** (*Project regulations*): pilih dokumen yang berlaku untuk project ini.
* **Chat LLM:** tanya jawab tentang regulasi, rule, dan data. Jawaban menyertakan sitasi pasal dan alat yang dipakai.
* **Analisis & laporan** (*Analyses & reports*), tombol **Jalankan analisis**:
  * *Situasi fraud & anomali*: narasi tren, tipologi, cluster anomali, komunitas graph;
  * *Relevansi rule*: rule mana yang masih relevan, perlu disetel, atau sebaiknya dipensiunkan;
  * *Dampak regulasi*: gap antara regulasi (baru) dan rule saat ini;
  * *Rekomendasi rule*: usulan rule baru berdasarkan pola data dan regulasi.
* **Usulan rule** (*Rule proposals*): usulan AI (dan analyst) lengkap dengan validasi, backtest 30 hari, dan sitasi. Approver menyetujui
  → rule masuk mode **shadow** → setelah terbukti, disetujui lagi untuk **active**. AI tidak pernah mengaktifkan
  rule sendiri.

> LLM berjalan di Ollama lokal (demo: qwen3:8b). Tanpa GPU, laporan analisis bisa memakan waktu lama (puluhan
> menit); status laporan tetap *LLM sedang menyusun laporan…* sampai selesai.

## 7. Data source (pakai data Anda sendiri)

Bagian **Data** di sidebar: **Sumber data** dan **Katalog field**.

1. **Sumber data baru:** webhook (API key ditampilkan **sekali**), file, atau database Postgres/MySQL (password
   disimpan sebagai nama variabel environment, bukan nilai).
2. **Baca struktur** (*Inspect*): unggah contoh file / tempel JSON / pilih tabel. Sistem membaca struktur, mendeteksi PII, dan
   menyarankan mapping (dengan tingkat keyakinan).
3. **Mapping editor:** cocokkan kolom sumber ke field standar dan pilih transformasi (format tanggal, angka format
   Indonesia, hash nomor kartu, normalisasi HP, …). Kolom label (mis. `is_fraud`) bisa dipetakan untuk training.
4. **Pratinjau → Simpan & aktifkan.** Semua kolom sumber langsung tersedia di rule sebagai `source.<kolom>`.
5. **Mulai import** (tab *Import job*) untuk file/tabel. Mode `load_only` hanya membangun histori, sedangkan `score`
   juga menilai setiap event. Baris yang gagal masuk ke tab *Error ingest*.
6. **Katalog field:** aktifkan *velocity enabled* untuk kolom sumber yang ingin dipakai di agregasi velocity.

## 8. Pengaturan project (project admin)

* **Ambang keputusan** (review/decline; default review ≥ 50, decline ≥ 80). Kalibrasi per project dengan melihat *Evaluasi* dan hasil backtest.
  Tahap berbeda (pre-payment vs retur) biasanya butuh threshold berbeda.
* **Bobot engine** dan **metode kombinasi**:
  * `noisy_or` (default): bukti dari beberapa engine saling menguatkan, dan engine yang "diam" tidak melemahkan
    sinyal kuat;
  * `weighted_average`: rata-rata berbobot.
* **Skor graph** per jarak, **timeout** engine, dan **keputusan saat rule-service tidak tersedia**.
* **Buat kasus otomatis untuk** keputusan tertentu; selalu satu kasus terbuka per pelanggan.
* Menu terpisah: **Anggota** project dan perannya (project admin), **Audit log** (approver ke atas).
