#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
workspace_tool="$repository_root/tools/contributor/bin/lenso-workspace"
pr_tool="$repository_root/tools/contributor/bin/lenso-pr"

bash -n "$workspace_tool" "$pr_tool" "$repository_root/tools/contributor/install.sh"

fixture_root="$(mktemp -d -t lenso-contributor-tools.XXXXXX)"
cleanup_fixture() {
  rm -rf "$fixture_root"
}
trap cleanup_fixture EXIT

mkdir -p "$fixture_root/bin" "$fixture_root/framework"
real_rg="$(command -v rg || true)"
cat > "$fixture_root/bin/wt" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${LENSO_WT_FAIL:-false}" = "true" ]; then
  printf 'simulated wt failure\n' >&2
  exit 17
fi
if [ "${1:-}" != "-C" ]; then
  exit 2
fi
repository="$2"
branch="$(git -C "$repository" symbolic-ref --quiet --short HEAD)"
path="$(git -C "$repository" rev-parse --show-toplevel)"
dirty=false
[ -z "$(git -C "$repository" status --porcelain=v1 --untracked-files=normal)" ] || dirty=true
jq -cn --arg branch "$branch" --arg path "$path" --argjson dirty "$dirty" '{
  schema:2,
  items:[{
    branch:$branch,
    worktree:{path:$path,main:true,changes:{staged:false,modified:$dirty,untracked:false,renamed:false,deleted:false,conflicted:false}},
    operation_state:null,
    display:{main_state:"is_main"}
  }]
}'
EOF
cat > "$fixture_root/bin/rg" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${LENSO_RG_FAIL:-false}" = "true" ]; then
  printf 'simulated pin scan failure\n' >&2
  exit 2
fi
if [ -n "${LENSO_REAL_RG:-}" ]; then
  exec "$LENSO_REAL_RG" "$@"
fi
# The fixture contains no Cargo Git pins. Match ripgrep's no-match result on
# minimal CI images that do not preinstall ripgrep.
exit 1
EOF
cat > "$fixture_root/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${LENSO_GH_TRANSIENT_ONCE:-false}" = "true" ] \
  && [ ! -e "$LENSO_GH_TRANSIENT_STATE" ]; then
  printf 'failed once\n' > "$LENSO_GH_TRANSIENT_STATE"
  printf 'partial response that must not escape a failed attempt\n'
  printf 'Post "https://api.github.com/graphql": EOF\n' >&2
  exit 1
fi
if [ "${1:-}" = "pr" ] && [ "${2:-}" = "list" ]; then
  repository=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--repo" ]; then
      repository="${2:-}"
      shift
    fi
    shift
  done
  printf '%s\n' "$repository" >> "$LENSO_GH_LOG"
  printf '%s\n' '[{"number":7,"state":"OPEN","title":"Fixture PR","url":"https://github.com/LioRael/example/pull/7","isDraft":false,"updatedAt":"2026-08-29T00:00:00Z","statusCheckRollup":[{"conclusion":"SUCCESS","status":"COMPLETED"}]}]'
  exit 0
fi
if [ "${1:-}" = "repo" ] && [ "${2:-}" = "view" ]; then
  printf 'LioRael/example\n'
  exit 0
fi
if [ "${1:-}" = "pr" ] && [ "${2:-}" = "checks" ]; then
  printf '%s\n' '[{"bucket":"pending","name":"fixture","state":"IN_PROGRESS","link":"https://github.com/LioRael/example/actions/1"}]'
  exit 8
fi
if [ "${1:-}" = "pr" ] && [ "${2:-}" = "view" ]; then
  jq -cn \
    --arg branch "$LENSO_FIXTURE_BRANCH" \
    --arg head "${LENSO_FIXTURE_PR_HEAD:-0000000000000000000000000000000000000000}" '{
    state:"OPEN",
    isDraft:false,
    mergeable:"MERGEABLE",
    mergeStateStatus:"CLEAN",
    headRefName:$branch,
    baseRefName:"main",
    headRefOid:$head,
    mergeCommit:null,
    url:"https://github.com/LioRael/example/pull/8",
    title:"Fixture PR"
  }'
  exit 0
fi
printf 'unexpected gh invocation in contributor fixture\n' >&2
exit 99
EOF
chmod +x "$fixture_root/bin/wt" "$fixture_root/bin/rg" "$fixture_root/bin/gh"

git init --bare "$fixture_root/origin.git" >/dev/null
git clone "$fixture_root/origin.git" "$fixture_root/framework/example" >/dev/null 2>&1
git -C "$fixture_root/framework/example" config user.name "Lenso Tool Test"
git -C "$fixture_root/framework/example" config user.email "tools@example.invalid"
printf 'fixture\n' > "$fixture_root/framework/example/README.md"
git -C "$fixture_root/framework/example" add README.md
git -C "$fixture_root/framework/example" commit -m "test: seed fixture" >/dev/null
git -C "$fixture_root/framework/example" push -u origin HEAD:main >/dev/null 2>&1
git -C "$fixture_root/framework/example" remote set-head origin main

export PATH="$fixture_root/bin:$PATH"
export LENSO_FRAMEWORK_ROOT="$fixture_root/framework"
export LENSO_GH_LOG="$fixture_root/gh.log"
export LENSO_REAL_RG="$real_rg"
export LENSO_FIXTURE_BRANCH="$(git -C "$fixture_root/framework/example" symbolic-ref --short HEAD)"

snapshot="$($workspace_tool snapshot --json)"
jq -e '
  .schema == "lenso.workspace-snapshot.v2"
  and (.repositories | length) == 1
  and .repositories[0].ahead == 0
  and .repositories[0].behind == 0
  and .repositories[0].worktreeCount == 1
' >/dev/null <<< "$snapshot"

doctor="$($workspace_tool doctor --json)"
jq -e '.status == "passed"' >/dev/null <<< "$doctor"

set +e
LENSO_WT_FAIL=true $workspace_tool doctor --json > "$fixture_root/doctor-wt-failed.json"
doctor_wt_exit=$?
set -e
[ "$doctor_wt_exit" -eq 1 ]
jq -e '
  .status == "failed"
  and (.checks.worktreeStatusFailures | length) == 1
' >/dev/null "$fixture_root/doctor-wt-failed.json"

git -C "$fixture_root/framework/example" remote set-url origin git@github.com:LioRael/example.git
release_status="$($workspace_tool release-status --no-fetch --json)"
jq -e '
  .status == "complete"
  and (.pullRequests | length) == 1
  and .pullRequests[0].repository == "LioRael/example"
  and .pullRequests[0].checks == "success"
' >/dev/null <<< "$release_status"
[ "$(<"$LENSO_GH_LOG")" = "LioRael/example" ]

git init "$fixture_root/framework/other" >/dev/null
git -C "$fixture_root/framework/other" config user.name "Lenso Tool Test"
git -C "$fixture_root/framework/other" config user.email "tools@example.invalid"
printf 'other\n' > "$fixture_root/framework/other/README.md"
git -C "$fixture_root/framework/other" add README.md
git -C "$fixture_root/framework/other" commit -m "test: seed other fixture" >/dev/null
set +e
$pr_tool finish \
  --repo "$fixture_root/framework/example" \
  --pr 8 \
  --worktree "$fixture_root/framework/other" \
  > "$fixture_root/pr-wrong-repo.out" 2> "$fixture_root/pr-wrong-repo.err"
wrong_repo_exit=$?
LENSO_GH_TRANSIENT_ONCE=true \
LENSO_GH_TRANSIENT_STATE="$fixture_root/gh-transient-once" \
$pr_tool finish \
  --repo "$fixture_root/framework/example" \
  --pr 8 \
  --worktree "$fixture_root/framework/example" \
  > "$fixture_root/pr-wrong-head.out" 2> "$fixture_root/pr-wrong-head.err"
wrong_head_exit=$?
set -e
[ "$wrong_repo_exit" -eq 1 ]
[ "$wrong_head_exit" -eq 1 ]
grep -q 'does not belong to the requested repository' "$fixture_root/pr-wrong-repo.err"
grep -q 'WARN transient GitHub failure; retrying' "$fixture_root/pr-wrong-head.err"
grep -q 'does not match PR head' "$fixture_root/pr-wrong-head.err"

set +e
LENSO_FIXTURE_PR_HEAD="$(git -C "$fixture_root/framework/example" rev-parse HEAD)" \
$pr_tool finish \
  --repo "$fixture_root/framework/example" \
  --pr 8 \
  --worktree "$fixture_root/framework/example" \
  --required-check fixture \
  > "$fixture_root/pr-pending.out" 2> "$fixture_root/pr-pending.err"
pending_exit=$?
set -e
[ "$pending_exit" -eq 1 ]
grep -q 'PR checks are still pending' "$fixture_root/pr-pending.err"

set +e
LENSO_FIXTURE_PR_HEAD="$(git -C "$fixture_root/framework/example" rev-parse HEAD)" \
$pr_tool finish \
  --repo "$fixture_root/framework/example" \
  --pr 8 \
  --worktree "$fixture_root/framework/example" \
  --required-check quality \
  > "$fixture_root/pr-missing-required.out" \
  2> "$fixture_root/pr-missing-required.err"
missing_required_exit=$?
$pr_tool finish \
  --repo "$fixture_root/framework/example" \
  --pr 8 \
  --worktree "$fixture_root/framework/example" \
  --merge \
  > "$fixture_root/pr-implicit-required.out" \
  2> "$fixture_root/pr-implicit-required.err"
implicit_required_exit=$?
set -e
[ "$missing_required_exit" -eq 1 ]
[ "$implicit_required_exit" -eq 2 ]
grep -q 'required PR checks are not observable: quality' \
  "$fixture_root/pr-missing-required.err"
grep -q -- '--merge requires at least one explicit --required-check' \
  "$fixture_root/pr-implicit-required.err"

set +e
LENSO_RG_FAIL=true $workspace_tool release-status --no-fetch --json \
  > "$fixture_root/release-pin-failed.json"
release_pin_exit=$?
LENSO_WT_FAIL=true $workspace_tool release-status --no-fetch --json \
  > "$fixture_root/release-wt-failed.json"
release_wt_exit=$?
set -e
[ "$release_pin_exit" -eq 1 ]
[ "$release_wt_exit" -eq 1 ]
jq -e '.status == "failed" and .pinScanError != null' \
  >/dev/null "$fixture_root/release-pin-failed.json"
jq -e '.status == "failed" and (.worktreeStatusFailures | length) > 0' \
  >/dev/null "$fixture_root/release-wt-failed.json"

printf 'dirty\n' > "$fixture_root/framework/example/untracked.txt"
set +e
$workspace_tool doctor --json > "$fixture_root/doctor-dirty.json"
doctor_exit=$?
$pr_tool finish \
  --repo "$fixture_root/framework/example" \
  --pr 1 \
  --worktree "$fixture_root/framework/example" \
  > "$fixture_root/pr.out" 2> "$fixture_root/pr.err"
pr_exit=$?
set -e
[ "$doctor_exit" -eq 1 ]
jq -e '.status == "attention" and (.checks.dirtyWorktrees | length) == 1' \
  >/dev/null "$fixture_root/doctor-dirty.json"
[ "$pr_exit" -eq 1 ]
grep -q 'target worktree is dirty; preserving it' "$fixture_root/pr.err"

mkdir -p "$fixture_root/install-target"
"$repository_root/tools/contributor/install.sh" \
  --framework-root "$fixture_root/install-target" >/dev/null
cmp "$workspace_tool" "$fixture_root/install-target/.lenso-tools/bin/lenso-workspace"
cmp "$pr_tool" "$fixture_root/install-target/.lenso-tools/bin/lenso-pr"

printf 'Contributor tool checks passed.\n'
