#!/usr/bin/env bash
# scripts/check-changelog-release.sh — Fail unless CHANGELOG.md documents the release being cut
#
# Usage: scripts/check-changelog-release.sh vX.Y.Z
#
# Requires, for version X.Y.Z:
#   1. a section header `## [X.Y.Z] - YYYY-MM-DD`,
#   2. at least one entry (a `- ` bullet) in that section, and
#   3. a link reference `[X.Y.Z]: https://...` (compare or release link).
# Run by every release-triggered workflow before it deploys or publishes anything.
set -euo pipefail

TAG="${1:?usage: $0 vX.Y.Z}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHANGELOG="${CHANGELOG:-${SCRIPT_DIR}/../CHANGELOG.md}"

if [[ ! "${TAG}" =~ ^v([0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?)$ ]]; then
  echo "error: tag '${TAG}' is not of the form vX.Y.Z" >&2
  exit 1
fi
VERSION="${BASH_REMATCH[1]}"
VERSION_RE="${VERSION//./\\.}"

fail() {
  echo "error: CHANGELOG.md has no release notes for ${TAG}: $1" >&2
  echo "Move the shipped entries out of [Unreleased] into '## [${VERSION}] - YYYY-MM-DD'" >&2
  echo "and add a '[${VERSION}]: <compare link>' reference before tagging." >&2
  exit 1
}

grep -Eq "^## \[${VERSION_RE}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}\s*$" "${CHANGELOG}" \
  || fail "missing '## [${VERSION}] - YYYY-MM-DD' section header"

entries=$(awk -v hdr="## [${VERSION}] - " '
  index($0, hdr) == 1 { inside = 1; next }
  inside && /^## / { exit }
  inside && /^[[:space:]]*- / { n++ }
  END { print n + 0 }
' "${CHANGELOG}")
[[ "${entries}" -gt 0 ]] || fail "the [${VERSION}] section has no entries"

grep -Eq "^\[${VERSION_RE}\]: https?://" "${CHANGELOG}" \
  || fail "missing '[${VERSION}]: <link>' reference at the bottom of the file"

echo "✅ CHANGELOG.md documents ${TAG} (${entries} entries)"
