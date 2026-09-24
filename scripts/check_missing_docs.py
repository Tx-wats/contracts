#!/usr/bin/env python3
"""
scripts/check_missing_docs.py — Fail on `missing_docs` warnings in hand-written code.

Reads `cargo check --message-format=json` output (built with
`-W missing_docs`) on stdin. Soroban's `#[contract]`, `#[contractimpl]`,
`#[contracttype]` and `#[contracterror]` macros (and the `contractspecfn` /
`contractclient` / `contractargs` macros `#[contractimpl]` expands to)
generate public items — spec statics, `spec_xdr_*` functions, client fields —
that carry no docs and that no attribute on our side can reach, so
diagnostics originating inside those expansions are ignored. An undocumented
hand-written item is still reported at its own span. Every other
`missing_docs` warning is an error.
"""

import json
import sys


def from_soroban_macro(span: dict) -> bool:
    expansion = span.get("expansion")
    while expansion:
        name = expansion.get("macro_decl_name", "").replace("soroban_sdk::", "")
        if name.startswith("#[contract"):
            return True
        expansion = expansion.get("span", {}).get("expansion")
    return False


def main():
    failures = []
    for line in sys.stdin:
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        # RUSTFLAGS also reaches build.rs, which is not part of the documented API.
        if "custom-build" in msg.get("target", {}).get("kind", []):
            continue
        diag = msg["message"]
        if (diag.get("code") or {}).get("code") != "missing_docs":
            continue
        spans = diag.get("spans", [])
        if any(from_soroban_macro(s) for s in spans):
            continue
        primary = next((s for s in spans if s.get("is_primary")), spans[0] if spans else {})
        failures.append(
            f"{primary.get('file_name', '?')}:{primary.get('line_start', '?')}: {diag['message']}"
        )

    for f in sorted(set(failures)):
        print(f"❌ {f}")
    if failures:
        print(f"\n{len(set(failures))} public item(s) without documentation.")
        sys.exit(1)
    print("✅ every public item is documented")


if __name__ == "__main__":
    main()
