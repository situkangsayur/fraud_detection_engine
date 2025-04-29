import os


def get_llm():
    provider = os.getenv("LLM_PROVIDER", "ollama").lower()

    if provider == "ollama":
        from langchain_community.llms import Ollama

        return Ollama(
            model=os.getenv("OLLAMA_MODEL", "deepseek-8b-instruct"),
            base_url=os.getenv("OLLAMA_BASE_URL", "http://localhost:11434"),
            temperature=0,
        )

    elif provider == "openai":
        from langchain.chat_models import ChatOpenAI

        return ChatOpenAI(
            temperature=0,
            model_name=os.getenv("OPENAI_MODEL", "gpt-4"),
            openai_api_key=os.getenv("OPENAI_API_KEY"),
        )

    elif provider == "gemini":
        from langchain_google_genai import ChatGoogleGenerativeAI

        return ChatGoogleGenerativeAI(
            model="gemini-pro",
            temperature=0,
            google_api_key=os.getenv("GOOGLE_API_KEY"),
        )

    else:
        raise ValueError(f"Unsupported LLM Provider: {provider}")
