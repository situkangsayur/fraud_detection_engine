from langchain.chains import RetrievalQA
from langchain.chat_models import ChatOpenAI
from langchain_community.chat_models import ChatOllama
from llm_module.rag_engine.retriever_state import get_live_retriever

from llm_module.core.llm_provider import get_llm


def extract_structured_policy():
    retriever = get_live_retriever()
    chain = RetrievalQA.from_chain_type(
        llm=get_llm()
        # llm=ChatOpenAI(temperature=0), retriever=retriever
        #        llm=ChatOllama(
        #            base_url="http://192.168.11.3:11434", model="deepseek-8b", temperature=0
    )

    prompt = """
    Bacalah seluruh dokumen regulasi.
    Ekstrak dan buatkan struktur JSON untuk rule atau policy fraud detection berdasarkan peraturan ini.

    Format JSON seperti:
    {
      "type": "StandardRule" or "VelocityRule",
      "description": "...",
      "field": "...",
      "operator": "...",  # hanya jika StandardRule
      "value": "...",     # hanya jika StandardRule
      "aggregation_function": "...",  # hanya jika VelocityRule
      "threshold": "..."  # hanya jika VelocityRule
    }

    Pastikan JSON sesuai dan hanya berdasarkan perintah atau batasan yang ada di dokumen regulasi.
    """

    result = chain.run(prompt)
    return result


def extract_structured_policy_with_source():
    retriever = get_live_retriever()
    chain = RetrievalQA.from_chain_type(
        llm=get_llm(),
        retriever=retriever,
        return_source_documents=True,  # 👉 Enable source return
    )

    prompt = """
    Bacalah dokumen regulasi.
    Ekstrak dan buatkan struktur JSON untuk rule atau policy fraud detection sesuai dokumen.
    Format:
    {
      "type": "StandardRule" or "VelocityRule",
      "description": "...",
      "field": "...",
      "operator": "...",
      "value": "...",
      "aggregation_function": "...",
      "threshold": "..."
    }
    """

    result = chain(prompt)

    structured_json = result["result"]
    sources = [doc.page_content for doc in result["source_documents"]]

    return structured_json, sources
