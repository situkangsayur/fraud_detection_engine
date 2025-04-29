#!/bin/bash
echo "🔁 Seeding production data..."
poetry run python app/seeder/seeder.py
