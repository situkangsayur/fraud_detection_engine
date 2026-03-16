.PHONY: run test test-cov seed dev-up dev-down dev-run bdd-install bdd-test

# === Production ===
run:
	poetry run uvicorn app.main:app --host 0.0.0.0 --port 8000 --reload

# === Development (MongoDB via Docker, app langsung) ===
dev-up:
	docker compose -f docker-compose.dev.yml up -d
	@echo "MongoDB dev started on port 27018"

dev-down:
	docker compose -f docker-compose.dev.yml down

dev-run:
	MONGO_URI=mongodb://root:root@localhost:27018 MONGO_DB_NAME=fraud_detection_dev \
	poetry run uvicorn app.main:app --host 0.0.0.0 --port 8000 --reload

dev-seed:
	MONGO_URI=mongodb://root:root@localhost:27018 MONGO_DB_NAME=fraud_detection_dev \
	poetry run python -m app.seeder.seeder

# === Unit Testing ===
test:
	USE_MOCK=true poetry run pytest --asyncio-mode=auto -v

test-cov:
	USE_MOCK=true poetry run pytest --asyncio-mode=auto --cov=app --cov-report=term-missing --cov-report=html

# === BDD & E2E Testing ===
bdd-install:
	cd e2e && npm install && npx playwright install --with-deps chromium

bdd-test:
	cd e2e && npm test

bdd-test-api:
	cd e2e && npm run test:api

bdd-test-ui:
	cd e2e && npm run test:ui

# === Seed ===
seed:
	poetry run python -m app.seeder.seeder
