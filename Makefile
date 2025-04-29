.PHONY: run test test-cov seed

run:
	poetry run uvicorn app.main:app --host 0.0.0.0 --port 8000 --reload

test:
	USE_MOCK=true poetry run pytest --asyncio-mode=auto

test-cov:
	USE_MOCK=true poetry run pytest --asyncio-mode=auto --cov=app --cov-report=term-missing

seed:
	poetry run python seeder.py
