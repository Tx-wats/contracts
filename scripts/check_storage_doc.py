#!/usr/bin/env python3
"""
scripts/check_storage_doc.py — Verify that docs/storage.md covers every
`DataKey` variant declared in contract source code.

Checks:
1. Every `DataKey` variant in a contract's enum appears as `DataKey::<Variant>`
   in that contract's section of docs/storage.md.
2. Every `DataKey` variant appears by name in a row of that contract's
   "Storage Tier Summary" entry.
3. Every `instance_key` constant (e.g. `GUARDIAN`, `SCHEMAV`) declared in a
   contract appears as `instance_key::<NAME>` in that contract's section of
   docs/storage.md.
4. Every `instance_key` constant appears by name in a row of that contract's
   "Storage Tier Summary" entry.
"""

import os
import re
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
DOC_PATH = os.path.join(REPO_ROOT, "docs", "storage.md")

CONTRACTS = {
    "AlertRegistry": os.path.join(REPO_ROOT, "contracts", "alert-registry", "src", "lib.rs"),
    "WatcherRegistry": os.path.join(REPO_ROOT, "contracts", "watcher-registry", "src", "lib.rs"),
}

# Variants that exist in the enum but are never stored, so they have no
# tier-summary row. They must still appear in the key table.
UNSTORED_VARIANTS = {
    "AlertRegistry": {"NextId"},
}


def extract_data_keys(file_path: str) -> set:
    """Extract the variant names of the `DataKey` enum in contract source."""
    with open(file_path, "r", encoding="utf-8") as f:
        content = f.read()

    match = re.search(r"pub enum DataKey\s*\{(.*?)\n\}", content, re.DOTALL)
    if not match:
        return set()

    body = re.sub(r"///[^\n]*", "", match.group(1))
    body = re.sub(r"\([^()]*\)", "", body)
    return set(re.findall(r"^\s*([A-Z][A-Za-z0-9]*)\s*,", body, re.MULTILINE))


def extract_instance_keys(file_path: str) -> set:
    """Extract the names of `instance_key` constants in contract source."""
    with open(file_path, "r", encoding="utf-8") as f:
        content = f.read()

    # Matches e.g. `const GUARDIAN: Symbol = instance_key("GUARDIAN");`
    return set(
        re.findall(
            r"const\s+([A-Z][A-Za-z0-9_]*)\s*:[^=]*=\s*instance_key\s*\(",
            content,
        )
    )


def parse_doc_sections(doc_path: str) -> tuple:
    """Return (contract -> section text, contract -> summary table rows)."""
    with open(doc_path, "r", encoding="utf-8") as f:
        lines = f.read().splitlines()

    sections = {name: [] for name in CONTRACTS}
    summary_rows = {name: [] for name in CONTRACTS}
    current = None
    in_summary = False

    for line in lines:
        if line.startswith("## "):
            heading = line[3:]
            in_summary = heading.strip() == "Storage Tier Summary"
            current = next((name for name in CONTRACTS if name in heading), None)
            continue
        if in_summary:
            for name in CONTRACTS:
                if line.startswith(f"| {name} |"):
                    summary_rows[name].append(line)
        elif current:
            sections[current].append(line)

    return ({k: "\n".join(v) for k, v in sections.items()}, summary_rows)


def main():
    print("Checking DataKey variants against docs/storage.md...")
    sections, summary_rows = parse_doc_sections(DOC_PATH)
    has_error = False

    for contract_name, source_file in CONTRACTS.items():
        print(f"\n--- Checking {contract_name} ({os.path.relpath(source_file, REPO_ROOT)}) ---")
        if not os.path.exists(source_file):
            print(f"ERROR: Source file not found: {source_file}")
            sys.exit(1)

        variants = extract_data_keys(source_file)
        print(f"DataKey variants: {sorted(variants)}")

        # Check 1: variant missing from the contract's key table
        undocumented = {v for v in variants if f"DataKey::{v}" not in sections[contract_name]}
        if undocumented:
            print(f"❌ ERROR: DataKey variants in {contract_name} missing from docs/storage.md key table:")
            for v in sorted(undocumented):
                print(f"   - DataKey::{v}")
            has_error = True

        # Check 2: variant missing from the tier summary
        summary = "\n".join(summary_rows[contract_name])
        unsummarised = {
            v
            for v in variants - UNSTORED_VARIANTS.get(contract_name, set())
            if f"`{v}`" not in summary
        }
        if unsummarised:
            print(f"❌ ERROR: DataKey variants in {contract_name} missing from the Storage Tier Summary:")
            for v in sorted(unsummarised):
                print(f"   - {v}")
            has_error = True

        # Check 3: instance_key constant missing from the contract's key table
        instance_keys = extract_instance_keys(source_file)
        print(f"instance_key constants: {sorted(instance_keys)}")

        undocumented_keys = {
            k for k in instance_keys if f"instance_key::{k}" not in sections[contract_name]
        }
        if undocumented_keys:
            print(f"❌ ERROR: instance_key constants in {contract_name} missing from docs/storage.md key table:")
            for k in sorted(undocumented_keys):
                print(f"   - instance_key::{k}")
            has_error = True

        # Check 4: instance_key constant missing from the tier summary
        unsummarised_keys = {k for k in instance_keys if f"`{k}`" not in summary}
        if unsummarised_keys:
            print(f"❌ ERROR: instance_key constants in {contract_name} missing from the Storage Tier Summary:")
            for k in sorted(unsummarised_keys):
                print(f"   - {k}")
            has_error = True

        if not undocumented and not unsummarised and not undocumented_keys and not unsummarised_keys:
            print(f"✅ {contract_name} storage documentation is in sync with DataKey and instance_key.")

    if has_error:
        print("\n❌ Storage documentation check failed! Please update docs/storage.md to match DataKey and instance_key.")
        sys.exit(1)
    else:
        print("\n✅ All DataKey variants and instance_key constants are documented in docs/storage.md.")
        sys.exit(0)


if __name__ == "__main__":
    main()
