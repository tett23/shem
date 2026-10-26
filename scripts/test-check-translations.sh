#!/bin/sh
# Tests of scripts/check-translations.sh (docs/adr/0008).
# Each case makes a temporary git repository and checks the exit status
# and the message of the check.
set -u

script=$(cd "$(dirname "$0")" && pwd)/check-translations.sh
work=$(mktemp -d "${TMPDIR:-/tmp}/check-translations.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
failures=0
cases=0

fail() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# A new repository with all four documents committed.
new_repo() {
  repo="$work/$1"
  mkdir -p "$repo"
  git -C "$repo" init -q -b main
  git -C "$repo" config user.name test
  git -C "$repo" config user.email test@example.com
  git -C "$repo" config commit.gpgsign false
  for f in README.md README.ja.md CONTRIBUTING.md CONTRIBUTING.ja.md; do
    echo "$f" > "$repo/$f"
  done
  git -C "$repo" add .
  git -C "$repo" commit -q -m "Initial commit"
}

# Makes a commit that changes the given files, with the message in $msg.
commit_change() {
  for f in "$@"; do
    echo "change" >> "$repo/$f"
  done
  git -C "$repo" add "$@"
  printf '%s\n' "$msg" > "$work/msg"
  git -C "$repo" commit -q --no-verify -F "$work/msg"
}

# Stages a change of the given files, without committing.
stage_change() {
  for f in "$@"; do
    echo "change" >> "$repo/$f"
  done
  git -C "$repo" add "$@"
  printf '%s\n' "$msg" > "$work/msg"
}

# expect <name> <status> <pattern or empty> <command...>
expect() {
  name=$1
  want=$2
  pattern=$3
  shift 3
  cases=$((cases + 1))
  out=$( (cd "$repo" && "$@") 2>&1)
  got=$?
  if [ "$got" -ne "$want" ]; then
    fail "$name: exit status $got, want $want; output: $out"
    return
  fi
  if [ -n "$pattern" ] && ! printf '%s\n' "$out" | grep -q -- "$pattern"; then
    fail "$name: output does not contain '$pattern': $out"
  fi
  if [ -z "$pattern" ] && [ -n "$out" ]; then
    fail "$name: unexpected output: $out"
  fi
}

# The staged check, as the commit-msg hook runs it.
new_repo staged-en-only
msg="Change the README"
stage_change README.md
expect "staged: README.md only" 1 "README.md changed without README.ja.md" sh "$script" staged "$work/msg"

new_repo staged-both
stage_change README.md README.ja.md
expect "staged: README.md and README.ja.md" 0 "" sh "$script" staged "$work/msg"

new_repo staged-ja-only
stage_change README.ja.md
expect "staged: README.ja.md only" 0 "" sh "$script" staged "$work/msg"

new_repo staged-trailer
msg="Fix a link

Translation-Unchanged: fix a link
Co-Authored-By: Someone <someone@example.com>"
stage_change CONTRIBUTING.md
expect "staged: trailer" 0 "" sh "$script" staged "$work/msg"

new_repo staged-trailer-lowercase
msg="Fix a link

translation-unchanged: fix a link"
stage_change CONTRIBUTING.md
expect "staged: trailer key in lower case" 0 "" sh "$script" staged "$work/msg"

new_repo staged-empty-reason
msg="Fix a link

Translation-Unchanged:   "
stage_change CONTRIBUTING.md
expect "staged: empty reason" 1 "CONTRIBUTING.md changed without CONTRIBUTING.ja.md" sh "$script" staged "$work/msg"

new_repo staged-trailer-not-last
msg="Fix a link

Translation-Unchanged: fix a link

Another paragraph"
stage_change CONTRIBUTING.md
expect "staged: trailer not in the last paragraph" 1 "CONTRIBUTING.md changed without" sh "$script" staged "$work/msg"

new_repo staged-wrong-pair
msg="Change two documents"
stage_change README.md CONTRIBUTING.ja.md
expect "staged: README.md with CONTRIBUTING.ja.md" 1 "README.md changed without README.ja.md" sh "$script" staged "$work/msg"

new_repo staged-comments
msg="Change the README
# Translation-Unchanged: a comment line is not a trailer"
stage_change README.md
expect "staged: comment line is ignored" 1 "README.md changed without" sh "$script" staged "$work/msg"

new_repo staged-other
msg="Change the code"
stage_change docs.txt 2>/dev/null || { echo x > "$repo/docs.txt"; git -C "$repo" add docs.txt; }
expect "staged: unrelated file" 0 "" sh "$script" staged "$work/msg"

# The range check, as CI runs it.
new_repo range
base=$(git -C "$repo" rev-parse HEAD)
msg="Good change"
commit_change README.md README.ja.md
msg="Bad change"
commit_change CONTRIBUTING.md
bad=$(git -C "$repo" rev-parse HEAD)
msg="Another good change"
commit_change README.ja.md
head=$(git -C "$repo" rev-parse HEAD)
expect "range: one bad commit" 1 "$bad: CONTRIBUTING.md changed without CONTRIBUTING.ja.md" sh "$script" range "$base" "$head"
count=$( (cd "$repo" && sh "$script" range "$base" "$head") 2>&1 | grep -c 'changed without')
[ "$count" -eq 1 ] || fail "range: $count violations reported, want 1"

new_repo range-merge
base=$(git -C "$repo" rev-parse HEAD)
git -C "$repo" switch -q -c topic
msg="Topic change"
commit_change README.md README.ja.md
git -C "$repo" switch -q main
msg="Main change"
commit_change CONTRIBUTING.ja.md
git -C "$repo" merge -q --no-ff --no-edit topic
head=$(git -C "$repo" rev-parse HEAD)
expect "range: merge commits are not checked" 0 "" sh "$script" range "$base" "$head"

new_repo range-root
msg="Bad change"
commit_change README.md
head=$(git -C "$repo" rev-parse HEAD)
expect "range: a missing base checks from the root" 1 "README.md changed without" sh "$script" range 0000000000000000000000000000000000000000 "$head"

# The commit-msg hook.
new_repo hook
mkdir -p "$repo/.githooks" "$repo/scripts"
cp "$script" "$repo/scripts/check-translations.sh"
cp "$(dirname "$script")/../.githooks/commit-msg" "$repo/.githooks/commit-msg"
git -C "$repo" config core.hooksPath .githooks
echo change >> "$repo/README.md"
git -C "$repo" add README.md
cases=$((cases + 1))
if git -C "$repo" commit -q -m "Bad change" > /dev/null 2>&1; then
  fail "hook: a bad commit was made"
fi
echo change >> "$repo/README.ja.md"
git -C "$repo" add README.ja.md
cases=$((cases + 1))
if ! git -C "$repo" commit -q -m "Good change" > /dev/null 2>&1; then
  fail "hook: a good commit was refused"
fi

# Usage errors.
expect "usage: no arguments" 64 "usage" sh "$script"
expect "usage: unknown mode" 64 "usage" sh "$script" unknown

if [ "$failures" -ne 0 ]; then
  echo "$failures of $cases cases failed"
  exit 1
fi
echo "all $cases cases passed"
