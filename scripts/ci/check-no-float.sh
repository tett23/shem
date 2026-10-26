#!/bin/sh
# The rendering uses integer and fixed-point arithmetic only, so src/ has no
# floating-point types (docs/adr/0001).
set -u
cd "$(dirname "$0")/../.." || exit 1
if grep -rnE '(^|[^A-Za-z0-9_])f(32|64)([^A-Za-z0-9_]|$)' src/; then
  echo "check-no-float: floating-point types in src/ (docs/adr/0001)" >&2
  exit 1
fi
