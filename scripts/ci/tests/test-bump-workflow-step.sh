#!/usr/bin/env bash
# Tests .github/workflows/bump-dependencies.yml: pins the shape of the workflow (triggers, environment, token
# scope, no merge, no label), runs its "Move the pin" step against the real scripts/ci/bump-resolved.py and
# the real Package.resolved and Package.swift (so the step must use the script's current command line), then
# runs its "Open the pull request" step against stub `git` and `gh` and asserts which pull requests it opens
# and closes. The steps are extracted from the workflow and executed, so the real text is what is tested.
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
check(job.get('env') == {'VERSION': '${{ github.event.client_payload.version || inputs.version }}'},
      'the job env must be exactly VERSION from the payload or the input')
check('continue-on-error' not in job, 'the job must not set continue-on-error')
steps = job['steps']
check(len(steps) == 4, 'the job must have exactly four steps: token, checkout, move, open')
token, checkout, move, open_pr = steps
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
check(checkout.get('with') == {'ref': 'main', 'token': '${{ steps.app.outputs.token }}'},
      'the checkout must be ref: main with the app token (so the push is made by the bot)')
check(move.get('name') == 'Move the pin' and open_pr.get('name') == 'Open the pull request',
      'the last two steps must be "Move the pin" and "Open the pull request", in that order')
for step in (move, open_pr):
    check(step.get('env') == {'GH_TOKEN': '${{ steps.app.outputs.token }}'},
          f"the step {step['name']!r} env must be exactly GH_TOKEN (the app token)")
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

# --- the "Move the pin" step, in a tree with the real script, manifest and resolved file ---
CURRENT="$("$ROOT/scripts/ci/resolved-version.py" "$ROOT/Package.resolved" calendar-connectors)"
IFS=. read -r MAJOR MINOR PATCH <<< "$CURRENT"
NEXT="$MAJOR.$MINOR.$((PATCH + 1))"
LOWER=""
if [ "$PATCH" -gt 0 ]; then LOWER="$MAJOR.$MINOR.$((PATCH - 1))"; elif [ "$MINOR" -gt 0 ]; then LOWER="$MAJOR.$((MINOR - 1)).0"; fi
NEW_SHA=1111111111111111111111111111111111111111
TAG_OBJECT=2222222222222222222222222222222222222222
OTHER_TAG_OBJECT=3333333333333333333333333333333333333333
fresh_tree() {
  rm -rf "$T/move" "$T/tags"
  mkdir -p "$T/move/scripts/ci" "$T/tags"
  cp "$ROOT/scripts/ci/bump-resolved.py" "$T/move/scripts/ci/"
  cp "$ROOT/Package.swift" "$ROOT/Package.resolved" "$T/move/"
  cp "$T/move/Package.resolved" "$T/move.original"
}
lightweight() { printf '{"ref":"refs/tags/%s","object":{"type":"commit","sha":"%s"}}' "$1" "$2" > "$T/tags/ref-$1.json"; }
annotated() { # annotated <version> <tag object sha> <target type> <target sha>
  printf '{"ref":"refs/tags/%s","object":{"type":"tag","sha":"%s"}}' "$1" "$2" > "$T/tags/ref-$1.json"
  printf '{"sha":"%s","object":{"type":"%s","sha":"%s"}}' "$2" "$3" "$4" > "$T/tags/tag-$2.json"
}
# move <version>: sets STATUS; output in $T/move.out, calls in $T/calls.
move() {
  : > "$T/calls"
  STATUS=0
  (cd "$T/move" && CALLS="$T/calls" TAGS="$T/tags" VERSION="$1" GH_TOKEN=unused PATH="$T/bin:$PATH" \
     bash "$T/move.sh") > "$T/move.out" 2>&1 || STATUS=$?
  no_forbidden_calls
}
pin_field() { jq -r --arg f "$2" '.pins[] | select(.identity == "calendar-connectors") | .state[$f]' "$1"; }
untouched() { cmp -s "$T/move/Package.resolved" "$T/move.original"; }

# A lightweight tag: the pin moves to the tag's commit and the new version, and nothing else changes.
fresh_tree; lightweight "$NEXT" "$NEW_SHA"
move "$NEXT"
[ "$STATUS" = 0 ] || fail "moving to $NEXT failed: $(cat "$T/move.out")"
[ "$(pin_field "$T/move/Package.resolved" version)" = "$NEXT" ] || fail "the pin did not move to $NEXT"
[ "$(pin_field "$T/move/Package.resolved" revision)" = "$NEW_SHA" ] || fail 'the pin revision is not the tag commit'
[ "$(jq -r .originHash "$T/move/Package.resolved")" = "$(jq -r .originHash "$T/move.original")" ] || fail 'originHash changed'
[ "$(diff "$T/move.original" "$T/move/Package.resolved" | grep -c '^[<>]')" = 4 ] ||
  fail "the diff is not exactly the revision and version lines: $(diff "$T/move.original" "$T/move/Package.resolved")"
grep -q "api repos/binary-companion/calendar-connectors/git/ref/tags/$NEXT " "$T/calls" || fail 'the tag ref was not read'
grep -q 'git/tags/' "$T/calls" && fail 'a lightweight tag was dereferenced as a tag object'

# An annotated tag: the commit is the dereferenced one, never the tag object.
fresh_tree; annotated "$NEXT" "$TAG_OBJECT" commit "$NEW_SHA"
move "$NEXT"
[ "$STATUS" = 0 ] || fail "an annotated tag failed: $(cat "$T/move.out")"
[ "$(pin_field "$T/move/Package.resolved" revision)" = "$NEW_SHA" ] || fail 'an annotated tag was not dereferenced to its commit'
grep -q "git/tags/$TAG_OBJECT" "$T/calls" || fail 'the tag object was not read'

# A tag that points at another tag (or at a tree) is refused; nothing is written.
fresh_tree; annotated "$NEXT" "$TAG_OBJECT" tag "$OTHER_TAG_OBJECT"
move "$NEXT"
[ "$STATUS" != 0 ] && untouched || fail 'a tag of a tag was accepted or modified the file'
fresh_tree; annotated "$NEXT" "$TAG_OBJECT" tree "$NEW_SHA"
move "$NEXT"
[ "$STATUS" != 0 ] && untouched || fail 'a tag that points at a tree was accepted or modified the file'

# A tag that does not exist (404) fails the step and leaves the file.
fresh_tree
move "$NEXT"
[ "$STATUS" != 0 ] && untouched || fail 'a missing tag did not fail the step cleanly'

# The API answering with something that is not a 40-character hex sha is refused by the script.
fresh_tree; lightweight "$NEXT" 'not-a-sha'
move "$NEXT"
[ "$STATUS" != 0 ] && untouched || fail 'a malformed sha was accepted'

# A hostile or malformed version is refused before any API call is made.
fresh_tree; lightweight "$NEXT" "$NEW_SHA"
for hostile in '../../../repos/other/x' '1.0' 'v1.0.5' "$NEXT-rc1" "$MAJOR.$MINOR.1234567890" "$NEXT
1.0.0" '' '1.0.5 --jq .' '$(touch /tmp/x)' '99999999999999999999.0.0'; do
  move "$hostile"
  [ "$STATUS" != 0 ] || fail "the version '$hostile' was accepted"
  untouched || fail "the version '$hostile' modified the file"
  grep -q 'gh api' "$T/calls" && fail "the version '$hostile' reached the API"
done

# A downgrade and a change of major are refused and leave the file (the tag exists for both).
if [ -n "$LOWER" ]; then
  fresh_tree; lightweight "$LOWER" "$NEW_SHA"
  move "$LOWER"
  [ "$STATUS" != 0 ] && untouched || fail "a downgrade to $LOWER was accepted"
fi
fresh_tree; lightweight "$((MAJOR + 1)).0.0" "$NEW_SHA"
move "$((MAJOR + 1)).0.0"
[ "$STATUS" != 0 ] && untouched || fail 'a major change was applied'
grep -q 'manual change' "$T/move.out" || fail 'a major change did not say it needs a manual change'

# The version that is already resolved, at the same commit, is a clean no-op (the open step then sees no diff).
fresh_tree; lightweight "$CURRENT" "$(pin_field "$T/move/Package.resolved" revision)"
move "$CURRENT"
[ "$STATUS" = 0 ] && untouched || fail 'the version that is already resolved was not a no-op'
# ... but at another commit it is refused (a moved tag), never silently rewritten.
fresh_tree; lightweight "$CURRENT" "$NEW_SHA"
move "$CURRENT"
[ "$STATUS" != 0 ] && untouched || fail 'a moved tag rewrote the revision of the current version'

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
  CALLS="$T/calls" PRS="$T/prs.json" CREATED="$T/created" RUNNER_TEMP="$T" VERSION="$version" PATH="$T/bin:$PATH" \
    bash "$T/open.sh" > "$T/out" 2>&1 || STATUS=$?
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

# 1.0.3 without its own pull request: the branch and the pull request are created, then the older ones closed.
BASE_SAVE="$BASE_PRS"
BASE_PRS="$(pr 10 $PREFIX-1.0.2),$(pr 11 $PREFIX-1.0.9),$(pr 14 feature/other),$(pr 15 $PREFIX-1.0.3-rc1),\
$(pr 16 xbump/calendar-connectors-1.0.1),$(pr 17 $PREFIX-1.0.1 true),$(pr 18 $PREFIX-1.0.0)"
run 1.0.3
[ "$STATUS" = 0 ] || fail "1.0.3 without a pull request exited $STATUS: $(cat "$T/out")"
called 'git push origin bump/calendar-connectors-1.0.3' || fail 'branch was not pushed'
called 'gh pr create --base main --head bump/calendar-connectors-1.0.3' || fail 'pull request was not created'
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
called 'gh pr create --base main --head bump/calendar-connectors-1.0.5' || fail 'a cross-repository pull request suppressed the create'
[ "$CLOSED" = "10 18" ] || fail "1.0.5 closed '$CLOSED', expected '10 18' (1.0.9 is higher)"

# A fork's pull request with a lower matching head is never closed, whatever its number.
run 1.0.10 "$(pr 31 $PREFIX-1.0.4 true)"
case " $CLOSED " in *" 31 "*|*" 17 "*) fail "a cross-repository pull request was closed: '$CLOSED'";; esac

# The branch exists on the remote but has no open pull request: open it from that branch, push nothing.
BRANCH_EXISTS=1 run 1.0.5
[ "$STATUS" = 0 ] || fail "existing branch exited $STATUS: $(cat "$T/out")"
called 'git push' && fail 'an existing branch was pushed again'
called 'gh pr create --base main --head bump/calendar-connectors-1.0.5' || fail 'existing branch got no pull request'
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
