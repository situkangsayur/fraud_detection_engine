from langchain.chains import RetrievalQA
from langchain.chat_models import ChatOpenAI
from langchain_community.chat_models import ChatOllama
from llm_module.rag_engine.retriever_state import get_live_retriever


def summarize_policy():
    retriever = get_live_retriever()
    chain = RetrievalQA.from_chain_type(
        # llm=ChatOpenAI(temperature=0), retriever=retriever
        llm=ChatOllama(
            base_url="http://192.168.11.3:11434", model="deepseek-8b", temperature=0
        )
    )

    prompt = "Buat ringkasan kebijakan utama dari regulasi ini, terutama terkait penilaian risiko individu, batasan transaksi mencurigakan, dan prosedur mitigasi penipuan."
    result = chain.run(prompt)
    return result
