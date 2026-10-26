#!/bin/sh
# Checks that a commit changing an English document also changes its
# Japanese translation (docs/adr/0008).
#
#   check-translations.sh staged <message file>   the staged changes (commit-msg hook)
#   check-translations.sh range <base> <head>     each non-merge commit of base..head (CI)
#
# A commit whose last paragraph has the trailer
# "Translation-Unchanged: <reason>" with a non-empty reason may leave the
# translation as it is. Exit status: 0 no violation, 1 violations, 64 usage.
set -u

pairs="README.md:README.ja.md CONTRIBUTING.md:CONTRIBUTING.ja.md"

usage() {
  echo "usage: check-translations.sh staged <message file> | range <base> <head>" >&2
  exit 64
}

# The last paragraph of a message, without comment lines.
last_paragraph() {
  grep -v '^#' | awk '
    /^[[:space:]]*$/ { if (n > 0) { para = ""; n = 0 } ; next }
    { para = para $0 "\n"; n++ }
    END { printf "%s", para }'
}

# Whether the message on standard input allows an unchanged translation.
allows_unchanged() {
  last_paragraph | grep -i -q '^Translation-Unchanged:[[:space:]]*[^[:space:]]'
}

# check <label> <changed files> <message>: prints the violations; returns 1 if any.
check() {
  label=$1
  files=$2
  message=$3
  found=0
  if printf '%s\n' "$message" | allows_unchanged; then
    return 0
  fi
  for pair in $pairs; do
    en=${pair%%:*}
    ja=${pair#*:}
    if printf '%s\n' "$files" | grep -q -x -F "$en" &&
      ! printf '%s\n' "$files" | grep -q -x -F "$ja"; then
      echo "check-translations: $label: $en changed without $ja (add Translation-Unchanged: <reason> if the translation is not affected)"
      found=1
    fi
  done
  return "$found"
}

[ $# -ge 1 ] || usage
case $1 in
  staged)
    [ $# -eq 2 ] || usage
    files=$(git diff --cached --name-only) || exit 1
    message=$(cat "$2") || exit 1
    check staged "$files" "$message" || exit 1
    ;;
  range)
    [ $# -eq 3 ] || usage
    base=$2
    head=$3
    if git cat-file -e "$base^{commit}" 2>/dev/null; then
      commits=$(git rev-list --no-merges --reverse "$base..$head") || exit 1
    else
      commits=$(git rev-list --no-merges --reverse "$head") || exit 1
    fi
    status=0
    for c in $commits; do
      files=$(git diff-tree --root --no-commit-id --name-only -r "$c")
      message=$(git log -1 --format=%B "$c")
      check "$c" "$files" "$message" || status=1
    done
    exit "$status"
    ;;
  *)
    usage
    ;;
esac
