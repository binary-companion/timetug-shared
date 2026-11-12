#!/usr/bin/env bash
# Tests .github/workflows/bump-dependencies.yml: pins the shape of the workflow (triggers, environment, token
# scope, no merge, no label), runs its "Move the pin" step against the real scripts/ci/bump-cargo-dependency.py
# and the real Cargo.toml and Cargo.lock with a stub `cargo` (so the step must use the script's current command
# line), then runs its "Open the pull request" step against stub `git` and `gh` and asserts which pull requests it
# opens and closes. The steps are extracted from the workflow and executed, so the real text is what is tested.
# BUMP_WORKFLOW points at another copy of the workflow (used to prove that mutated copies fail).
# PYTHON selects the interpreter that has PyYAML (CI points it at build/venv/bin/python).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
WORKFLOW="${BUMP_WORKFLOW:-$ROOT/.github/workflows/bump-dependencies.yml}"
PYTHON="${PYTHON:-python3}"
command -v jq >/dev/null 2>&1 || { echo "FAIL: jq is required" >&2; exit 2; }
"$PYTHON" -c 'import yaml' 2>/dev/null || { echo "FAIL: PyYAML is required (pip install -r scripts/ci/requirements-test.txt)" >&2; exit 2; }
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin"
fail() { echo "FAIL: $*" >&2; exit 1; }

# --- the shape of the workflow, then the two steps extracted ---
"$PYTHON" - "$WORKFLOW" "$T/open.sh" "$T/move.sh" <<'PY'
import re, sys, yaml
workflow, open_target, move_target = sys.argv[1:]
raw = open(workflow).read()
document = yaml.safe_load(raw)
def check(condition, message):
    if not condition:
        sys.exit(f'FAIL: {workflow}: {message}')
triggers = document.get(True, document.get('on'))
check(isinstance(triggers, dict) and set(triggers) == {'repository_dispatch', 'workflow_dispatch'},
      'the triggers must be exactly repository_dispatch and workflow_dispatch (never pull_request_target or push)')
check(triggers['repository_dispatch'] == {'types': ['library-released']},
      'repository_dispatch must react to exactly the event type library-released')
inputs = triggers['workflow_dispatch'].get('inputs', {})
check(set(inputs) == {'version'} and inputs['version'].get('required') is True,
      'workflow_dispatch must take exactly one required input, version')
check(document.get('permissions') == {}, 'the workflow-level permissions must be {}')
check(document.get('concurrency') == {'group': 'bump-calendar-connectors', 'cancel-in-progress': False},
      'concurrency must be group bump-calendar-connectors without cancelling')
check(list(document['jobs']) == ['bump'], 'the workflow must have exactly the job bump')
job = document['jobs']['bump']
check(job.get('if') == "github.event_name == 'workflow_dispatch' || github.event.client_payload.library == 'calendar-connectors'",
      "the job must run only for a manual dispatch or for client_payload.library == 'calendar-connectors'")
check(job.get('environment') == 'bump', 'the job must use the bump environment (it holds the bot key)')
check(job.get('runs-on') == 'ubuntu-latest', 'the job must run on ubuntu-latest')
check(job.get('permissions') == {'contents': 'read'}, 'the job permissions must be exactly contents: read')
check(job.get('env') == {'VERSION': '${{ github.event.client_payload.version || inputs.version }}',
                         'BASE': '${{ github.ref_name }}'},
      'the job env must be exactly VERSION from the payload or the input, and BASE (the branch the workflow runs on)')
check('continue-on-error' not in job, 'the job must not set continue-on-error')
steps = job['steps']
check(len(steps) == 5, 'the job must have exactly five steps: token, checkout, toolchain, move, open')
token, checkout, toolchain, move, open_pr = steps
check(token.get('id') == 'app' and token.get('uses') == 'actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510',
      'the first step must be the token step (id app) pinned to the reviewed create-github-app-token SHA')
check(token.get('with') == {'app-id': '${{ vars.BUMP_APP_ID }}',
                            'private-key': '${{ secrets.BUMP_APP_PRIVATE_KEY }}',
                            'owner': '${{ github.repository_owner }}',
                            'repositories': 'timetug-shared',
                            'permission-contents': 'write',
                            'permission-pull-requests': 'write'},
      'the token step must be scoped to repositories: timetug-shared with exactly permission-contents: write and '
      'permission-pull-requests: write')
check(checkout.get('uses') == 'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
      'the checkout must be pinned to the reviewed actions/checkout SHA')
check(checkout.get('with') == {'ref': '${{ github.ref_name }}', 'token': '${{ steps.app.outputs.token }}'},
      'the checkout must be the branch the workflow runs on with the app token (so the push is made by the bot)')
check(toolchain.get('run') == 'rustup show active-toolchain || rustup toolchain install',
      'the third step must install the pinned Rust toolchain (cargo update needs it)')
check(move.get('name') == 'Move the pin' and open_pr.get('name') == 'Open the pull request',
      'the last two steps must be "Move the pin" and "Open the pull request", in that order')
check('env' not in move, 'the step "Move the pin" must not get the app token (it only edits files and runs cargo)')
check(open_pr.get('env') == {'GH_TOKEN': '${{ steps.app.outputs.token }}'},
      "the step 'Open the pull request' env must be exactly GH_TOKEN (the app token)")
for step in (move, open_pr):
    check('${{' not in step['run'], f"the step {step['name']!r} must not contain ${{{{ }}}} (values reach it through env)")
    check('continue-on-error' not in step, f"the step {step['name']!r} must not set continue-on-error")
    check(not re.search(r'gh\s+pr\s+merge|--auto\b|--admin\b|--label|--add-label|/merge\b|gh\s+workflow\s+run', step['run']),
          f"the step {step['name']!r} must never merge, auto-merge, label or start workflows (a new bump is a patch by default)")
for line in raw.splitlines():
    uses = re.match(r'\s*(?:-\s*)?uses:\s*(\S+)(.*)', line)
    if uses:
        check(re.fullmatch(r'[\w./-]+@[0-9a-f]{40}', uses.group(1)) and re.match(r'\s*# v\d+\s*$', uses.group(2)),
              f'every action must be pinned by commit SHA with the tag in a comment: {line.strip()}')
check('refs/pull' not in raw and 'pull_request' not in raw, 'the workflow must not mention pull_request triggers')
open(open_target, 'w').write(open_pr['run'])
open(move_target, 'w').write(move['run'])
PY

# --- stubs ---
cat > "$T/bin/git" <<'SCRIPT'
#!/usr/bin/env bash
echo "git $*" >> "$CALLS"
case "$1" in
  diff) [ "${DIFF_CLEAN:-0}" = 1 ] && exit 0 || exit 1;;
  ls-remote) [ "${BRANCH_EXISTS:-0}" = 1 ] && exit 0 || exit 2;;
esac
exit 0
SCRIPT
# `gh pr list --head X` matches the head branch name of every pull request, forks included, like the real one.
# `gh api` answers from canned JSON files and applies the real --jq expression with jq.
cat > "$T/bin/gh" <<'SCRIPT'
#!/usr/bin/env bash
echo "gh $*" >> "$CALLS"
case "$1 $2" in
  "pr list")
    head=""; expr="."; state=""
    args=("$@")
    for ((i = 0; i < ${#args[@]}; i++)); do
      [ "${args[i]}" = --head ] && head="${args[i+1]}"
      [ "${args[i]}" = --jq ] && expr="${args[i+1]}"
      [ "${args[i]}" = --state ] && state="${args[i+1]}"
    done
    # Closed and merged pull requests are not in the fixture; the real default would still hide them, but an
    # explicit --state open is what the workflow must ask for, so a lookup without it is an error here.
    [ "$state" = open ] || { echo "stub gh: pr list without --state open" >&2; exit 98; }
    jq -c --arg head "$head" 'map(select($head == "" or .headRefName == $head))' "$PRS" | jq -r "$expr";;
  "pr create")
    args=("$@")
    for ((i = 0; i < ${#args[@]}; i++)); do
      [ "${args[i]}" = --title ] && echo "${args[i+1]}" > "$CREATED.title"
      [ "${args[i]}" = --body-file ] && cp "${args[i+1]}" "$CREATED.body"
    done
    echo "https://github.com/binary-companion/timetug-shared/pull/99";;
  "pr close")
    case " ${FAIL_CLOSE:-} " in *" $3 "*) echo "could not close $3" >&2; exit 1;; esac;;
  "release view") echo "release notes of the connector";;
  "api repos/binary-companion/calendar-connectors/git/ref/tags/"*)
    file="$TAGS/ref-${2##*/}.json"
    [ -f "$file" ] || { echo "gh: Not Found (HTTP 404)" >&2; exit 1; }
    expr="."; [ "$3" = --jq ] && expr="$4"
    jq -r "$expr" "$file";;
  "api repos/binary-companion/calendar-connectors/git/tags/"*)
    file="$TAGS/tag-${2##*/}.json"
    [ -f "$file" ] || { echo "gh: Not Found (HTTP 404)" >&2; exit 1; }
    expr="."; [ "$3" = --jq ] && expr="$4"
    jq -r "$expr" "$file";;
  *) echo "stub gh: unexpected call: $*" >&2; exit 99;;
esac
exit 0
SCRIPT
chmod +x "$T/bin/git" "$T/bin/gh"
no_forbidden_calls() { # after every run: the workflow never merges, labels or starts workflows
  if grep -Eq 'pr merge|--auto|--admin|--label|--add-label|pr edit|workflow run' "$T/calls"; then
    fail "a forbidden gh call was made: $(grep -E 'pr merge|--auto|--admin|--label|--add-label|pr edit|workflow run' "$T/calls")"
  fi
}

# --- the "Move the pin" step, in a tree with the real script, manifest and lock file and a stub cargo ---
# The stub records its arguments and rewrites the lock the way `cargo update -p` would: every connector source
# moves to the tag the manifest names, at a made-up commit.
cat > "$T/bin/cargo" <<'SCRIPT'
#!/usr/bin/env bash
echo "cargo $*" >> "$CALLS"
[ "${CARGO_FAIL:-0}" != 1 ] || { echo "error: failed to fetch the tag" >&2; exit 101; }
manifest=Cargo.toml
for ((i = 1; i <= $#; i++)); do [ "${!i}" = --manifest-path ] && { j=$((i + 1)); manifest="${!j}"; }; done
tag="$(grep '^calendar-core = ' "$manifest" | grep -o 'tag = "[^"]*"' | cut -d'"' -f2)"
sed -E "s@[?](rev|tag)=[^#\"]+#[0-9a-f]+@?tag=$tag#1111111111111111111111111111111111111111@" Cargo.lock > Cargo.lock.new
mv Cargo.lock.new Cargo.lock
SCRIPT
chmod +x "$T/bin/cargo"
CURRENT="$("$ROOT/scripts/ci/cargo-dependency-version.py" "$ROOT/Cargo.lock" calendar-connectors 2>/dev/null || true)"
if [ -z "$CURRENT" ]; then CURRENT=1.0.0; fi   # a revision pin: the first bump is not compared
case "$CURRENT" in
  *-alpha.*) NEXT="${CURRENT%.*}.$(( ${CURRENT##*.} + 1 ))" ;;
  *) NEXT=2.0.0-alpha.1 ;;
esac
fresh_tree() {
  rm -rf "$T/move"
  mkdir -p "$T/move/scripts/ci"
  cp "$ROOT/scripts/ci/bump-cargo-dependency.py" "$ROOT/scripts/ci/cargo-dependency-version.py" "$T/move/scripts/ci/"
  cp "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$T/move/"
  cp "$T/move/Cargo.toml" "$T/move.toml.original"; cp "$T/move/Cargo.lock" "$T/move.lock.original"
}
# move <version>: sets STATUS; output in $T/move.out, calls in $T/calls.
move() {
  : > "$T/calls"
  STATUS=0
  (cd "$T/move" && CALLS="$T/calls" VERSION="$1" PATH="$T/bin:$PATH" bash "$T/move.sh") > "$T/move.out" 2>&1 || STATUS=$?
  no_forbidden_calls
}
locked_tags() { grep -o 'calendar-connectors?tag=[^#"]*' "$T/move/Cargo.lock" | sort -u | tr '\n' ' ' | sed 's/ $//'; }
untouched() { cmp -s "$T/move/Cargo.toml" "$T/move.toml.original" && cmp -s "$T/move/Cargo.lock" "$T/move.lock.original"; }

# A bump to the next prerelease: every connector line takes the tag, nothing else changes in the manifest, and
# cargo is asked to update exactly the connector crates.
fresh_tree
move "$NEXT"
[ "$STATUS" = 0 ] || fail "moving to $NEXT failed: $(cat "$T/move.out")"
[ "$(locked_tags)" = "calendar-connectors?tag=$NEXT" ] || fail "the lock does not follow: '$(locked_tags)'"
changed="$(diff "$T/move.toml.original" "$T/move/Cargo.toml" | grep -c '^>' || true)"
[ "$changed" = "$(grep -c 'binary-companion/calendar-connectors' "$T/move.toml.original")" ] ||
  fail "the manifest diff is not exactly the connector lines: $(diff "$T/move.toml.original" "$T/move/Cargo.toml")"
grep -q "tag = \"$NEXT\"" "$T/move/Cargo.toml" || fail 'the manifest does not carry the tag'
[ "$(grep -c '^cargo update --manifest-path ' "$T/calls")" = 1 ] || fail "cargo was not called once: $(cat "$T/calls")"
grep -q -- '-p calendar-core' "$T/calls" || fail 'cargo was not asked to update calendar-core'
[ "$(cat "$T/calls" | tr ' ' '\n' | grep -c '^-p$')" = 8 ] || fail 'cargo was not asked to update the eight connector crates'
no_cargo() { ! grep -q '^cargo' "$T/calls"; }

# A tag that cannot be fetched (cargo fails) leaves both files as they were.
fresh_tree; CARGO_FAIL=1 move "$NEXT"
[ "$STATUS" != 0 ] && untouched || fail 'a failing cargo did not fail the step cleanly'
unset CARGO_FAIL

# A hostile or malformed version is refused before cargo runs and before any file is touched.
fresh_tree
for hostile in '../../../repos/other/x' '1.0' 'v1.0.5' "$NEXT-rc1" '2.0.0-beta.1' '1.0.1234567890' "2.0.0
1.0.0" '' '1.0.5 --jq .' '$(touch /tmp/x)' '99999999999999999999.0.0' '2.0.0-alpha.01' '2.0.0+build'; do
  move "$hostile"
  [ "$STATUS" != 0 ] || fail "the version '$hostile' was accepted"
  untouched || fail "the version '$hostile' modified the files"
  no_cargo || fail "the version '$hostile' reached cargo"
done

# After the first bump to a tag: a downgrade, a change of major and a bump of the same version.
fresh_tree; move "$NEXT"; [ "$STATUS" = 0 ] || fail "the setup bump failed: $(cat "$T/move.out")"
cp "$T/move/Cargo.toml" "$T/move.toml.original"; cp "$T/move/Cargo.lock" "$T/move.lock.original"
move 2.0.0-alpha.0
[ "$STATUS" != 0 ] && untouched || fail 'a downgrade was accepted or modified the files'
grep -q 'refusing to move' "$T/move.out" || fail 'a downgrade did not say so'
move 3.0.0
[ "$STATUS" != 0 ] && untouched || fail 'a major change was applied'
grep -q 'manual change' "$T/move.out" || fail 'a major change did not say it needs a manual change'
move "$NEXT"
[ "$STATUS" = 0 ] && untouched || fail 'the version that is already pinned was not a no-op'
no_cargo || fail 'a no-op bump called cargo'
# A later prerelease, then the release itself.
move 2.0.0-alpha.9
[ "$STATUS" = 0 ] && [ "$(locked_tags)" = 'calendar-connectors?tag=2.0.0-alpha.9' ] || fail "a later prerelease failed: $(cat "$T/move.out")"
move 2.0.0
[ "$STATUS" = 0 ] && [ "$(locked_tags)" = 'calendar-connectors?tag=2.0.0' ] || fail "the release after its prereleases failed: $(cat "$T/move.out")"

# --- the "Open the pull request" step against stub git and gh ---
pr() { printf '{"number":%s,"headRefName":"%s","isCrossRepository":%s}' "$1" "$2" "${3:-false}"; }
PREFIX=bump/calendar-connectors
BASE_PRS="$(pr 10 $PREFIX-1.0.2),$(pr 11 $PREFIX-1.0.9),$(pr 12 $PREFIX-1.0.10),$(pr 13 $PREFIX-1.0.3),\
$(pr 14 feature/other),$(pr 15 $PREFIX-1.0.3-rc1),$(pr 16 xbump/calendar-connectors-1.0.1),\
$(pr 17 $PREFIX-1.0.1 true),$(pr 18 $PREFIX-1.0.0)"
# run <version> <extra pull requests as json objects, or empty>; sets STATUS, CLOSED (sorted numbers) and CALLS.
run() {
  local version="$1" extra="${2:-}"
  printf '[%s%s]\n' "$BASE_PRS" "${extra:+,$extra}" > "$T/prs.json"
  : > "$T/calls"; rm -f "$T/created.title" "$T/created.body"
  STATUS=0
  (cd "$ROOT" && CALLS="$T/calls" PRS="$T/prs.json" CREATED="$T/created" RUNNER_TEMP="$T" VERSION="$version" BASE=v2 \
     PATH="$T/bin:$PATH" bash "$T/open.sh") > "$T/out" 2>&1 || STATUS=$?
  CLOSED="$({ grep '^gh pr close ' "$T/calls" || true; } | awk '{print $4}' | sort -n | tr '\n' ' ' | sed 's/ $//')"
  no_forbidden_calls
}
called() { grep -q -- "$1" "$T/calls"; }

# 1.0.3 with the same-repository 1.0.3 pull request open: nothing is created; 1.0.2 and 1.0.0 are superseded;
# 1.0.9, 1.0.10, the equal 1.0.3, the rc, the unrelated, the prefixed-name and the cross-repository one are not.
run 1.0.3
[ "$STATUS" = 0 ] || fail "1.0.3 with an open pull request exited $STATUS: $(cat "$T/out")"
[ "$CLOSED" = "10 18" ] || fail "1.0.3 closed '$CLOSED', expected '10 18'"
called 'gh pr create' && fail 'an open same-repository pull request still got a new one'
called 'git push' && fail 'an open same-repository pull request still got a push'
called 'gh pr close 10 --comment Superseded by #13 --delete-branch' || fail 'close did not name the open pull request #13'

# 1.0.10 compares numerically: 1.0.9, 1.0.3, 1.0.2 and 1.0.0 are lower; the equal 1.0.10 is not closed.
run 1.0.10
[ "$STATUS" = 0 ] || fail "1.0.10 exited $STATUS: $(cat "$T/out")"
[ "$CLOSED" = "10 11 13 18" ] || fail "1.0.10 closed '$CLOSED', expected '10 11 13 18'"

# Prereleases sort below their release: 2.0.0-alpha.3 supersedes alpha.1 and alpha.2 but not the release 2.0.0 (which is
# higher), and 2.0.0 supersedes every alpha. A branch with another prerelease word is not a bump branch.
BASE_SAVE2="$BASE_PRS"
BASE_PRS="$(pr 20 $PREFIX-2.0.0-alpha.1),$(pr 21 $PREFIX-2.0.0-alpha.2),$(pr 22 $PREFIX-2.0.0),$(pr 23 $PREFIX-2.0.0-beta.1),\
$(pr 24 $PREFIX-2.0.0-alpha.10),$(pr 25 $PREFIX-2.0.0-alpha.3)"
run 2.0.0-alpha.3
[ "$STATUS" = 0 ] || fail "2.0.0-alpha.3 exited $STATUS: $(cat "$T/out")"
[ "$CLOSED" = "20 21" ] || fail "2.0.0-alpha.3 closed '$CLOSED', expected '20 21'"
run 2.0.0
[ "$STATUS" = 0 ] || fail "2.0.0 exited $STATUS: $(cat "$T/out")"
[ "$CLOSED" = "20 21 24 25" ] || fail "2.0.0 closed '$CLOSED', expected '20 21 24 25'"
BASE_PRS="$BASE_SAVE2"

# 1.0.3 without its own pull request: the branch and the pull request are created, then the older ones closed.
BASE_SAVE="$BASE_PRS"
BASE_PRS="$(pr 10 $PREFIX-1.0.2),$(pr 11 $PREFIX-1.0.9),$(pr 14 feature/other),$(pr 15 $PREFIX-1.0.3-rc1),\
$(pr 16 xbump/calendar-connectors-1.0.1),$(pr 17 $PREFIX-1.0.1 true),$(pr 18 $PREFIX-1.0.0)"
run 1.0.3
[ "$STATUS" = 0 ] || fail "1.0.3 without a pull request exited $STATUS: $(cat "$T/out")"
called 'git push origin bump/calendar-connectors-1.0.3' || fail 'branch was not pushed'
called 'gh pr create --base v2 --head bump/calendar-connectors-1.0.3' || fail 'pull request was not created'
[ "$CLOSED" = "10 18" ] || fail "1.0.3 (new) closed '$CLOSED', expected '10 18'"
called 'Superseded by #99' || fail 'close did not name the new pull request #99'
# What the pull request carries: branch, commit, bot identity, title, release notes; no label, no reviewer.
called 'git switch -c bump/calendar-connectors-1.0.3' || fail 'the branch was not created from the working tree'
called 'git commit -am Bump calendar-connectors to 1.0.3' || fail 'the commit message is not "Bump calendar-connectors to 1.0.3"'
called 'git config user.name timetug-bot\[bot\]' || fail 'the commit author name is not the bot'
called 'git config user.email 342080999+timetug-bot\[bot\]@users.noreply.github.com' || fail 'the commit author email is not the bot'
[ "$(cat "$T/created.title")" = 'Bump calendar-connectors to 1.0.3' ] || fail "the title is '$(cat "$T/created.title")'"
grep -q 'release notes of the connector' "$T/created.body" || fail 'the body lacks the release notes'
grep -q 'Opened by timetug-bot' "$T/created.body" || fail 'the body lacks the bot line'
called 'gh release view 1.0.3 --repo binary-companion/calendar-connectors' || fail 'the notes were not read from calendar-connectors'
grep -E '^gh pr create' "$T/calls" | grep -Eq -- '--(label|reviewer|assignee|draft)' && fail 'the pull request was labelled, assigned or drafted'

# A fork's pull request with the new branch name must not count as the pull request for the branch.
run 1.0.5 "$(pr 30 $PREFIX-1.0.5 true)"
[ "$STATUS" = 0 ] || fail "cross-repository head exited $STATUS: $(cat "$T/out")"
called 'git push origin bump/calendar-connectors-1.0.5' || fail 'a cross-repository pull request suppressed the push'
called 'gh pr create --base v2 --head bump/calendar-connectors-1.0.5' || fail 'a cross-repository pull request suppressed the create'
[ "$CLOSED" = "10 18" ] || fail "1.0.5 closed '$CLOSED', expected '10 18' (1.0.9 is higher)"

# A fork's pull request with a lower matching head is never closed, whatever its number.
run 1.0.10 "$(pr 31 $PREFIX-1.0.4 true)"
case " $CLOSED " in *" 31 "*|*" 17 "*) fail "a cross-repository pull request was closed: '$CLOSED'";; esac

# The branch exists on the remote but has no open pull request: open it from that branch, push nothing.
BRANCH_EXISTS=1 run 1.0.5
[ "$STATUS" = 0 ] || fail "existing branch exited $STATUS: $(cat "$T/out")"
called 'git push' && fail 'an existing branch was pushed again'
called 'gh pr create --base v2 --head bump/calendar-connectors-1.0.5' || fail 'existing branch got no pull request'
unset BRANCH_EXISTS

# One failing close does not stop the others, and the step still fails.
BASE_PRS="$BASE_SAVE"
FAIL_CLOSE=10 run 1.0.10
[ "$STATUS" != 0 ] || fail 'a failed close did not fail the step'
[ "$CLOSED" = "10 11 13 18" ] || fail "after a failed close the attempts were '$CLOSED', expected '10 11 13 18'"
grep -q '::warning::could not close #10' "$T/out" || fail 'a failed close left no warning'
unset FAIL_CLOSE

# No change in the working tree: the step does nothing.
DIFF_CLEAN=1 run 1.0.10
[ "$STATUS" = 0 ] && [ -z "$CLOSED" ] || fail 'a clean tree opened or closed something'
called 'gh pr' && fail 'a clean tree called gh pr'
called 'git push' && fail 'a clean tree pushed'
echo PASS
