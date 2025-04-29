
# 🧠 Fraud Policy Engine + LLM Assistant (📊🔍🤖)

Fraud Policy Engine adalah sistem evaluasi risiko transaksi berbasis FastAPI yang kini diperkuat oleh **Large Language Models (LLM)** seperti GPT, Gemini, atau DeepSeek melalui pendekatan **RAG (Retrieval-Augmented Generation)**. Sistem ini mendukung integrasi regulasi OJK/BI secara cerdas, serta mampu **merekomendasikan dan mengevaluasi rules dan policies** secara adaptif dan akurat.

---

## 🚀 Fitur Utama

### 💼 Core Fraud Engine
- ✅ CRUD User & Transaction
- ✅ CRUD Policy, Standard Rule, Velocity Rule
- ✅ Evaluate Transaction Risk (manual & batch)
- ✅ Risk Score & Classification (Normal/Suspect/Fraud)
- ✅ Statistik: Per User, Per Rule, Per Policy
- ✅ Export Laporan CSV + Risk Alerts

### 🧠 LLM Engine (LangChain + MongoDB)
- ✅ RAG (Retrieval-Augmented Generation) dari dokumen OJK/BI
- ✅ Chat Assistant untuk tanya jawab regulasi & fraud policy
- ✅ Pembuatan rekomendasi rules & evaluasi policy
- ✅ Analisis rule yang belum tersedia
- ✅ Penjelasan keputusan sistem risk scoring secara natural

---

## 🧱 Arsitektur Integrasi LLM

```mermaid
graph TD
    A[User Upload PDF Regulasi] --> B[Text Split & Embed]
    B --> C[Store ke MongoDB Vector Search]
    D[User Chat / Tanya Natural Language] --> E[Streamlit UI]
    E --> F[LangChain Retriever (MongoDB)]
    F --> G[LLM GPT / Gemini / DeepSeek]
    G --> H[Jawaban, Rekomendasi Rule/Policy]
```

---

## 📂 Struktur Proyek

```
fraud-policy-engine/
├── app/                      # FastAPI Fraud Engine
│   └── ...
├── streamlit_app/           # Streamlit Fraud Dashboard
│   └── app_streamlit.py
├── llm_module/              # Modul LLM + RAG
│   ├── rag_engine/
│   │   ├── retriever.py
│   │   ├── embedder.py
│   │   └── loader.py
│   ├── policy_recommender/
│   │   └── recommender.py
│   └── main.py
├── streamlit_llm_ui/        # Chatbot Interaktif untuk OJK/BI
│   └── app_llm_chat.py
├── Dockerfile
├── docker-compose.yml
├── .env
├── README.md
└── pyproject.toml
```

---

## ⚙️ Cara Menjalankan

### 1. Backend Fraud Engine + Dashboard
```bash
docker-compose up --build
```
- API FastAPI → http://localhost:8000/docs  
- Dashboard Streamlit → http://localhost:8501  

### 2. Modul LLM Chat Assistant (opsional)
```bash
poetry run streamlit run streamlit_llm_ui/app_llm_chat.py
```
- UI Chat OJK/BI → http://localhost:8502

---

## 🛢️ Konfigurasi MongoDB Vector Store (.env)

```dotenv
MONGO_URI=mongodb://localhost:27017
MONGO_DB_NAME=fraud_detection
VECTOR_COLLECTION=fraud_llm.docs
USE_MOCK=false
```

---

## 📦 Dependency Tambahan (LLM)

```toml
[tool.poetry.dependencies]
langchain = "*"
pymongo = "*"
sentence-transformers = "*"
streamlit = "*"
pdfplumber = "*"
altair = "*"
pandas = "*"
requests = "*"
```

---

## 💬 Contoh Interaksi Natural Language

> "Apakah policy saat ini sesuai OJK No.11 Tahun 2022?"  
> "Rule apa yang perlu ditambahkan untuk transaksi > 100 juta?"  
> "Kenapa user123 diklasifikasikan fraud pada April 2025?"

---

## ✅ Roadmap Selanjutnya

| Fitur | Status |
|------|--------|
| Fraud Engine API + UI | ✅ |
| Seeder + Docker Compose | ✅ |
| LLM RAG ke MongoDB | ✅ |
| Chat Assistant Interaktif | ✅ |
| Auto Suggest Rules dari Data | ✅ |
| Evaluasi Policy dari Regulasi | ✅ |
| Push Notification & Tag Fraud | 🔜 |
| Visual Analytics & Monitoring | 🔜 |

---

## 🤝 Lisensi & Kontribusi

Proyek ini open-source dan dapat digunakan untuk edukasi, riset, dan solusi fintech compliance di Indonesia.

