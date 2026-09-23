---

## 📁 `streamlit_llm_ui/README.md`

```markdown
# 💬 Streamlit LLM Chat UI – Fraud Policy Engine

Aplikasi antarmuka interaktif untuk berkomunikasi dengan model LLM (GPT, Gemini, DeepSeek, dll.)  
Dapat digunakan untuk:

- 🤖 Bertanya tentang regulasi OJK/BI
- 🧩 Melihat saran rule & policy dari model AI
- 📄 Upload dokumen regulasi (PDF)
- 🧠 Melihat ringkasan kebijakan
- 🔍 Mengekstrak rule dalam format JSON
- 🗂️ Melihat audit trail aktivitas

---

## 🖼️ Tampilan Fitur

- **Chat GPT/Gemini lokal**
- **Upload regulasi → embed → reload retriever**
- **Policy summary → natural language**
- **Extract rule/policy dalam format JSON**
- **Validasi struktur + Highlight sumber**
- **Audit trail viewer**

---

## 🚀 Menjalankan Manual

```bash
poetry run streamlit run streamlit_llm_ui/app_llm_chat.py
