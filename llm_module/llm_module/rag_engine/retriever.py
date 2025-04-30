from langchain.vectorstores import MongoDBAtlasVectorSearch
from langchain.embeddings import OpenAIEmbeddings
import os


def get_mongo_retriever(collection_name="fraud_llm.docs"):
    from pymongo import MongoClient

    mongo_uri = os.getenv("MONGO_URI", "mongodb://localhost:27017")
    db_name = os.getenv("MONGO_DB_NAME", "fraud_detection")

    client = MongoClient(mongo_uri)
    collection = client[db_name][collection_name.split(".")[1]]
    embeddings = OpenAIEmbeddings()
    vectorstore = MongoDBAtlasVectorSearch(collection, embeddings)
    return vectorstore.as_retriever()
