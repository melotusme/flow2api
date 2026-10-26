#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
# Keep Python bytecode local instead of waiting for iCloud placeholders.
export PYTHONPYCACHEPREFIX="${PYTHONPYCACHEPREFIX:-$HOME/Library/Caches/flow2api/pycache}"
exec ./venv/bin/python -u main.py
