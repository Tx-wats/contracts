#!/usr/bin/env bash
# scripts/lib/common.sh — Helpers shared by deploy.sh, verify.sh and upgrade.sh.
# Source it, don't run it:  source "$(dirname "${BASH_SOURCE[0]}")/lib/common.sh"

# All contract crates in the workspace that ship as WASM.
# shellcheck disable=SC2034  # read by the scripts that source this file
ALL_CONTRACTS=(alert-registry watcher-registry)

WASM_DIR="target/wasm32-unknown-unknown/release"

# Parse the flags common to the scripts, setting NETWORK, CONTRACT and
# CONTRACT_ID. NETWORK defaults to $NETWORK or testnet. Exits on unknown flags.
parse_args() {
  NETWORK="${NETWORK:-testnet}"
  CONTRACT="${CONTRACT:-}"
  CONTRACT_ID="${CONTRACT_ID:-}"
  NO_GATING="${NO_GATING:-false}"
  ASSUME_YES="${ASSUME_YES:-false}"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --network)     NETWORK="${2:?--network needs a value}";         shift 2 ;;
      --contract)    CONTRACT="${2:?--contract needs a value}";       shift 2 ;;
      --contract-id) CONTRACT_ID="${2:?--contract-id needs a value}"; shift 2 ;;
      --no-gating)   NO_GATING=true;                                  shift 1 ;;
      --yes|-y)      ASSUME_YES=true;                                 shift 1 ;;
      *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
  done
  export NETWORK CONTRACT CONTRACT_ID NO_GATING ASSUME_YES
}

# Exit unless --contract and --contract-id were both given, and --contract
# names a known crate.
require_contract_args() {
  local names
  names="$(IFS='|'; echo "${ALL_CONTRACTS[*]}")"
  [[ -z "$CONTRACT" ]] && { echo "Error: --contract required ($names)" >&2; exit 1; }
  [[ -z "$CONTRACT_ID" ]] && { echo "Error: --contract-id required" >&2; exit 1; }
  case " ${ALL_CONTRACTS[*]} " in
    *" $CONTRACT "*) ;;
    *) echo "Error: unknown contract '$CONTRACT' (expected one of: ${ALL_CONTRACTS[*]})" >&2; exit 1 ;;
  esac
}

# Set RPC_URL and NETWORK_PASSPHRASE for $NETWORK (testnet or mainnet).
# Mainnet has no public default RPC, so MAINNET_RPC_URL must be set.
resolve_network() {
  case "$NETWORK" in
    testnet)
      RPC_URL="https://soroban-testnet.stellar.org"
      NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
      ;;
    mainnet)
      RPC_URL="${MAINNET_RPC_URL:?MAINNET_RPC_URL must be set for mainnet}"
      NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
      ;;
    *)
      echo "Unsupported network: $NETWORK (use testnet or mainnet)" >&2; exit 1 ;;
  esac
  export RPC_URL NETWORK_PASSPHRASE
}

# Path of the optimized WASM for a crate name, e.g. alert-registry ->
# target/wasm32-unknown-unknown/release/alert_registry.optimized.wasm
wasm_path() {
  echo "$WASM_DIR/${1//-/_}.optimized.wasm"
}

# Build and optimize the given contract crates (defaults to all of them).
#
# Only the contract crates are built: building the whole workspace for wasm32
# pulls in test-utils, which force-enables soroban-sdk's std-only `testutils`
# feature. --locked keeps the build reproducible, which verify.sh relies on.
#
# Rust 1.82+ emits the WebAssembly reference-types proposal, which the Soroban
# host rejects at upload ("reference-types not enabled"). `stellar contract
# optimize` runs wasm-opt, which lowers the module back into the accepted
# subset — so this step is required for deployability, not just size. Every
# script uses the optimized artifact so on-chain hashes match local builds.
build_contract() {
  local crates=("$@")
  [[ ${#crates[@]} -eq 0 ]] && crates=("${ALL_CONTRACTS[@]}")

  local pkg_args=()
  local c
  for c in "${crates[@]}"; do
    pkg_args+=(-p "$c")
  done

  echo "==> Building ${crates[*]}..."
  cargo build --release --target wasm32-unknown-unknown --locked "${pkg_args[@]}"

  echo "==> Optimizing WASM for on-chain upload..."
  for c in "${crates[@]}"; do
    stellar contract optimize --wasm "$WASM_DIR/${c//-/_}.wasm"
  done
}
