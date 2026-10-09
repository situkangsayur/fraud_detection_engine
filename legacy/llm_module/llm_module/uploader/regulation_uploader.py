import os
from pymongo import MongoClient
from llm_module.rag_engine.loader import load_and_split_pdf
from llm_module.rag_engine.embedder import store_embeddings

from llm_module.rag_engine.retriever_state import reset_retriever


def upload_new_regulation(pdf_file_path: str):
    """Upload and embed new regulation PDF to MongoDB VectorDB"""
    docs = load_and_split_pdf(pdf_file_path)
    store_embeddings(docs)
    reset_retriever()  # 👉 Reset retriever so next query gets fresh data
    return f"✅ Uploaded and embedded {len(docs)} regulation chunks. Retriever updated."
