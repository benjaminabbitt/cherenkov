#!/usr/bin/env bash
# Tests the `push-check` recipe (lefthook's pre-push step) with the check
# replaced by a stub through the recipe's `push_check_cmd` variable, so the
# real `just check` never runs. Run: scripts/test-push-check.sh
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
sha=1111111111111111111111111111111111111111
zero=0000000000000000000000000000000000000000
fail=0

# run_case NAME EXPECTED_EXIT EXPECT_CHECK_RAN CHECK_CMD STDIN_LINE...
run_case() {
    local name=$1 want=$2 want_ran=$3 cmd=$4
    shift 4
    local marker out status ran=0
    marker="$(mktemp)"
    rm -f "$marker"
    out="$(printf '%s\n' "$@" |
        just --justfile "$root/justfile" --set push_check_cmd "touch $marker; $cmd" push-check 2>&1)"
    status=$?
    [ -e "$marker" ] && ran=1
    rm -f "$marker"
    if [ "$status" = "$want" ] && [ "$ran" = "$want_ran" ]; then
        echo "ok   $name"
    else
        echo "FAIL $name: exit $status (want $want), check ran $ran (want $want_ran)"
        echo "$out" | sed 's/^/     /'
        fail=1
    fi
}

main="refs/heads/B $sha refs/heads/main $sha"
branch="refs/heads/B $sha refs/heads/B $sha"
delete="(delete) $zero refs/heads/old $sha"

run_case test_push_to_main_red_check_fails_the_push 1 1 false "$main"
run_case test_push_to_branch_red_check_warns_and_passes 0 1 false "$branch"
run_case test_push_to_main_green_check_passes 0 1 true "$main"
run_case test_push_of_only_deletions_runs_no_check 0 0 false "$delete" "$delete"
run_case test_push_of_deletion_and_main_red_still_fails 1 1 false "$delete" "$main"
run_case test_push_to_master_red_is_not_the_guarded_branch 0 1 false \
    "refs/heads/B $sha refs/heads/master $sha"

exit "$fail"
