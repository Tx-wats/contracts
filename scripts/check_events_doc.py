#!/usr/bin/env python3
"""
scripts/check_events_doc.py — Verify that docs/events.md accurately reflects
on-chain event emissions in contract source code.

Checks:
1. Every event emitted in contract source is documented in docs/events.md under the
   corresponding contract section.
2. Every event emitted in contract source is marked as implemented (✅) in docs/events.md.
3. Every doc entry marked as implemented (✅) actually exists in contract source code.
4. Planned events (🔲) must not be emitted in source code without being updated to implemented.
"""

import os
import re
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
DOC_PATH = os.path.join(REPO_ROOT, "docs", "events.md")

CONTRACTS = {
    "AlertRegistry": os.path.join(REPO_ROOT, "contracts", "alert-registry", "src", "lib.rs"),
    "WatcherRegistry": os.path.join(REPO_ROOT, "contracts", "watcher-registry", "src", "lib.rs"),
}


def extract_source_events(file_path: str) -> set:
    """Extract (category, action) pairs published in contract source (excluding tests)."""
    with open(file_path, "r", encoding="utf-8") as f:
        content = f.read()

    # Strip test module (from `mod tests {` or `#[cfg(test)]\nmod tests` onwards)
    test_split = re.split(r"(?:#\[cfg\(test\)\]\s*)?mod\s+tests\s*\{", content)
    prod_code = test_split[0]

    # Match publish calls:
    # env.events().publish((symbol_short!("cat"), symbol_short!("act")), ...);
    # or .publish((symbol_short!("cat"), symbol_short!("act")), ...);
    pattern = re.compile(
        r'publish\s*\(\s*\(\s*symbol_short!\(\s*"([^"]+)"\s*\)\s*,\s*symbol_short!\(\s*"([^"]+)"\s*\)\s*\)',
        re.DOTALL,
    )

    events = set()
    for match in pattern.finditer(prod_code):
        cat, act = match.group(1), match.group(2)
        events.add(f"{cat}.{act}")

    return events


def parse_doc_events(doc_path: str) -> dict:
    """Parse docs/events.md into contract sections with their events and status."""
    with open(doc_path, "r", encoding="utf-8") as f:
        lines = f.readlines()

    doc_data = {"AlertRegistry": {}, "WatcherRegistry": {}}
    current_contract = None
    current_event = None

    for line in lines:
        line_str = line.strip()
        if line_str.startswith("## "):
            contract_heading = line_str.replace("## ", "").strip()
            if "AlertRegistry" in contract_heading:
                current_contract = "AlertRegistry"
            elif "WatcherRegistry" in contract_heading:
                current_contract = "WatcherRegistry"
            else:
                current_contract = None
            current_event = None

        elif line_str.startswith("### ") and current_contract:
            # e.g., "### `alert.register`" or "### alert.register"
            event_name = line_str.replace("### ", "").replace("`", "").strip()
            if "." in event_name:
                current_event = event_name
                doc_data[current_contract][current_event] = {"status": "unknown"}

        elif current_contract and current_event and line_str.startswith("**Status:**"):
            if "✅" in line_str or "implemented" in line_str.lower():
                doc_data[current_contract][current_event]["status"] = "implemented"
            elif "🔲" in line_str or "planned" in line_str.lower():
                doc_data[current_contract][current_event]["status"] = "planned"

    return doc_data


def main():
    print("Checking event emissions in source code against docs/events.md...")
    doc_data = parse_doc_events(DOC_PATH)
    has_error = False

    for contract_name, source_file in CONTRACTS.items():
        print(f"\n--- Checking {contract_name} ({os.path.relpath(source_file, REPO_ROOT)}) ---")
        if not os.path.exists(source_file):
            print(f"ERROR: Source file not found: {source_file}")
            sys.exit(1)

        source_events = extract_source_events(source_file)
        doc_events = doc_data.get(contract_name, {})

        print(f"Source emitted events: {sorted(list(source_events))}")
        doc_impl = {k for k, v in doc_events.items() if v["status"] == "implemented"}
        doc_plan = {k for k, v in doc_events.items() if v["status"] == "planned"}
        print(f"Doc implemented (✅):   {sorted(list(doc_impl))}")
        print(f"Doc planned (🔲):       {sorted(list(doc_plan))}")

        # Check 1: Source events missing completely from documentation
        undocumented = source_events - set(doc_events.keys())
        if undocumented:
            print(f"❌ ERROR: Events emitted in {contract_name} source but completely UNDOCUMENTED in docs/events.md:")
            for e in sorted(undocumented):
                print(f"   - {e}")
            has_error = True

        # Check 2: Source events documented as planned instead of implemented
        false_planned = source_events.intersection(doc_plan)
        if false_planned:
            print(f"❌ ERROR: Events emitted in {contract_name} source but marked as planned (🔲) in docs/events.md:")
            for e in sorted(false_planned):
                print(f"   - {e}")
            has_error = True

        # Check 3: Doc entries marked implemented (✅) but not emitted in source
        phantom_implemented = doc_impl - source_events
        if phantom_implemented:
            print(f"❌ ERROR: Events marked implemented (✅) in docs/events.md but NOT emitted in {contract_name} source:")
            for e in sorted(phantom_implemented):
                print(f"   - {e}")
            has_error = True

        if not undocumented and not false_planned and not phantom_implemented:
            print(f"✅ {contract_name} event documentation is in sync with source code.")

    if has_error:
        print("\n❌ Event documentation check failed! Please update docs/events.md to match source emissions.")
        sys.exit(1)
    else:
        print("\n✅ All event documentation matches contract source emissions perfectly.")
        sys.exit(0)


if __name__ == "__main__":
    main()
