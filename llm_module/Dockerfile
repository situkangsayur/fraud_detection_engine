FROM python:3.12-slim-bullseye

ARG PROJECT_NAME=llm_module
ARG PROJECT_VERSION=0.1.0

RUN apt-get update && apt-get install -y --no-install-recommends curl build-essential && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

ENV POETRY_VERSION=1.8.2
RUN curl -sSL https://install.python-poetry.org | python3 -

WORKDIR /app
ENV VIRTUAL_ENV=/opt/venv
RUN python3 -m venv $VIRTUAL_ENV
ENV PATH="$VIRTUAL_ENV/bin:$PATH"
RUN ln -s /root/.local/bin/poetry /opt/venv/bin/poetry

COPY dist/${PROJECT_NAME}-${PROJECT_VERSION}.tar.gz /app/

RUN poetry init -n && poetry add /app/${PROJECT_NAME}-${PROJECT_VERSION}.tar.gz

# COPY app/seeder/seeder.py /app/${PROJECT_NAME}/seeder.py
ENV PYTHONPATH=/app

CMD ["python3"]
