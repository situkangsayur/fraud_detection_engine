import streamlit as st
from langchain.chat_models import ChatOpenAI
from langchain.chains import RetrievalQA
from llm_module.rag_engine.retriever import get_mongo_retriever
from llm_module.agent.policy_agent import get_policy_llm_agent
from llm_module.uploader.regulation_uploader import upload_new_regulation
from llm_module.audit.audit_logger import log_audit_entry
from llm_module.summarizer.policy_summary import summarize_policy
from llm_module.summarizer.structured_extractor import extract_structured_policy
from llm_module.summarizer.structured_extractor import (
    extract_structured_policy_with_source,
)
from llm_module.validator.rule_schema_validator import validate_rule_structure


from pymongo import MongoClient
import os


"""
st.set_page_config(page_title="🤖 LLM Fraud Policy Assistant", layout="wide")
st.title("🧠 LLM Chat – OJK/BI Regulation & Policy Analysis")

query = st.text_input(
    "Ask me anything about fraud rules, OJK policies, or transactions..."
)

# if query:
#     retriever = get_mongo_retriever()
#     qa = RetrievalQA.from_chain_type(
#         llm=ChatOpenAI(temperature=0), retriever=retriever, return_source_documents=True
#     )
#     result = qa.run(query)
#     st.subheader("📘 Answer")
#     st.write(result)

if query:
    agent = get_policy_llm_agent()
    response = agent.run(query)
    st.subheader("📘 Answer")
    st.write(response)


"""
st.set_page_config(page_title="🧠 Fraud LLM Assistant", layout="wide")

tabs = st.tabs(["🤖 LLM Chat", "📄 Upload Regulation", "🗂️ Audit Trail"])

# --- Tab 1: LLM Chat
with tabs[0]:
    st.title("🤖 Chat with Fraud Policy LLM")

    query = st.text_input(
        "Ask me anything about fraud rules, OJK regulations, or fraud system..."
    )

    if query:
        agent = get_policy_llm_agent()
        response = agent.run(query)
        st.subheader("📘 Answer")
        st.write(response)

        log_audit_entry("llm_chat_query", {"query": query, "response": response})

# --- Tab 2: Upload New Regulation
with tabs[1]:
    st.title("📄 Upload New Regulation")

    uploaded_file = st.file_uploader("Upload a regulation PDF", type=["pdf"])

    if uploaded_file is not None:
        with open("temp_upload.pdf", "wb") as f:
            f.write(uploaded_file.read())

        result = upload_new_regulation("temp_upload.pdf")
        st.success(result)

        log_audit_entry(
            "upload_regulation",
            {"file_uploaded": uploaded_file.name, "status": "success"},
        )

# --- Tab 3: Audit Trail
with tabs[2]:
    st.title("🗂️ Audit Trail Log Viewer")

    mongo_uri = os.getenv("MONGO_URI", "mongodb://localhost:27017")
    db_name = os.getenv("MONGO_DB_NAME", "fraud_detection")
    client = MongoClient(mongo_uri)
    audit_collection = client[db_name]["audit_trail"]

    audit_entries = list(audit_collection.find({}).sort("timestamp", -1))

    if audit_entries:
        for entry in audit_entries:
            st.write(f"🕒 {entry['timestamp']} - **{entry['action_type']}**")
            st.json(entry["details"])
    else:
        st.info("No audit logs found yet.")


with st.sidebar.expander("🧠 Generate Policy Summary"):
    if st.button("📚 Ringkas Kebijakan Regulasi OJK/BI"):
        summary = summarize_policy()
        st.subheader("🧾 Ringkasan Kebijakan")
        st.write(summary)


tabs = st.tabs(
    ["🤖 Chat", "📄 Upload", "🗂️ Audit", "🧠 Ringkasan", "🔍 Ekstraksi Struktural"]
)

with tabs[4]:
    st.title("🔍 Ekstraksi Kebijakan Struktural")
    if st.button("Ekstrak Kebijakan ke Format JSON"):
        structured, sources = extract_structured_policy_with_source()

        st.subheader("📂 Output Structured Policy (JSON)")
        st.code(structured, language="json")

        is_valid, validation_msg = validate_rule_structure(structured)
        if is_valid:
            st.success("✅ Struktur valid! Bisa langsung digunakan.")
        else:
            st.error(f"❌ Struktur tidak valid: {validation_msg}")

        st.subheader("🔦 Sumber Referensi Regulasi")
        for i, src in enumerate(sources):
            with st.expander(f"Source #{i+1}"):
                st.write(src)
