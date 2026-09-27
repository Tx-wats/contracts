#!/usr/bin/env bash
# scripts/test.sh — Run all contract tests with consistent dependency resolution
#
# This script runs `cargo test --workspace --locked` to match the behavior of CI
# and `make test`, ensuring all tests are executed with the same dependency
# versions. Additional arguments are passed through to cargo.
#
# Usage:
#   ./scripts/test.sh                   # Run all tests
#   ./scripts/test.sh --package foo     # Run tests for a specific package
#   ./scripts/test.sh -- --test-threads=1  # Pass flags to test binary
#
set -euo pipefail
cargo test --workspace --locked "$@"
