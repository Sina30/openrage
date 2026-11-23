#!/usr/bin/env bash
# `cargo test` over the arguments, for a hosted runner: a test that fails is run again alone, in a
# process of its own, and only one that fails there too is red. Thousands of tests share a process
# and a few cores here, and a test that waits on a worker or reads a file's time can lose that race;
# it is the rule `just ready` holds a lane to.
set -uo pipefail

log=$(mktemp)
cargo test --no-fail-fast "$@" 2>&1 | tee "$log" && exit 0

# After a binary's tests cargo names it: "to rerun pass `-p or-io --lib`". A binary that failed and
# names no test died, or is a doctest: it is listed with no name.
awk '
  /^test .* \.\.\. FAILED$/ { failed[n++] = $2 }
  /to rerun pass `/ {
    sub(/.*to rerun pass `/, ""); sub(/`.*/, "")
    if (n == 0 || $0 ~ /--doc/) print $0 "\t"
    else for (i = 0; i < n; i++) print $0 "\t" failed[i]
    n = 0
  }' "$log" > "$log.again"

# Red with no binary named: the build failed.
[ -s "$log.again" ] || exit 1

red=0
while IFS=$'\t' read -r target name; do
  if [ -z "$name" ]; then
    echo "::error::$target failed and names no test to run again"
    red=1
    continue
  fi
  echo "::group::again, alone: $name"
  # shellcheck disable=SC2086 # the target is cargo's own words, several arguments
  cargo test $target -- --exact "$name" || red=1
  echo "::endgroup::"
done < "$log.again"
exit $red
