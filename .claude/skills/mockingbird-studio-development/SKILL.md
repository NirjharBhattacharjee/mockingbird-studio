---
name: mockingbird-studio-development
description: Start the 8-pane tmux agent team (gateway, planner, three coders, issues, reviewer, comments) for a mockingbird project. Use when the user types /mockingbird-studio-development or asks to start the dev team.
---

Run, with the project directory the user named (default: the current working directory):

```bash
~/.claude/skills/mockingbird-studio-development/launch.sh [project-dir]
```

Then tell the user the one line it printed. Inside tmux it switches their client to the team itself; outside tmux it prints `tmux attach -t dev-<project>`. Pane 1 is the gateway; they talk to it. Don't start any work yourself.

If it says "already running", it has switched them (or prints the attach line) the same way. To stop the team: `tmux kill-session -t dev-<project>`.
