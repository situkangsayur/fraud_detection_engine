#!/bin/bash
echo "🧪 Running tests with Mongomock..."
USE_MOCK=true poetry run pytest --asyncio-mode=auto --cov=app --cov-report=term-missing
