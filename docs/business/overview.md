# Fraud Detection Platform — Ringkasan Non-Teknis

Dokumen ini untuk manajemen, tim risk/fraud, compliance, dan bisnis. Tidak perlu latar belakang teknis.

## 1. Masalah yang diselesaikan

Fraud pada transaksi online datang dalam banyak bentuk:

| Jenis | Contoh kejadian |
|---|---|
| **Carding / kartu curian** | Penipu menguji puluhan kartu curian dengan transaksi kecil, lalu belanja besar dengan kartu yang lolos. |
| **Account takeover (ATO)** | Akun pelanggan e-commerce dibobol: login dari perangkat baru, password diganti, alamat pengiriman diubah, lalu belanja barang mahal. |
| **Pengambilalihan rekening / e-wallet** | Rekening korban dikuras ke rekening tujuan baru, sering tengah malam. |
| **Sistem dibobol** | Celah di sistem/API dipakai untuk membuat transaksi massal dari banyak akun sekaligus. |
| **Abuse kebijakan (promo, voucher, cashback)** | Semua transaksinya *sah secara teknis*, tetapi satu orang membuat puluhan akun (perangkat, alamat, atau nomor HP yang mirip) untuk memanen voucher atau cashback. |
| **Abuse retur/refund** | Pelanggan yang berulang kali mengklaim "barang tidak sampai" atau meretur barang segera setelah diterima. |
| **Rekening penampung (money mule)** | Akun yang dipakai untuk menampung dan meneruskan dana hasil kejahatan. |

Tidak ada satu metode yang bisa menangkap semua jenis ini. Karena itu platform ini menggabungkan **lima "mesin"
deteksi** yang saling melengkapi.

## 2. Lima mesin deteksi

| Mesin | Analogi | Kekuatan |
|---|---|---|
| **Rule engine** | Checklist dari analis berpengalaman | Cepat, transparan, dan mudah diaudit. Cocok untuk pola yang sudah diketahui dan untuk kewajiban regulasi. |
| **Machine learning terawasi** (*supervised*) | Analis yang belajar dari ribuan kasus fraud masa lalu | Menangkap kombinasi sinyal halus yang sulit ditulis sebagai aturan. |
| **Machine learning tak terawasi** (*unsupervised*) | Detektor "ada yang aneh" | Menemukan pola **baru** yang belum pernah diberi label, dan mengelompokkan perilaku serupa. |
| **Graph (jaringan)** | Peta hubungan antar pelanggan | Mengungkap jaringan: akun yang berbagi kartu, perangkat, alamat, atau nomor HP (termasuk yang *mirip*). Misalnya, pelanggan yang hanya berjarak 2 langkah dari penipu yang sudah terbukti dianggap berisiko tinggi. |
| **Asisten AI (LLM)** | Konsultan kepatuhan dan analis data | Membaca regulasi (OJK, BI, SOP internal), menilai apakah aturan yang ada masih relevan, menjelaskan kondisi fraud terkini, dan **mengusulkan** aturan baru. Asisten ini tidak pernah mengaktifkan apa pun sendiri. |

Setiap transaksi atau aktivitas dinilai oleh semua mesin dalam waktu sepersekian detik. Hasil akhirnya satu keputusan:

* ✅ **Approve**: lanjutkan.
* 🔎 **Review**: tahan dan periksa manual (otomatis masuk antrean *case*).
* ⛔ **Decline**: tolak.

Setiap keputusan disertai **alasan yang bisa dibaca manusia**, misalnya: "perangkat baru + password diganti dalam 24 jam",
"kartu dipakai 4 pelanggan berbeda dalam 30 hari", atau "berjarak 2 langkah dari akun fraud". Karena itu keputusan bisa
dipertanggungjawabkan ke pelanggan, auditor, maupun regulator.

## 3. Tenant dan project: satu perusahaan, banyak titik perlindungan

Platform ini **multi-tenant**: satu instalasi bisa melayani banyak perusahaan (tenant), dan data antar perusahaan
terisolasi total.

Di dalam satu perusahaan ada banyak **project**, masing-masing untuk satu titik perlindungan dengan perilaku dan aturan
yang berbeda. Contoh untuk sebuah marketplace:

| Project | Kapan dinilai | Contoh fokus |
|---|---|---|
| Checkout (*pre-payment*) | sebelum pembayaran diproses | carding, ATO saat checkout |
| Pasca pembayaran (*post-payment*) | setelah bayar, sebelum barang dikirim | alamat mencurigakan, nominal tidak wajar |
| Retur (*returns*) | saat permintaan retur/refund | pelaku retur berulang, akun-akun yang saling terkait |
| Promo | saat klaim voucher/cashback | "peternakan" akun promo |
| Keamanan akun | saat login atau ganti data akun | percobaan login gagal, perangkat baru |

Setiap project punya **aturan, model AI, data jaringan (graph), sumber data, dan daftar regulasi** sendiri. Daftar hitam
perusahaan (misalnya kartu yang pernah chargeback) bisa dibagikan ke semua project.

## 4. Pakai data Anda apa adanya

Perusahaan tidak perlu mengubah format datanya. Unggah contoh file (CSV, Excel, JSON), sambungkan tabel database, atau
kirim data lewat webhook. Sistem lalu:

1. membaca struktur data secara otomatis, termasuk nama kolom berbahasa Indonesia seperti `tgl_transaksi`, `nominal`,
   atau `no_hp`;
2. menyarankan pemetaan ke kolom standar, yang cukup dikonfirmasi oleh pengguna;
3. mengamankan data sensitif secara otomatis: nomor kartu dan rekening diubah menjadi sidik jari (*hash*) dan tidak
   disimpan mentah;
4. membuat **semua kolom** langsung bisa dipakai di aturan dan model, tanpa perlu mengubah kode.

Dataset historis yang sudah berlabel (misalnya kolom `is_fraud`) bisa langsung dipakai untuk melatih model AI.

## 5. Tata kelola dan kepatuhan

* **Empat mata (*maker–checker*)**: aturan, model AI, dan usulan dari asisten AI harus disetujui orang **lain**
  sebelum aktif.
* **Mode bayangan (*shadow*)**: aturan baru bisa diuji pada lalu lintas nyata tanpa memengaruhi keputusan, sampai
  terbukti efektif.
* **Backtest**: sebelum disetujui, efek sebuah aturan pada data historis terlihat dulu (berapa yang tertangkap, berapa
  yang salah tangkap).
* **Jejak audit** yang tidak bisa diubah: siapa mengubah apa, kapan, dan nilai sebelum/sesudahnya.
* **Versi**: setiap perubahan aturan membuat versi baru, dan setiap keputusan mencatat versi aturan dan model yang
  dipakai.
* **Regulasi berubah?** Unggah versi baru regulasinya. Asisten AI akan membandingkan pasal demi pasal, menilai dampaknya
  terhadap aturan yang sudah ada, dan menyiapkan usulan perubahan untuk ditinjau.

## 6. Peran pengguna

| Peran | Bisa apa |
|---|---|
| Platform admin | Mengelola tenant (penyedia layanan) |
| Tenant admin | Mengelola user, project, dan library regulasi perusahaan |
| Project admin | Mengatur project: anggota, threshold, bobot mesin, sumber data |
| Approver | Menyetujui atau menolak aturan, model, dan usulan AI |
| Analyst | Membuat aturan, melatih model, menangani case, memberi label |
| Viewer | Hanya melihat |

## 7. Siklus perbaikan berkelanjutan

```
Transaksi masuk → dinilai 5 mesin → keputusan + alasan
      ↑                                   ↓
Model & aturan diperbarui      Analis meninjau case & memberi label (fraud / bukan)
      ↑                                   ↓
Asisten AI mengusulkan ← Statistik aturan, pola anomali baru, perubahan regulasi
```

Semakin banyak case yang ditinjau dan diberi label, semakin pintar model AI-nya, dan semakin tepat usulan aturan dari
asisten AI.

## 8. Yang perlu diketahui (batasan)

* Kualitas machine learning terawasi bergantung pada **label**. Di awal, andalkan rule engine dan deteksi anomali.
* Asisten AI memberi **usulan**, bukan keputusan hukum. Interpretasi regulasi tetap menjadi tanggung jawab tim
  compliance.
* Model bahasa berjalan **lokal** (Ollama), sehingga data tidak dikirim ke layanan AI pihak ketiga. Konsekuensinya,
  server membutuhkan CPU/RAM (idealnya GPU) yang memadai.

## 9. Istilah

| Istilah | Arti |
|---|---|
| Rule / aturan | Kondisi yang, bila terpenuhi, menambah skor risiko atau memaksa keputusan |
| Ruleset | Kumpulan aturan untuk satu tujuan, misalnya "Carding" |
| Velocity | Aturan berbasis frekuensi atau jumlah dalam rentang waktu (mis. >5 transaksi/jam) |
| Reference list | Daftar putih/hitam/pantau (mis. kartu yang pernah chargeback) |
| Trapped | Aturan tidak bisa dievaluasi (mis. data kosong atau pembagian dengan nol). Dicatat dan bisa diarahkan ke review. |
| Shadow | Aturan atau model berjalan "diam-diam" untuk diuji tanpa memengaruhi keputusan |
| Graph distance | Jumlah "langkah" hubungan antara dua pelanggan melalui data bersama |
| Label | Penanda hasil investigasi: fraud atau bukan |
