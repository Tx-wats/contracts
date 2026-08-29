#!/usr/bin/env bash
# scripts/run-mutants.sh — Run mutation testing with cargo-mutants on TxWatch contracts
set -euo pipefail

echo "======================================================="
echo "Running Mutation Testing on TxWatch Smart Contracts"
echo "======================================================="

# Verify cargo-mutants is installed
if ! command -v cargo-mutants &> /dev/null; then
    echo "cargo-mutants is not installed. Installing locked version..."
    cargo install cargo-mutants --version 24.7.1 --locked
fi

# Run mutants in-place for fast incremental compilation
PACKAGE="${1:-all}"

case "$PACKAGE" in
    "watcher-registry")
        echo "--> Testing watcher-registry mutations..."
        cargo mutants --in-place -p watcher-registry
        ;;
    "alert-registry")
        echo "--> Testing alert-registry mutations..."
        cargo mutants --in-place -p alert-registry
        ;;
    "all")
        echo "--> Testing watcher-registry mutations..."
        cargo mutants --in-place -p watcher-registry
        echo "--> Testing alert-registry mutations..."
        cargo mutants --in-place -p alert-registry
        ;;
    *)
        echo "Usage: $0 [watcher-registry|alert-registry|all]"
        exit 1
        ;;
esac

echo "======================================================="
echo "Mutation testing complete. Check mutants.out/ for summary."
echo "======================================================="
