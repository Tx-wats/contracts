#!/usr/bin/env python3
"""
scripts/check_function_docs.py — Verify that every public entrypoint in
`contracts/<crate>/spec.golden.json` is documented in its reference doc.

Checks:
1. Every `function_v0` entry in a contract's spec golden file has its name
   mentioned in backticks (`name`, `name()` or `Contract::name`) somewhere in
   the "## Functions" section of that contract's doc.
"""

import json
import os
import re
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

CONTRACTS = {
    "AlertRegistry": {
        "spec": os.path.join(REPO_ROOT, "contracts", "alert-registry", "spec.golden.json"),
        "doc": os.path.join(REPO_ROOT, "docs", "alert-registry.md"),
    },
    "WatcherRegistry": {
        "spec": os.path.join(REPO_ROOT, "contracts", "watcher-registry", "spec.golden.json"),
        "doc": os.path.join(REPO_ROOT, "docs", "watcher-registry.md"),
    },
}


def extract_entrypoints(spec_path: str) -> set:
    """Return the names of the public functions in a contract spec golden file."""
    with open(spec_path, "r", encoding="utf-8") as f:
        entries = json.load(f)
    return {entry["function_v0"]["name"] for entry in entries if "function_v0" in entry}


def functions_section(doc_path: str) -> str:
    """Return the text of the doc's "## Functions" section (up to the next "## ")."""
    with open(doc_path, "r", encoding="utf-8") as f:
        lines = f.read().splitlines()

    section = []
    in_section = False
    for line in lines:
        if line.startswith("## "):
            in_section = line[3:].strip() == "Functions"
            continue
        if in_section:
            section.append(line)
    return "\n".join(section)


def is_documented(section: str, name: str) -> bool:
    """True if `name` appears as a code span, optionally qualified or followed by ()."""
    pattern = rf"`(?:[A-Za-z_]\w*::)?{re.escape(name)}(?:\(\))?`"
    return re.search(pattern, section) is not None


def main():
    print("Checking public entrypoints against the reference docs...")
    has_error = False

    for contract_name, paths in CONTRACTS.items():
        print(f"\n--- Checking {contract_name} ({os.path.relpath(paths['spec'], REPO_ROOT)}) ---")
        if not os.path.exists(paths["spec"]):
            print(f"ERROR: Spec file not found: {paths['spec']}")
            sys.exit(1)
        if not os.path.exists(paths["doc"]):
            print(f"ERROR: Doc file not found: {paths['doc']}")
            sys.exit(1)

        entrypoints = extract_entrypoints(paths["spec"])
        section = functions_section(paths["doc"])
        undocumented = sorted(name for name in entrypoints if not is_documented(section, name))

        if undocumented:
            print(f"❌ ERROR: entrypoints in {contract_name} missing from {os.path.relpath(paths['doc'], REPO_ROOT)}:")
            for name in undocumented:
                print(f"   - {name}")
            has_error = True
        else:
            print(f"✅ all {len(entrypoints)} {contract_name} entrypoints are documented.")

    if has_error:
        print("\n❌ Function documentation check failed! Add the missing entrypoints to the Functions section of the doc.")
        sys.exit(1)
    else:
        print("\n✅ Every public entrypoint is documented.")
        sys.exit(0)


if __name__ == "__main__":
    main()
