#!/usr/bin/env bash
# Copy docs/ to the Obsidian vault on p14s (one-way: the repo is the source of truth).
# Usage: docs/sync-to-vault.sh [ssh-host]   (default: p14s)
set -euo pipefail

HOST="${1:-p14s}"
VAULT_DIR="/home/hendri/Documents/obsidian/MyNotes/nvda11/fraud-project/dokumentasi"
DOCS_DIR="$(cd "$(dirname "$0")" && pwd)"

ssh "$HOST" "mkdir -p '$VAULT_DIR'"
rsync -a --delete \
  --exclude 'sync-to-vault.sh' \
  --exclude '00 Index Dokumentasi.md' \
  "$DOCS_DIR/" "$HOST:$VAULT_DIR/"

# Obsidian landing note (not part of the repo).
ssh "$HOST" "cat > '$VAULT_DIR/00 Index Dokumentasi.md'" <<EOF
---
title: Dokumentasi Fraud Detection Platform v2
tags: [fraud-detection, dokumentasi]
synced: $(date +%F\ %H:%M)
source: nvda11-gpu ~/own_project/fraud_detection_engine/docs ($(git -C "$DOCS_DIR" rev-parse --abbrev-ref HEAD) @ $(git -C "$DOCS_DIR" rev-parse --short HEAD))
---

# Dokumentasi Fraud Detection Platform v2

> [!info] Salinan read-only dari \`docs/\` di repo. Edit di repo, lalu jalankan \`docs/sync-to-vault.sh\`.

- [[README|Index dokumentasi]] · [[backlog]] · [[roadmap]] · [[CHANGELOG]]
- PRD/TRD: [[prd-agent-ai-console|PRD Multi-Agent & AI Console]] · [[trd-agent-ai-console|TRD AI Console]]
- Ide & spec awal: [[00 Index Pengembangan]]
EOF
echo "synced $DOCS_DIR -> $HOST:$VAULT_DIR"
