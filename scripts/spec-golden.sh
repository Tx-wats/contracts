#!/usr/bin/env bash
# scripts/spec-golden.sh — Check or update the contract spec golden files
#
# Reads the built (optimized) WASM for each contract and extracts the contract
# spec — the public function names, signatures, types and event discriminants —
# using `stellar contract info interface --wasm <file> --output json`.
#
# The output must match the committed `contracts/<crate>/spec.golden.json`.
# Unexpected diffs fail the check so that renaming or removing an entrypoint
# (which would silently break every generated binding) is caught in review.
#
# Usage:
#   bash scripts/spec-golden.sh            # fail on unreviewed diffs
#   bash scripts/spec-golden.sh --update   # overwrite committed golden files
#
# Both the WASM and the optimized artifact must be present:
#   cargo build --release --target wasm32-unknown-unknown --locked -p alert-registry -p watcher-registry
#   for crate in alert-registry watcher-registry; do
#     stellar contract optimize --wasm target/wasm32-unknown-unknown/release/${crate//-/_}.wasm
#   done
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# shellcheck source=scripts/lib/common.sh
source "${SCRIPT_DIR}/lib/common.sh"

UPDATE=false
[[ "${1:-}" == "--update" ]] && UPDATE=true

WASM_DIR="${REPO_ROOT}/target/wasm32-unknown-unknown/release"
SPEC_DIR="${REPO_ROOT}/contracts"

FAIL=0
TMP_FILES=()
trap 'rm -f "${TMP_FILES[@]}" 2>/dev/null' EXIT

for crate in "${ALL_CONTRACTS[@]}"; do
  wasm="${WASM_DIR}/${crate//-/_}.optimized.wasm"

  if [[ ! -f "${wasm}" ]]; then
    echo "error: ${crate}: ${wasm} not found — build and optimize the release WASM first" >&2
    exit 1
  fi

  tmp_spec="$(mktemp "${REPO_ROOT}/.tmp-spec-XXXXXXXX.json")"
  TMP_FILES+=("${tmp_spec}" "${tmp_spec}.diff")

  stellar contract info interface --wasm "${wasm}" --output json > "${tmp_spec}"

  golden="${SPEC_DIR}/${crate}/spec.golden.json"

  if [[ ! -f "${golden}" ]]; then
    echo "error: ${crate}: ${golden} missing — run with --update once to establish it" >&2
    exit 1
  fi

  if ! diff -u "${golden}" "${tmp_spec}" > "${tmp_spec}.diff" 2>&1; then
    if [[ "${UPDATE}" == true ]]; then
      mv "${tmp_spec}" "${golden}"
    else
      echo "error: ${crate}: contract spec drifted from committed spec.golden.json" >&2
      echo "  Rebuild and re-optimize the release WASM, then run:" >&2
      echo "    bash scripts/spec-golden.sh --update" >&2
      echo "  or review the diff above and mark it as intentional." >&2
      cat "${tmp_spec}.diff" >&2
      FAIL=1
    fi
  fi
done

if [[ "${FAIL}" -ne 0 ]]; then
  exit 1
fi

if [[ "${UPDATE}" == true ]]; then
  echo "Updated spec.golden.json for ${ALL_CONTRACTS[*]}."
else
  echo "Spec golden files match the contracted WASM."
fi
