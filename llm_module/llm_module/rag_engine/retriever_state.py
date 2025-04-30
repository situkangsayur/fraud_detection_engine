from llm_module.rag_engine.retriever import get_mongo_retriever

# Global retriever cache
retriever_instance = None


def get_live_retriever():
    global retriever_instance
    if retriever_instance is None:
        retriever_instance = get_mongo_retriever()
    return retriever_instance


def reset_retriever():
    global retriever_instance
    retriever_instance = None
    print("✅ Retriever cache cleared.")
