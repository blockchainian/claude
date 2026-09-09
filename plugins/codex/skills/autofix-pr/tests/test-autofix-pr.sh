#!/usr/bin/env bash
# ABOUTME: End-to-end test for autofix-pr.sh using stubbed gh and daemon-runner CLIs.
# ABOUTME: Covers a clean round, exhausted rounds, UX routing by label and config-thread detection.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
DRIVER="$HERE/../autofix-pr.sh"

SCRATCH="$(mktemp -d /tmp/codex-autofix-pr-test.XXXXXX)"
trap 'rm -rf "$SCRATCH"' EXIT

FAILS=0
assert() { # assert <desc> <cmd...>
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then
    echo "PASS: $desc"
  else
    echo "FAIL: $desc  [cmd: $*]"
    FAILS=$((FAILS+1))
  fi
}
assert_eq() { # assert_eq <desc> <expected> <actual>
  if [ "$2" = "$3" ]; then
    echo "PASS: $1"
  else
    echo "FAIL: $1  [expected '$2' got '$3']"
    FAILS=$((FAILS+1))
  fi
}

# ---------- skill metadata ----------
# Claude Code prefixes the plugin name itself, so the frontmatter name carries no namespace.
SKILL_NAME="$(awk '/^name:/{sub(/^name: */, ""); gsub(/"/, ""); print; exit}' "$HERE/../SKILL.md")"
assert_eq "SKILL.md name resolves to /codex:autofix-pr" "autofix-pr" "$SKILL_NAME"

# ---------- fixture helpers ----------
STUB_BIN="$SCRATCH/bin"
mkdir -p "$STUB_BIN"
cp "$HERE/stub-gh/gh" "$STUB_BIN/gh"
export PATH="$STUB_BIN:$PATH"
export FIXPR_DAEMON_RUNNER="$HERE/stub-daemon-runner/daemon-run"

REPO="$SCRATCH/repo"
git init -q -b main "$REPO"
git -C "$REPO" config user.email test@test && git -C "$REPO" config user.name test
echo base > "$REPO/base.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base

new_stub_dir() { # new_stub_dir <name>
  STUB_DIR="$SCRATCH/stub-$1"
  mkdir -p "$STUB_DIR"
  export STUB_DIR
}

write_pr() { # write_pr <sha> <label-or-empty>
  local labels="[]"
  [ -n "${2:-}" ] && labels="[{\"name\": \"$2\"}]"
  cat > "$STUB_DIR/pr.json" <<EOF
{"number": 7, "head": {"sha": "$1", "ref": "feature"},
 "base": {"repo": {"owner": {"login": "acme"}, "name": "app"}},
 "labels": $labels}
EOF
}

write_commit_time() { printf '{"commit": {"committer": {"date": "%s"}}}\n' "$1" > "$STUB_DIR/commit.json"; }
write_comment_time() { printf '[{"id": 1, "created_at": "%s", "body": "review"}]\n' "$1" > "$STUB_DIR/comments.json"; }

thread() { # thread <id> <resolved true|false> <path> <body>
  printf '{"id": "%s", "isResolved": %s, "isOutdated": false, "path": "%s", "comments": {"nodes": [{"body": "%s"}]}}' \
    "$1" "$2" "$3" "$4"
}
write_threads() { # write_threads <file> <thread-json>...
  local file="$1"; shift
  local joined=""
  while [ "$#" -gt 0 ]; do
    [ -n "$joined" ] && joined="$joined,"
    joined="$joined$1"
    shift
  done
  printf '{"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [%s]}}}}}\n' "$joined" > "$file"
}

run_driver() { # run_driver <args...>; captures stdout in $OUT and exit code in $RC
  set +e
  OUT="$("$DRIVER" --pr 7 --repo "$REPO" --wait 2 --poll 1 "$@" 2>"$SCRATCH/err.log")"
  RC=$?
  set -e 2>/dev/null || true
  printf '%s' "$OUT" > "$SCRATCH/out.json"
}

field() { printf '%s' "$OUT" | jq -c "$1"; }

# ---------- scenario 1: one clean round ----------
new_stub_dir clean
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T1 false proxy/src/api.ts 'null deref on the error path')"
write_threads "$STUB_DIR/threads.after.json" "$(thread T1 true proxy/src/api.ts 'null deref on the error path')"
cat > "$STUB_DIR/last-message.txt" <<'EOF'
Fixed the null deref and pushed.
Deployed staging: {"target": "staging", "sha": "bbbbbbb2", "ok": true, "steps": []}
wrangler said: Current Version ID: 30e88bb3-9cc7-463b-863e-c6ef33b51657 for staging.
EOF
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
sed 's/aaaaaaa1/bbbbbbb2/' "$STUB_DIR/pr.json" > "$STUB_DIR/pr.json.tmp" && mv "$STUB_DIR/pr.json.tmp" "$STUB_DIR/pr.json"
EOF

run_driver --max-rounds 2
echo "---- scenario 1 exit=$RC ----"
echo "$OUT"
assert_eq "clean round exits 0" 0 "$RC"
assert "output is one flat JSON object" jq -e 'type == "object" and (map(type == "array" or type == "number" or type == "boolean") | all)' "$SCRATCH/out.json"
assert_eq "one round ran" 1 "$(field .rounds)"
assert_eq "the pushed head sha is reported" '["bbbbbbb2"]' "$(field .pushed)"
assert_eq "the staging deploy sha is reported" '["bbbbbbb2"]' "$(field .staging_deploys)"
assert_eq "no ux threads" '[]' "$(field .ux_threads)"
assert_eq "no config threads" '[]' "$(field .config_threads)"
assert_eq "nothing remains" '[]' "$(field .remaining)"
assert_eq "codex ran once" 1 "$(wc -l < "$STUB_DIR/invocations.log" | tr -d ' ')"
assert "the daemon thread carries the round name" \
  grep -q '^name=7/fix-pr r1 ' "$STUB_DIR/invocations.log"
assert "the prompt names the Codex fix-pr skill and the PR" \
  grep -q 'fix-pr skill' "$STUB_DIR/prompts.log"
assert "the prompt asks for staging-only by default" grep -q 'staging-only' "$STUB_DIR/prompts.log"
assert_eq "the run is not awaiting a review" false "$(field .awaiting_review)"
assert "gh read the review threads over graphql" grep -q 'graphql' "$STUB_DIR/gh.log"

# ---------- scenario 2: rounds exhausted ----------
new_stub_dir exhausted
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T9 false proxy/src/api.ts 'still broken')"

run_driver --max-rounds 2
echo "---- scenario 2 exit=$RC ----"
echo "$OUT"
assert_eq "unresolved threads after the last round exit 2" 2 "$RC"
assert_eq "both rounds ran" 2 "$(field .rounds)"
assert_eq "the unresolved thread is reported as remaining" '["T9"]' "$(field .remaining)"
assert_eq "nothing was pushed" '[]' "$(field .pushed)"
assert_eq "codex ran twice" 2 "$(wc -l < "$STUB_DIR/invocations.log" | tr -d ' ')"
assert "the second thread name carries round 2" \
  grep -q '^name=7/fix-pr r2 ' "$STUB_DIR/invocations.log"

# ---------- scenario 3: UX thread routed by label ----------
new_stub_dir ux
write_pr aaaaaaa1 claude-code-ux
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
UX_THREAD="$(thread T_UX false website/app/page.tsx '[UX — Claude Code] **The warning row needs a new column.**')"
write_threads "$STUB_DIR/threads.json" "$(thread T2 false proxy/src/api.ts 'unchecked index')" "$UX_THREAD"
write_threads "$STUB_DIR/threads.after.json" "$(thread T2 true proxy/src/api.ts 'unchecked index')" "$UX_THREAD"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
EOF

run_driver --max-rounds 2
echo "---- scenario 3 exit=$RC ----"
echo "$OUT"
assert_eq "a UX thread alone does not block completion" 0 "$RC"
assert_eq "one round ran" 1 "$(field .rounds)"
assert_eq "the UX thread is routed to Claude Code" '["T_UX"]' "$(field .ux_threads)"
assert_eq "the UX thread is not counted as remaining" '[]' "$(field .remaining)"
assert "staging-only is passed to the Codex skill" grep -q 'staging-only' "$STUB_DIR/prompts.log"

# ---------- scenario 3c: --production hands the production flip to the Codex skill ----------
new_stub_dir ship
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T_S false proxy/src/api.ts 'off by one')"
write_threads "$STUB_DIR/threads.after.json" "$(thread T_S true proxy/src/api.ts 'off by one')"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
EOF

run_driver --max-rounds 1 --production
echo "---- scenario 3c exit=$RC ----"
assert "with --production the prompt asks for production" grep -q 'deploy production' "$STUB_DIR/prompts.log"
assert "with --production the prompt does not say staging-only" \
  sh -c "! grep -q 'staging-only' '$STUB_DIR/prompts.log'"

# ---------- scenario 3a: UX marks are streamed to --ux-file while the round runs ----------
new_stub_dir ux-stream
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T2 false proxy/src/api.ts 'unchecked index')" "$(thread T_UX2 false website/app/page.tsx 'needs a column')"
write_threads "$STUB_DIR/threads.after.json" "$(thread T2 true proxy/src/api.ts 'unchecked index')" "$(thread T_UX2 false website/app/page.tsx '[UX — Claude Code] **needs a column**')"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
sed 's/"labels": \[\]/"labels": [{"name": "claude-code-ux"}]/' "$STUB_DIR/pr.json" > "$STUB_DIR/pr.json.tmp" && mv "$STUB_DIR/pr.json.tmp" "$STUB_DIR/pr.json"
EOF
UX_FILE="$SCRATCH/ux-stream.txt"
run_driver --max-rounds 1 --ux-file "$UX_FILE"
echo "---- scenario 3a exit=$RC ----"
echo "$OUT"
assert_eq "the marked thread is streamed to the ux file" "T_UX2" "$(cat "$UX_FILE" 2>/dev/null)"
assert_eq "the marked thread is also in the final JSON" '["T_UX2"]' "$(field .ux_threads)"
assert "the prompt tells Codex to leave marked threads to Claude Code" grep -q 'belong to Claude Code' "$STUB_DIR/prompts.log"
assert "the prompt tells Codex to rebase before pushing" grep -q 'rebase' "$STUB_DIR/prompts.log"
assert "the prompt names the UI paths Claude Code owns" grep -q 'website/src/components' "$STUB_DIR/prompts.log"

# ---------- scenario 3b: the same thread without the PR label stays remaining ----------
new_stub_dir ux-nolabel
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$UX_THREAD"

run_driver --max-rounds 1
echo "---- scenario 3b exit=$RC ----"
echo "$OUT"
assert_eq "without the label the UX marker is not routed" '[]' "$(field .ux_threads)"
assert_eq "without the label the thread remains" '["T_UX"]' "$(field .remaining)"

# ---------- scenario 4: config thread left for the user ----------
new_stub_dir config
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
CFG_THREAD="$(thread T_CFG false .claude/settings.json 'this hook entry denies the deploy command')"
write_threads "$STUB_DIR/threads.json" "$(thread T3 false proxy/src/api.ts 'swallowed error')" "$CFG_THREAD"
write_threads "$STUB_DIR/threads.after.json" "$(thread T3 true proxy/src/api.ts 'swallowed error')" "$CFG_THREAD"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
EOF

run_driver --max-rounds 2
echo "---- scenario 4 exit=$RC ----"
echo "$OUT"
assert_eq "a config thread alone does not block completion" 0 "$RC"
assert_eq "the config thread is left for the user" '["T_CFG"]' "$(field .config_threads)"
assert_eq "the config thread is not counted as remaining" '[]' "$(field .remaining)"

# ---------- scenario 5: no review comment newer than the PR head ----------
new_stub_dir nocomment
write_pr aaaaaaa1
write_commit_time "2026-09-08T12:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T4 false proxy/src/api.ts 'stale finding')"

run_driver --max-rounds 2
echo "---- scenario 5 exit=$RC ----"
echo "$OUT"
assert_eq "no round runs without a review newer than the head" 0 "$(field .rounds)"
assert_eq "the driver reports the pre-existing thread" '["T4"]' "$(field .remaining)"
assert "codex was never invoked" test ! -f "$STUB_DIR/invocations.log"

# ---------- scenario 6: the reviewer reacts to the push with a review, no inline comment ----------
new_stub_dir rereview
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T5 false proxy/src/api.ts 'first finding')"
write_threads "$STUB_DIR/threads.r1.json" "$(thread T5 true proxy/src/api.ts 'first finding')" "$(thread T6 false proxy/src/db.ts 'finding on the fix')"
write_threads "$STUB_DIR/threads.r2.json" "$(thread T5 true proxy/src/api.ts 'first finding')" "$(thread T6 true proxy/src/db.ts 'finding on the fix')"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
if [ "$STUB_ROUND" = 1 ]; then
  cp "$STUB_DIR/threads.r1.json" "$STUB_DIR/threads.json"
  sed 's/aaaaaaa1/bbbbbbb2/' "$STUB_DIR/pr.json" > "$STUB_DIR/pr.json.tmp" && mv "$STUB_DIR/pr.json.tmp" "$STUB_DIR/pr.json"
  printf '{"commit": {"committer": {"date": "2026-09-08T12:00:00Z"}}}\n' > "$STUB_DIR/commit.json"
  printf '[{"id": 9, "submitted_at": "2026-09-08T12:30:00Z", "state": "COMMENTED", "user": {"login": "reviewer-bot"}, "commit_id": "bbbbbbb2", "body": "review"}]\n' > "$STUB_DIR/reviews.json"
else
  cp "$STUB_DIR/threads.r2.json" "$STUB_DIR/threads.json"
fi
EOF

run_driver --max-rounds 2
echo "---- scenario 6 exit=$RC ----"
echo "$OUT"
assert_eq "the thread the reviewer opened on the push is fixed in round 2" 2 "$(field .rounds)"
assert_eq "nothing remains after the re-review" '[]' "$(field .remaining)"
assert_eq "the push is reported once" '["bbbbbbb2"]' "$(field .pushed)"
assert "the wait reads reviews, comments and reactions in one graphql query" \
  sh -c "grep -q 'reactions(' '$STUB_DIR/gh.log' && ! grep -qE 'pulls/7/(reviews|comments)|issues/7/reactions' '$STUB_DIR/gh.log'"

# ---------- scenario 7: no review reaches the pushed head within the wait ----------
new_stub_dir unreviewed
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T7 false proxy/src/api.ts 'finding')"
write_threads "$STUB_DIR/threads.after.json" "$(thread T7 true proxy/src/api.ts 'finding')"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
sed 's/aaaaaaa1/bbbbbbb2/' "$STUB_DIR/pr.json" > "$STUB_DIR/pr.json.tmp" && mv "$STUB_DIR/pr.json.tmp" "$STUB_DIR/pr.json"
printf '{"commit": {"committer": {"date": "2026-09-08T12:00:00Z"}}}\n' > "$STUB_DIR/commit.json"
EOF

run_driver --max-rounds 2
echo "---- scenario 7 exit=$RC ----"
echo "$OUT"
assert_eq "one round ran" 1 "$(field .rounds)"
assert_eq "an unreviewed push is reported as awaiting review" true "$(field .awaiting_review)"
assert_eq "nothing is known to remain" '[]' "$(field .remaining)"

# ---------- scenario 7b: the engine's own thread replies are not the reviewer reacting ----------
new_stub_dir own-replies
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T8 false proxy/src/api.ts 'finding')"
write_threads "$STUB_DIR/threads.after.json" "$(thread T8 true proxy/src/api.ts 'finding')"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
sed 's/aaaaaaa1/bbbbbbb2/' "$STUB_DIR/pr.json" > "$STUB_DIR/pr.json.tmp" && mv "$STUB_DIR/pr.json.tmp" "$STUB_DIR/pr.json"
printf '{"commit": {"committer": {"date": "2026-09-08T12:00:00Z"}}}\n' > "$STUB_DIR/commit.json"
printf '[{"id": 9, "submitted_at": "2026-09-08T12:01:00Z", "state": "COMMENTED", "user": {"login": "me"}, "body": ""}]\n' > "$STUB_DIR/reviews.json"
printf '[{"id": 2, "created_at": "2026-09-08T12:01:00Z", "user": {"login": "me"}, "body": "Fixed in bbbbbbb2."}]\n' > "$STUB_DIR/comments.json"
EOF

run_driver --max-rounds 2
echo "---- scenario 7b exit=$RC ----"
echo "$OUT"
assert_eq "one round ran" 1 "$(field .rounds)"
assert_eq "the engine's own replies do not count as a review of the push" true "$(field .awaiting_review)"

# ---------- scenario 7c: a clean re-review is a thumbs-up reaction on the PR, not a review ----------
new_stub_dir clean-reaction
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T9 false proxy/src/api.ts 'finding')"
write_threads "$STUB_DIR/threads.after.json" "$(thread T9 true proxy/src/api.ts 'finding')"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
sed 's/aaaaaaa1/bbbbbbb2/' "$STUB_DIR/pr.json" > "$STUB_DIR/pr.json.tmp" && mv "$STUB_DIR/pr.json.tmp" "$STUB_DIR/pr.json"
printf '{"commit": {"committer": {"date": "2026-09-08T12:00:00Z"}}}\n' > "$STUB_DIR/commit.json"
printf '[{"id": 3, "content": "+1", "created_at": "2026-09-08T12:09:00Z", "user": {"login": "reviewer-bot"}}]\n' > "$STUB_DIR/reactions.json"
EOF

run_driver --max-rounds 2
echo "---- scenario 7c exit=$RC ----"
echo "$OUT"
assert_eq "one round ran" 1 "$(field .rounds)"
assert_eq "a thumbs-up on the PR after the push counts as the reviewer reacting" false "$(field .awaiting_review)"
assert_eq "the reviewed head is clean" '[]' "$(field .remaining)"
assert "the reaction came through the graphql wait query" grep -q 'reactions(' "$STUB_DIR/gh.log"

# ---------- scenario 7d: the bot's "eyes" reaction means it is still reviewing ----------
new_stub_dir eyes-reaction
write_pr aaaaaaa1
write_commit_time "2026-09-08T12:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json"
printf '[{"id": 4, "content": "eyes", "created_at": "2026-09-08T12:01:00Z", "user": {"login": "reviewer-bot"}}]\n' > "$STUB_DIR/reactions.json"

run_driver --max-rounds 1
echo "---- scenario 7d exit=$RC ----"
echo "$OUT"
assert_eq "an eyes reaction does not end the wait" true "$(field .awaiting_review)"

# ---------- scenario 7e: a review of the previous head, submitted after a late push, does not count ----------
new_stub_dir late-push
write_pr bbbbbbb2
write_commit_time "2026-09-08T12:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json"
printf '[{"id": 9, "submitted_at": "2026-09-08T12:10:00Z", "state": "COMMENTED", "user": {"login": "reviewer-bot"}, "commit_id": "aaaaaaa1", "body": "review of the old head"}]\n' > "$STUB_DIR/reviews.json"

run_driver --max-rounds 1
echo "---- scenario 7e exit=$RC ----"
echo "$OUT"
assert_eq "a review made on another commit is not a review of this head" true "$(field .awaiting_review)"

# ---------- scenario 7f: a review made on the head counts even when its time is older than the commit time ----------
new_stub_dir head-review
write_pr bbbbbbb2
write_commit_time "2026-09-08T12:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json"
printf '[{"id": 9, "submitted_at": "2026-09-08T12:00:30Z", "state": "COMMENTED", "user": {"login": "reviewer-bot"}, "commit_id": "bbbbbbb2", "body": "review of this head"}]\n' > "$STUB_DIR/reviews.json"

run_driver --max-rounds 1
echo "---- scenario 7f exit=$RC ----"
assert_eq "a review made on the head counts" false "$(field .awaiting_review)"

# ---------- scenario 7g: the PR head lags the branch ref after a push ----------
new_stub_dir stale-pr-head
write_pr aaaaaaa1
write_commit_time "2026-09-08T10:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json" "$(thread T10 false proxy/src/api.ts 'finding')"
write_threads "$STUB_DIR/threads.after.json" "$(thread T10 true proxy/src/api.ts 'finding')"
printf '[{"id": 9, "submitted_at": "2026-09-08T11:00:00Z", "state": "COMMENTED", "user": {"login": "reviewer-bot"}, "commit_id": "aaaaaaa1", "body": "review"}]\n' > "$STUB_DIR/reviews.json"
cat > "$STUB_DIR/round-action.sh" <<'EOF'
cp "$STUB_DIR/threads.after.json" "$STUB_DIR/threads.json"
printf 'bbbbbbb2' > "$STUB_DIR/branch-head"
printf '{"commit": {"committer": {"date": "2026-09-08T12:00:00Z"}}}\n' > "$STUB_DIR/commit.json"
EOF

run_driver --max-rounds 2
echo "---- scenario 7g exit=$RC ----"
echo "$OUT"
assert_eq "the push is seen on the branch ref even though the PR head lags" '["bbbbbbb2"]' "$(field .pushed)"
assert_eq "a branch head the reviewer has not seen is awaiting review, not clean" true "$(field .awaiting_review)"

# ---------- scenario 8: a PR pushed after its last review is not read as clean ----------
new_stub_dir fresh-push
write_pr aaaaaaa1
write_commit_time "2026-09-08T12:00:00Z"
write_comment_time "2026-09-08T11:00:00Z"
write_threads "$STUB_DIR/threads.json"

run_driver --max-rounds 2
echo "---- scenario 8 exit=$RC ----"
echo "$OUT"
assert_eq "no round runs" 0 "$(field .rounds)"
assert_eq "the unreviewed head is reported as awaiting review" true "$(field .awaiting_review)"

echo
if [ "$FAILS" -eq 0 ]; then echo "ALL TESTS PASSED"; else echo "$FAILS TEST(S) FAILED"; exit 1; fi
