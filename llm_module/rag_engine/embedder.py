from langchain.embeddings import OpenAIEmbeddings  # or change to HuggingFaceEmbeddings
from langchain.vectorstores import MongoDBAtlasVectorSearch
from pymongo import MongoClient
import os


def store_embeddings(docs, collection_name="fraud_llm.docs"):
    mongo_uri = os.getenv("MONGO_URI", "mongodb://localhost:27017")
    db_name = os.getenv("MONGO_DB_NAME", "fraud_detection")
    embeddings = OpenAIEmbeddings()
    client = MongoClient(mongo_uri)
    collection = client[db_name][collection_name.split(".")[1]]

    vectorstore = MongoDBAtlasVectorSearch(collection, embeddings)
    vectorstore.add_documents(docs)
    print(f"✅ Stored {len(docs)} docs to MongoDB vector collection.")
