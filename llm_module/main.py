import os
from rag_engine.loader import load_and_split_pdf
from rag_engine.embedder import store_embeddings


def main():
    pdf_path = "data/regulasi_ojk.pdf"
    print(f"📄 Loading and splitting: {pdf_path}")
    docs = load_and_split_pdf(pdf_path)
    print(f"🔢 {len(docs)} chunks created.")
    store_embeddings(docs)


if __name__ == "__main__":
    main()
