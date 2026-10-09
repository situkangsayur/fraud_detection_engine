# 🛡️ Fraud Detection Engine + LLM Assistant – Full Documentation

---

## 📌 Overview

Sistem ini adalah platform modular berbasis FastAPI, MongoDB, Streamlit, dan LLM (GPT/Gemini/Ollama) untuk:

- 🚦 Menilai risiko transaksi secara otomatis
- ⚙️ Mengelola rule engine berbasis policy, standard, dan velocity rule
- 🧠 Mengintegrasikan LLM untuk membaca regulasi OJK/BI dan menyarankan struktur rule/policy
- 📊 Melakukan analisis performa fraud rule/policy
- 💬 Menyediakan antarmuka Streamlit untuk eksplorasi dan interaksi AI

---

## 📂 Struktur Proyek

```
fraud_detection_engine/
├── app/                     # FastAPI core (user, transaction, rule, policy, processing)
├── llm_module/              # Modul embedding, retriever, summarizer, agent, validation
├── streamlit_llm_ui/        # Chat-based Streamlit UI
├── Dockerfile               # Untuk fraud_engine
├── Dockerfile.llm_chat      # Untuk llm_chat_ui
├── Dockerfile.llm_embedder  # Untuk llm_module/main.py
├── docker-compose.yml
├── .env
└── README.md
```

---

## 📈 Mermaid Diagram – Arsitektur Umum Fraud Detection Engine + LLM

```mermaid
flowchart TD
    subgraph Frontend
        A1[🧑 User] --> A2[🖥️ Streamlit UI Fraud Engine]
        A1 --> A3[💬 Streamlit Chat LLM UI]
    end

    subgraph CoreEngine[FastAPI Engine]
        A2 --> B1[🧾 /transaction - Evaluasi transaksi]
        A2 --> B2[📋 /rule, /policy - CRUD engine]
        A3 --> B3[📊 /stats - Get statistik rule/policy]
    end

    subgraph LLM_Module[🧠 LLM Assistant]
        A3 --> C1[LangChain Agent]
        C1 --> C2["🔎 MongoDB VectorStore - Regulasi"]
        C1 --> C3[🛠️ REST Tool - Hit API FastAPI]
        C1 --> C4[📥 Rule Recommender]
        C1 --> C5[✅ Rule Validator]
        C1 --> C6[📤 Auto Poster ke API FastAPI]
        C1 --> C7[📚 Ringkasan PDF Regulasi]
    end

    B1 --> D1[📦 MongoDB - Transaksi]
    B2 --> D2[🧩 MongoDB - Rules & Policy]
    C1 --> D2
    C1 --> D3[🗂️ MongoDB - Audit Log]
```

---

## 🛠️ Penjelasan Teknis – Implementasi LLM

### 1. RAG (Retrieval-Augmented Generation)
- Load dokumen regulasi PDF
- Embedding via OpenAI atau HuggingFace
- Disimpan ke MongoDB Vector Store
- Digunakan retriever untuk query berbasis konteks

### 2. LangChain Agent
- Menggunakan tools:
  - REST API Tool (fetch stats)
  - Rule JSON builder
  - Poster rule
- Dapat menggunakan OpenAI, Ollama, atau Gemini
- Memory support untuk multi-turn chat

### 3. Validasi dan Poster
- Hasil ekstraksi divalidasi menggunakan `rule_schema_validator`
- Jika valid → dikirim ke API `/rule/standard` atau `/rule/velocity`
- Jika tidak → ditampilkan sebagai error

### 4. Audit Trail
- Semua interaksi agent, upload, dan ekstraksi dicatat di collection MongoDB `audit_trail`

---

## 📘 Mermaid – Alur Agent Reasoning untuk Rule Rekomendasi

```mermaid
sequenceDiagram
    participant U as User (Streamlit)
    participant A as LangChain Agent
    participant R as MongoDB VectorDB
    participant S as FastAPI Stats API
    participant V as Rule Validator
    participant P as FastAPI Rule API

    U->>A: "Tolong buat rule baru..."
    A->>R: retrieve regulasi dari MongoDB
    A->>S: fetch stats policy/rule
    A->>A: reasoning (combine regulasi + statistik)
    A->>V: validate rule JSON structure
    alt Valid
        A->>P: POST rule ke /rule endpoint
        P-->>A: 200 OK
        A-->>U: ✅ Rule berhasil dikirim
    else Invalid
        V-->>A: ❌ validation failed
        A-->>U: Tampilkan error dan source
    end
```

---

## 🔄 Alur Upload Regulasi dan Auto-Retriever Reload

```mermaid
sequenceDiagram
    participant U as User (Upload PDF)
    participant S as Streamlit LLM UI
    participant E as Embedder
    participant M as MongoDB Vector
    participant C as Retriever Cache

    U->>S: Upload PDF OJK
    S->>E: Simpan file & proses
    E->>M: Simpan embedding
    E->>C: Reset retriever cache
    S-->>U: ✅ Berhasil, siap digunakan
```

---

## 🧪 API Endpoint Overview

| Endpoint | Method | Keterangan |
|----------|--------|------------|
| `/api/v1/user/` | GET/POST | Manajemen user |
| `/api/v1/transaction/` | POST/GET | Transaksi baru & daftar |
| `/api/v1/policy/` | POST/GET | Policy baru & daftar |
| `/api/v1/rule/standard` | POST/GET | Rule standard |
| `/api/v1/rule/velocity` | POST/GET | Rule velocity |
| `/api/v1/process/transaction` | POST | Evaluasi transaksi |
| `/api/v1/stats/...` | GET | Statistik rule/policy/transaksi |

---

## 🔧 Konfigurasi `.env`

```dotenv
MONGO_URI=mongodb://mongo:27017
MONGO_DB_NAME=fraud_detection
LLM_PROVIDER=ollama
OLLAMA_MODEL=deepseek-8b-instruct
OLLAMA_BASE_URL=http://localhost:11434
OPENAI_API_KEY=your-openai-key
GOOGLE_API_KEY=your-google-api-key
```

---

## 🛠️ Teknologi

- FastAPI
- MongoDB
- Streamlit
- LangChain
- Ollama / OpenAI / Gemini
- Docker + Poetry

---

## ✅ Status

| Komponen | Status |
|----------|--------|
| FastAPI Fraud Engine | ✅ |
| LLM Chat UI | ✅ |
| LangChain Agent | ✅ |
| Embedder + Retriever | ✅ |
| Auto retriever refresh | ✅ |
| Audit Log | ✅ |
| Rule Extractor + Validator | ✅ |
