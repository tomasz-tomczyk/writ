#!/usr/bin/env bash
# Spec section 11, from crit #873: the README and every SKILL.md are
# grepped against `writ --help`.
#
# A flag in the documentation that the binary rejects is a bug, not a
# typo: it is the failure mode that made invariant 8 an invariant. This
# script is the only thing that notices, so it runs in CI.
set -euo pipefail

writ=${WRIT:-target/debug/writ}
if [ ! -x "$writ" ]; then
  echo "check-doc-flags: no writ binary at $writ. Build it, or set WRIT." >&2
  exit 1
fi

# Every long flag the binary accepts anywhere: the top level, plus each
# subcommand it lists. `help` is skipped because it takes none.
known=$(mktemp)
trap 'rm -f "$known"' EXIT

"$writ" --help | grep -oE -- '--[a-z][a-z0-9-]*' >>"$known"
commands=$("$writ" --help |
  awk '/^Commands:/{on=1; next} /^Options:/{on=0}
       on && /^  [a-z]/ && $1 != "help" {print $1}')
for command in $commands; do
  "$writ" "$command" --help | grep -oE -- '--[a-z][a-z0-9-]*' >>"$known"
done
sort -u -o "$known" "$known"

docs=$(find . -name README.md -o -name SKILL.md |
  grep -v '/target/' | grep -v '/\.worktrees/' | sort)
if [ -z "$docs" ]; then
  echo "check-doc-flags: no README.md or SKILL.md found. Nothing was checked." >&2
  exit 1
fi

# A flag belongs to the nearest command word on its line. A line that
# runs some other program mentions its flags, not writ's, so
# `cargo install --path` is not a claim about writ. A line with no
# command word at all -- a table row, a sentence -- is a claim about
# writ, because these documents are about writ.
# shellcheck disable=SC2086
awk '
  FILENAME != last { last = FILENAME }
  {
    line = $0
    stripped = line
    sub(/^[ \t]*[-*][ \t]+/, "", stripped)
    gsub(/^[|>$ \t`]+/, "", stripped)
    split(stripped, words, /[ \t`]/)
    first = words[1]
    if (first ~ /^[a-z][a-z0-9_.\/-]*$/ && first != "writ") next
    while (match(line, /--[a-z][a-z0-9-]*/)) {
      print FILENAME "\t" substr(line, RSTART, RLENGTH)
      line = substr(line, RSTART + RLENGTH)
    }
  }
' $docs | sort -u >"$known.used"

status=0
while IFS=$'\t' read -r doc flag; do
  if ! grep -qxF -- "$flag" "$known"; then
    echo "$doc: $flag is documented and writ does not accept it" >&2
    status=1
  fi
done <"$known.used"
rm -f "$known.used"

if [ "$status" -eq 0 ]; then
  count=$(echo "$docs" | wc -l | tr -d ' ')
  echo "check-doc-flags: every flag in $count document(s) is a real writ flag."
fi
exit "$status"
