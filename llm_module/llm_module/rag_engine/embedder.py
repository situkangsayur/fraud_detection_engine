from langchain.embeddings import OpenAIEmbeddings  # or change to HuggingFaceEmbeddings
from langchain.vectorstores import MongoDBAtlasVectorSearch
from pymongo import MongoClient
import os
from langchain_community.embeddings import HuggingFaceEmbeddings

from langchain.embeddings import HuggingFaceEmbeddings


def store_embeddings(docs, collection_name="fraud_llm.docs"):
    mongo_uri = os.getenv("MONGO_URI", "mongodb://localhost:27017")
    db_name = os.getenv("MONGO_DB_NAME", "fraud_detection")

    # Pakai HuggingFace embeddings lokal
    embeddings = HuggingFaceEmbeddings(
        model_name="sentence-transformers/all-MiniLM-L6-v2"
    )

    # embeddings = HuggingFaceEmbeddings(model_name="BAAI/bge-small-en-v1.5")

    from langchain_community.vectorstores import MongoDBAtlasVectorSearch
    from pymongo import MongoClient

    client = MongoClient(mongo_uri)
    collection = client[db_name][collection_name.split(".")[1]]
    vectorstore = MongoDBAtlasVectorSearch(collection, embeddings)
    vectorstore.add_documents(docs)


"""
def store_embeddings(docs, collection_name="fraud_llm.docs"):
    mongo_uri = os.getenv("MONGO_URI", "mongodb://localhost:27017")
    db_name = os.getenv("MONGO_DB_NAME", "fraud_detection")
    embeddings = OpenAIEmbeddings()
    client = MongoClient(mongo_uri)
    collection = client[db_name][collection_name.split(".")[1]]

    vectorstore = MongoDBAtlasVectorSearch(collection, embeddings)
    vectorstore.add_documents(docs)

"""
