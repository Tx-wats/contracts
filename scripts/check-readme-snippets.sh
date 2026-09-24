#!/usr/bin/env bash
# scripts/check-readme-snippets.sh — Check README contract calls against the compiled contract spec
#
# Requires the release WASM (cargo build --release --target wasm32-unknown-unknown
# -p alert-registry -p watcher-registry) and the `stellar` CLI on PATH.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
WASM_DIR="${REPO_ROOT}/target/wasm32-unknown-unknown/release"

SPEC_DIR="$(mktemp -d)"
trap 'rm -rf "${SPEC_DIR}"' EXIT

for contract in alert-registry watcher-registry; do
  wasm="${WASM_DIR}/${contract//-/_}.wasm"
  if [[ ! -f "${wasm}" ]]; then
    echo "error: ${wasm} not found — build the release WASM first" >&2
    exit 1
  fi
  stellar contract info interface --wasm "${wasm}" --output json > "${SPEC_DIR}/${contract}.json"
done

python3 "${SCRIPT_DIR}/check_readme_snippets.py" \
  --spec "alert-registry=${SPEC_DIR}/alert-registry.json" \
  --spec "watcher-registry=${SPEC_DIR}/watcher-registry.json" \
  "$@"
