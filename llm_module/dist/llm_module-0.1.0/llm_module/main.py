import os
from rag_engine.loader import load_and_split_pdf
from rag_engine.embedder import store_embeddings

from dotenv import load_dotenv

load_dotenv()  # Will automatically load variables from .env file


def main():
    pdf_path = "../datasource/pojkatifraud.pdf"
    print(f"📄 Loading and splitting: {pdf_path}")
    docs = load_and_split_pdf(pdf_path)
    print(f"🔢 {len(docs)} chunks created.")
    store_embeddings(docs)


if __name__ == "__main__":
    main()
