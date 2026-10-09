#!/bin/bash
echo "🔁 Seeding production data..."
poetry run python app/seeder/seeder.py

echo "running service fraud engine"
poetry run uvicorn app.main:app --host 0.0.0.0 --port 8000
