#!/usr/bin/env bash
# Start the 8-pane agent team for one project in a detached tmux session.
# Usage: launch.sh [project-dir]   (defaults to the current directory)
set -euo pipefail

dir=$(cd "${1:-$PWD}" && pwd)
proj=$(basename "$dir")
s=$(printf 'dev-%s' "$proj" | tr '.:' '--')

# Inside tmux, `attach` refuses to nest; switch the current client instead.
go() {
  if [ -n "${TMUX:-}" ]; then
    tmux switch-client -t "$s" && echo "$1: switched to $s (back: tmux switch-client -l)"
  else
    echo "$1: tmux attach -t $s"
  fi
}

if tmux has-session -t "$s" 2>/dev/null; then
  go "already running"
  exit 0
fi

# role  model  effort
team=(
  "gateway      claude-opus-5-5   medium"
  "planner      claude-fable-5-2  xhigh"
  "coder-hard   claude-opus-5-5   high"
  "coder-medium claude-opus-5-5   medium"
  "coder-easy   claude-sonnet-5-5 high"
  "issues       claude-sonnet-5-5 medium"
  "reviewer     claude-opus-5-5   medium"
  "comments     claude-haiku-5-5  medium"
)

role_text() {
  case $1 in
  gateway) cat <<'EOF'
You are the gateway, pane 1. The user talks only to you. Turn each request into work for your peers and send it with SendMessage:
- New feature or unclear scope: the planner first. When its plan is approved, the issues pane opens the issues.
- Each issue: rate it hard, medium or easy and send it to coder-hard, coder-medium or coder-easy.
- An open PR: the reviewer. Text the user will read (PR bodies, replies to review comments): the comments pane.
Run one coder at a time unless two tasks touch separate files. Agree the merge order with the user before coding starts. Give the user one status message per batch. Answer "what's up" from what you already know, and check GitHub only when something is due. Never merge unless the user says to.
EOF
  ;;
  planner) cat <<'EOF'
You are the planner. You plan the build: PRDs, ERDs, product scope and design. Write them under docs/ in the repo, on a branch, through a PR like any other change. Split the work into tasks small enough for PRs under 30 files, each rated hard, medium or easy, with the order they should merge in. Send the plan to the gateway and wait for approval. Don't write app code.
EOF
  ;;
  coder-*) cat <<'EOF'
You are a coder. Take one issue at a time from the gateway. Follow the project's AGENTS.md: a worktree in .worktrees/, branch agent/<name> from origin/main, run its checks, commit with `git commit -s`, open a PR that closes the issue. Wait for CI with one background `gh pr checks <n> --watch --required`. Report the PR URL and head SHA to the gateway once, or the reason if you are blocked.
EOF
  ;;
  issues) cat <<'EOF'
You are the issues pane. Turn the planner's approved plan into GitHub issues with `gh issue create`: one per task, with acceptance criteria, its difficulty, and what it depends on. Send the gateway the list of issue numbers in merge order. Don't create issues for a plan the gateway hasn't approved.
EOF
  ;;
  reviewer) cat <<'EOF'
You are the reviewer. Review the PRs the gateway sends you. If a bot reviewer such as CodeRabbit is set up on the repo, it is the review layer: triage its comments and send the coder one batched list of real fixes. Otherwise run a local /code-review at medium effort. Check the project's hard invariants in AGENTS.md on every PR. Report findings to the gateway, not progress.
EOF
  ;;
  comments) cat <<'EOF'
You are the comments pane. You write the text people will read: PR descriptions, replies to review comments, issue comments, commit messages when asked. Run it through /unslop if the project asks for it. Post only what the gateway hands you, and confirm back once when it's posted.
EOF
  ;;
  esac
}

common="Project: $proj ($dir). You are one of 8 Claude sessions in tmux session $s. Message your peers with SendMessage, using these names: $(for t in "${team[@]}"; do read -r r _ <<<"$t"; printf '%s-%s ' "$proj" "$r"; done)
Stay idle until you are given a task. Don't send progress chatter: report once, when done or blocked. Follow ~/.claude/CLAUDE.md and the project's AGENTS.md; they win over anything here."

# Even grid: 4 columns x 2 rows, panes in team order, row by row.
grid4x2() {
  local W H w h1 h2 ids l c x
  read -r W H <<<"$(tmux display -p -t "$s" '#{window_width} #{window_height}')"
  w=$(((W - 3) / 4)) h1=$(((H - 1) / 2)) h2=$((H - 1 - (H - 1) / 2))
  ids=($(tmux list-panes -t "$s" -F '#{pane_id}' | tr -d %))
  row() { # y height first-id-index; last column takes the leftover width
    local out="" k cw
    for k in 0 1 2 3; do
      x=$((k * (w + 1))); cw=$w; [ $k -eq 3 ] && cw=$((W - x))
      out+="${cw}x$2,$x,$1,${ids[$3 + k]},"
    done
    printf '%s' "${out%,}"
  }
  l="${W}x$H,0,0[${W}x$h1,0,0{$(row 0 $h1 0)},${W}x$h2,0,$((h1 + 1)){$(row $((h1 + 1)) $h2 4)}]"
  c=$(python3 -c 'import sys;c=0
for ch in sys.argv[1]: c=((c>>1)+((c&1)<<15)+ord(ch))&0xffff
print("%04x"%c)' "$l")
  tmux select-layout -t "$s" "$c,$l" >/dev/null
}

run=$(mktemp -d "${TMPDIR:-/tmp}/$s.XXXX")
i=0
for t in "${team[@]}"; do
  read -r role model effort <<<"$t"
  printf '%s\n\n%s' "$common" "$(role_text "$role")" >"$run/$role.md"
  # Full auto: no permission prompts at all. The user asked for this on 2026-11-25.
  # --add-dir: the parent folder (sibling repos) and temp files, so reads there don't ask.
  # Planner: when Fable's limit is hit, fall back to Opus 5.5 (tried again each turn).
  fb=""; [ "$role" = planner ] && fb="--fallback-model claude-opus-5-5"
  cmd="claude --name '$proj-$role' --model $model $fb --effort $effort --dangerously-skip-permissions --add-dir '$(dirname "$dir")' --add-dir '${TMPDIR:-/tmp}' --append-system-prompt-file '$run/$role.md'"
  if [ $i -eq 0 ]; then
    first=$(tmux new-session -d -P -F "#{pane_id}" -s "$s" -c "$dir" -x 240 -y 60 "$cmd")
  else
    tmux split-window -t "$s" -c "$dir" "$cmd"
    tmux select-layout -t "$s" tiled >/dev/null # room for the next split
  fi
  tmux select-pane -t "$s" -T "$role"
  i=$((i + 1))
done

tmux set -t "$s" pane-border-status top >/dev/null
grid4x2
tmux select-pane -t "$first"
go started
