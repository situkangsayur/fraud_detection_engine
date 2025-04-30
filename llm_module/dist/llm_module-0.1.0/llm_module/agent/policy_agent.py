# from langchain.chat_models import ChatOpenAI

from langchain_community.chat_models import ChatOllama

from langchain.agents import initialize_agent, AgentType
from llm_module.agent.tools import (
    fetch_rule_statistics,
    fetch_policy_statistics,
    create_sample_standard_rule,
    post_new_rule,
)
from llm_module.rag_engine.retriever import get_mongo_retriever

from langchain.memory import ConversationBufferMemory


"""
def get_policy_llm_agent():
    llm = ChatOpenAI(temperature=0)
    retriever = get_mongo_retriever()

    tools = [
        fetch_rule_statistics,
        fetch_policy_statistics,
        create_sample_standard_rule,
    ]

    agent = initialize_agent(
        tools=tools,
        llm=llm,
        agent=AgentType.ZERO_SHOT_REACT_DESCRIPTION,
        verbose=True,
        handle_parsing_errors=True,
    )

    return agent
"""
from llm_module.core.llm_provider import get_llm

llm = get_llm()


def get_policy_llm_agent():
    # llm = ChatOpenAI(temperature=0)

    # llm = ChatOllama(
    #     base_url="http://192.168.11.3:11434", model="deepseek-r1:8b", temperature=0
    # )
    retriever = get_mongo_retriever()

    memory = ConversationBufferMemory(memory_key="chat_history", return_messages=True)

    tools = [
        fetch_rule_statistics,
        fetch_policy_statistics,
        create_sample_standard_rule,
        post_new_rule,
    ]

    agent = initialize_agent(
        tools=tools,
        llm=llm,
        agent=AgentType.CONVERSATIONAL_REACT_DESCRIPTION,
        verbose=True,
        memory=memory,
        handle_parsing_errors=True,
    )

    return agent
