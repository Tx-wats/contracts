#!/usr/bin/env bash
# scripts/check-events-doc.sh — Check docs/events.md against contract event emissions
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
python3 "${SCRIPT_DIR}/check_events_doc.py" "$@"
