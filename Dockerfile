FROM python:3.12-slim-bullseye

ARG PROJECT_NAME=fraud_detection_engine
ARG PROJECT_VERSION
ARG COMMON_VERSION

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

RUN tar -xzf /app/${PROJECT_NAME}-${PROJECT_VERSION}.tar.gz -C /app && \
    mv /app/${PROJECT_NAME}-${PROJECT_VERSION} ${PROJECT_NAME} && \
    cd ${PROJECT_NAME} && poetry install

WORKDIR /app/${PROJECT_NAME}
COPY .env /app/${PROJECT_NAME}/
# COPY app/seeder/seeder.py /app/${PROJECT_NAME}/seeder.py
ENV PYTHONPATH=/app

CMD poetry run python app/seeder/seeder.py && \
    poetry run uvicorn app.main:app --host 0.0.0.0 --port 8000
