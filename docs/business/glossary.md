# Glosarium

Istilah fraud, mesin deteksi, machine learning, graph, LLM/agent, dan platform yang dipakai di dokumen dan tampilan.
Urut abjad. Kolom **Kelompok**: Fraud, Engine (rule & keputusan), ML, Graph, AI (LLM/agent), Platform, Metrik.

| Istilah | Kelompok | Arti |
|---|---|---|
| **Account takeover (ATO)** | Fraud | Akun pelanggan diambil alih penipu: login dari perangkat baru, password/email/HP diganti, lalu dipakai bertransaksi. |
| **Agent (agen AI)** | AI | Model bahasa yang bisa memanggil alat (*tools*) secara berulang untuk menyelesaikan tugas, bukan hanya menjawab satu kali. Asisten saat ini adalah satu agen dengan 14 alat. |
| **Anomaly detection** | ML | Deteksi data yang "tidak biasa" tanpa butuh label. Di platform: isolation forest, LOF, autoencoder. |
| **Approve / Review / Decline** | Engine | Tiga keputusan akhir. Default: skor < 50 approve, 50–79 review (masuk antrean case), ≥ 80 decline. |
| **Approver** | Platform | Peran yang menyetujui atau menolak rule, ruleset, model, dan usulan. Tidak bisa menyetujui buatannya sendiri. |
| **AUC (ROC-AUC)** | Metrik | Ukuran kualitas ranking skor, 0,5 (acak) sampai 1,0 (sempurna). AUC 0,95 berarti 95% kemungkinan fraud acak mendapat skor lebih tinggi dari transaksi sah acak. Tidak bergantung pada ambang. |
| **Audit log** | Platform | Catatan siapa melakukan apa, kapan, dengan nilai sebelum/sesudah. Append-only: tidak bisa diubah atau dihapus. |
| **Backtest** | Engine | Menjalankan rule pada data historis untuk melihat berapa yang kena, precision, dan recall sebelum rule diaktifkan. |
| **Bank account takeover** | Fraud | Rekening bank/e-wallet korban diambil alih dan dikuras ke rekening tujuan baru. |
| **BFF (backend for frontend)** | Platform | Lapisan server di aplikasi web yang menyimpan token login di cookie aman dan meneruskan permintaan ke API. |
| **BIN** | Fraud | 6–8 digit awal nomor kartu yang menunjukkan bank penerbit dan negaranya. |
| **Bobot engine** | Engine | Besarnya pengaruh tiap mesin pada skor akhir. Diatur per project. |
| **Card testing** | Fraud | Penipu menguji banyak kartu curian dengan transaksi kecil untuk mencari kartu yang masih aktif. |
| **Carding** | Fraud | Penggunaan data kartu curian untuk berbelanja. |
| **Case** | Platform | Satu berkas investigasi. Dibuka otomatis untuk keputusan review/decline, digabung per pelanggan. Diselesaikan sebagai fraud (dengan tipologi) atau legit. |
| **Champion / challenger** | Engine | Membandingkan versi yang berjalan (*champion*) dengan versi baru di shadow (*challenger*) pada lalu lintas yang sama. |
| **Chargeback** | Fraud | Dana transaksi ditarik kembali oleh pemilik kartu lewat bank, sering karena transaksi tidak diakui. |
| **Chunk** | AI | Potongan dokumen regulasi (biasanya per pasal) yang disimpan untuk pencarian. |
| **Clustering** | ML | Mengelompokkan event yang mirip tanpa label. Di platform: HDBSCAN, k-means, DBSCAN, gaussian mixture. |
| **Community (komunitas graph)** | Graph | Kelompok pelanggan yang lebih rapat terhubung satu sama lain dibanding ke luar (algoritma Louvain). Fraud rate komunitas bisa dipakai di rule. |
| **Component (komponen terhubung)** | Graph | Sekelompok pelanggan yang saling terhubung lewat entitas bersama. Komponen besar dengan fraud rate tinggi sering berupa jaringan promo atau mule. |
| **Composite rule** | Engine | Rule velocity yang histori-nya disaring dulu dengan kondisi, mis. "≥ 3 pelanggan di perangkat yang sama memakai **kode promo yang sama** dalam 7 hari". |
| **Credential stuffing** | Fraud | Mencoba pasangan email/password bocoran secara massal ke banyak akun. |
| **Critic** | AI | (Rencana multi-agent) agen yang memeriksa draft rule secara independen: sitasi benar, tidak duplikat, dampak wajar. |
| **Degraded mode** | Engine | Jika satu mesin gagal atau terlambat, keputusan tetap diambil dari mesin lain dan ditandai *degraded*. Event bisa di-*rescore*. |
| **Drift (PSI)** | ML | Perubahan pola data dibanding periode acuan, diukur dengan *Population Stability Index*. "Significant" berarti pola data berubah dan model mungkin perlu dilatih ulang. |
| **Embedding** | AI | Representasi angka dari teks sehingga teks bermakna mirip bisa ditemukan. Di platform: model `bge-m3`. |
| **Entity (entitas)** | Graph | Data pengenal yang bisa dipakai bersama: email, HP, perangkat, IP, kartu, rekening, alamat, referensi transaksi, API client. |
| **Entity resolution** | Graph | Menyatukan penulisan berbeda dari entitas yang sama (mis. `0812-3456-789` dan `+62 812 3456 789`). |
| **Event** | Platform | Satu aktivitas yang dinilai: transaksi, login, perubahan akun, klaim promo, payout, registrasi, refund, atau jenis kustom. |
| **Fan-in / fan-out** | Fraud | Banyak pengirim ke satu penerima (fan-in) atau satu pengirim ke banyak penerima (fan-out). Pola khas money mule dan pencucian uang. |
| **Feature (fitur)** | ML | Nilai turunan yang dipakai model dan rule, mis. jumlah transaksi 24 jam atau umur akun. |
| **Field catalog** | Platform | Daftar semua field yang bisa dipakai di rule dan model, termasuk kolom asli sumber (`source.*`). |
| **Formula** | Engine | Ekspresi matematika di rule, mis. `F(x,y,z) = 2x + 2^y / z^2`, dengan variabel diikat ke field. |
| **FPR (false positive rate)** | Metrik | Persentase transaksi sah yang ikut ditandai review/decline. FPR 23% berarti hampir 1 dari 4 transaksi sah tertahan. |
| **Fraud proximity / graph distance** | Graph | Jumlah "langkah" hubungan dari seorang pelanggan ke pelanggan fraud terdekat lewat entitas bersama. |
| **Gemini** | AI | Model bahasa Google yang bisa dipakai sebagai opsi untuk chat. Data pertanyaan dikirim ke Google. |
| **Ground truth** | Metrik | Kebenaran sebenarnya (fraud atau bukan) pada data uji, dipakai untuk mengukur hasil. Tidak pernah dikirim ke platform. |
| **Hallucination (halusinasi)** | AI | Model bahasa menyebut fakta yang tidak ada, mis. nomor pasal karangan. Karena itu sitasi harus diperiksa. |
| **Hash / fingerprint** | Platform | Sidik jari satu arah dari nomor kartu/rekening (HMAC-SHA256 dengan kunci per tenant). Bisa dicocokkan, tidak bisa dibalik. |
| **Hybrid search** | AI | Pencarian pasal yang menggabungkan kecocokan kata kunci (BM25) dan kecocokan makna (embedding). |
| **Label** | ML | Hasil investigasi: fraud atau legit. Bahan belajar model supervised dan titik acuan graph. |
| **Label maturity** | ML | Event yang lebih tua dari N hari (default 14) tanpa laporan fraud dianggap legit saat melatih model. |
| **LLM (large language model)** | AI | Model bahasa yang memahami dan menulis teks. Di demo: `qwen3:8b` lewat Ollama. |
| **Maker–checker** | Platform | Prinsip empat mata: pembuat perubahan dan penyetujunya harus orang berbeda. |
| **Mapping** | Platform | Pemetaan kolom data sumber ke field standar platform, beserta transformasinya. Ber-versi. |
| **Masking** | Platform | Menampilkan data sensitif secara tersamar, mis. `a***@domain.com` atau `411111********1111`. |
| **Money mule** | Fraud | Rekening/akun penampung yang dipakai untuk menerima dan meneruskan dana hasil kejahatan. |
| **Multi-agent** | AI | (Rencana) beberapa agen dengan tugas berbeda (regulasi, analitik, desain rule, simulasi, kritik) yang bekerja berurutan dan saling memeriksa. Lihat [multi-agent.md](../technical/multi-agent.md). |
| **Noisy-OR** | Engine | Cara default menggabungkan skor mesin: bukti dari beberapa mesin saling menguatkan, dan mesin yang "diam" tidak melemahkan sinyal kuat. Alternatif: rata-rata berbobot. |
| **Ollama** | AI | Perangkat lunak untuk menjalankan model bahasa di server sendiri. |
| **Precision** | Metrik | Dari semua yang ditandai, berapa persen yang benar fraud. |
| **Project** | Platform | Satu titik perlindungan dalam satu perusahaan (mis. checkout, retur), dengan rule, model, graph, dan pengaturan sendiri. |
| **Promo abuse / promo farm** | Fraud | Satu orang membuat banyak akun untuk memanen voucher, diskon, atau cashback. |
| **Proposal (usulan rule)** | Platform | Usulan rule baru, perubahan, pensiun, atau ambang dari AI atau analyst. Berstatus pending sampai disetujui approver; jika disetujui, rule masuk shadow. |
| **RAG (retrieval-augmented generation)** | AI | Teknik menjawab dengan terlebih dahulu mencari dokumen relevan (pasal regulasi), lalu menyusun jawaban berdasarkan dokumen itu beserta sitasinya. |
| **RBAC** | Platform | *Role-based access control*: hak akses ditentukan oleh peran. |
| **Reason code** | Engine | Alasan yang bisa dibaca manusia di balik skor, mis. kode rule yang kena dan kontribusinya. |
| **Recall** | Metrik | Dari semua fraud, berapa persen yang tertangkap. |
| **Reference list** | Engine | Daftar putih, hitam, pantau, atau lookup (mis. `card_blacklist`, `merchant_limits`). Bisa per project atau untuk seluruh tenant. |
| **Refund abuse** | Fraud | Klaim retur/refund berulang atau palsu, sering oleh akun-akun yang saling terkait. |
| **Rescore** | Engine | Menilai ulang satu event dengan rule dan model yang berlaku sekarang. |
| **RLS (row-level security)** | Platform | Fitur database yang memastikan setiap permintaan hanya melihat baris milik tenant yang aktif. |
| **Rule** | Engine | Kondisi yang, bila terpenuhi, menambah skor risiko atau memaksa keputusan (paksa review, tolak, atau setujui). Jenis: simple, velocity, velocity statistik, composite, reference, graph. |
| **Ruleset** | Engine | Kumpulan rule untuk satu tujuan (mis. "Carding") dengan bobot per rule dan cara agregasi. |
| **Shadow mode** | Engine | Rule, ruleset, atau versi baru berjalan pada lalu lintas nyata dan hasilnya dicatat, tetapi tidak memengaruhi keputusan. |
| **Sitasi** | AI | Rujukan ke dokumen dan pasal sumber yang dipakai asisten AI dalam jawabannya. |
| **Stage (tahap)** | Platform | Jenis titik perlindungan sebuah project: `pre_payment`, `post_payment`, `returns`, `promo`, `account_security`, `payout`, atau `custom`. Menentukan template awal. |
| **Supernode** | Graph | Entitas yang dipakai terlalu banyak pelanggan (mis. IP Wi-Fi publik). Diabaikan agar tidak menghubungkan orang yang tidak berkaitan. |
| **Supervised ML** | ML | Model yang belajar dari contoh berlabel. Bawaan: `mlp_backprop` (jaringan saraf), logistic regression, gradient boosting, random forest. |
| **System breach** | Fraud | Celah di sistem/API dipakai untuk membuat transaksi massal dari banyak akun. |
| **Template** | Platform | Paket rule, ruleset, dan reference list bawaan yang dipasang otomatis saat project dibuat, sesuai stage. |
| **Tenant** | Platform | Satu perusahaan pengguna. Data antar tenant terisolasi total. |
| **Threshold (ambang)** | Engine | Batas skor untuk review dan decline. Diatur per project; perlu dikalibrasi karena tiap tahap punya pola berbeda. |
| **Tipologi** | Fraud | Kategori modus fraud, mis. carding, account takeover, promo abuse, money mule. |
| **Tool (alat)** | AI | Fungsi yang bisa dipanggil agen AI, mis. `search_regulations` atau `backtest_rule_definition`. |
| **Trapped** | Engine | Rule tidak bisa dievaluasi (data kosong, pembagian dengan nol, histori kurang, daftar tidak ada). Bisa diabaikan, diberi skor, atau dipaksa review. |
| **Unsupervised ML** | ML | Model yang belajar tanpa label: deteksi anomali dan clustering. Berguna untuk pola baru. |
| **Velocity** | Engine | Rule berbasis frekuensi atau jumlah dalam rentang waktu, mis. > 5 transaksi per jam per kartu. |
| **Velocity statistik** | Engine | Membandingkan nilai sekarang dengan distribusi historisnya: z-score, peluang gaussian, tren linear, lonjakan poisson. |
| **Versi (versioning)** | Engine | Setiap perubahan rule membuat versi baru yang tidak bisa diubah. Setiap keputusan mencatat versi yang dipakai. |
| **Webhook** | Platform | Cara mengirim event ke platform secara langsung lewat HTTP dengan API key. |
