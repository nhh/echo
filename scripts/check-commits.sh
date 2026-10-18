#!/usr/bin/env bash
# Prueft Commit-Nachrichten auf Conventional Commits und den Ticket-Footer
# (CLAUDE.md 7). Aufruf: scripts/check-commits.sh <von>..<bis>
# Ohne Bereich prueft es alles, was noch nicht auf origin/main liegt.
set -euo pipefail

range="${1:-origin/main..HEAD}"
types='feat|fix|refactor|perf|test|docs|build|ci|chore|style'
scopes='protocol|storage|core|audio|video|session|server|ui|client|board|build|ci|deps'
header_pattern="^(${types})(\((${scopes})\))?!?: [a-z0-9\`].{0,69}[^.]$"
# Tickets im YouTrack-Projekt echo; Beitraege von aussen nennen ihr GitHub-Issue.
refs_pattern='^Refs: (echo-[0-9]+|#[0-9]+)(, (echo-[0-9]+|#[0-9]+))*$'

failed=0
while read -r sha; do
  author=$(git log -1 --format='%ae' "$sha")
  # Dependabot schreibt eigene Nachrichten ohne Ticket.
  [[ "$author" == *dependabot* ]] && continue

  subject=$(git log -1 --format='%s' "$sha")
  body=$(git log -1 --format='%b' "$sha")
  if ! grep -Eq "$header_pattern" <<<"$subject"; then
    echo "::error::${sha:0:7}: Kopfzeile ist kein Conventional Commit: $subject"
    failed=1
  fi
  if ! grep -Eq "$refs_pattern" <<<"$body"; then
    echo "::error::${sha:0:7}: Footer 'Refs: echo-<n>' fehlt: $subject"
    failed=1
  fi
done < <(git rev-list --no-merges "$range")

exit "$failed"
