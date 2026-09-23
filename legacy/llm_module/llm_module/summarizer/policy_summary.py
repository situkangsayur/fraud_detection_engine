from langchain.chains import RetrievalQA
from langchain.chat_models import ChatOpenAI
from langchain_community.chat_models import ChatOllama
from llm_module.core.llm_provider import get_llm
from llm_module.rag_engine.retriever_state import get_live_retriever


def summarize_policy():
    retriever = get_live_retriever()
    llm = get_llm()  # dynamic load dari .env
    chain = RetrievalQA.from_chain_type(
        # llm=ChatOpenAI(temperature=0), retriever=Retriever
        llm=llm
    )

    prompt = "Buat ringkasan kebijakan utama dari regulasi ini, terutama terkait penilaian risiko individu, batasan transaksi mencurigakan, dan prosedur mitigasi penipuan."
    result = chain.run(prompt)
    return result
