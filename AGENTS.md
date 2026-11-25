# Agent workflow

Rules for every agent working in this repo. Read [README.md](./README.md)
first: it says what the app is and why it exists.

## What lives here

The app is a Rust Tauri app: an MCP server, a job runner and one window.
The window's UI is plain HTML, CSS and JavaScript, with no framework and no
build step.

Not here:

- Video rendering, transcription, looks, measuring. Those are mockingbird's
  tools. Call them; don't copy them.
- Voice, score and research code. They stay in their own tools and run as
  subprocesses with explicit arguments.
- A user's footage, voice, scripts or channel assets. Projects live outside
  the repo. Tests use synthetic fixtures.

## Hard invariants

If a task, a review comment or your own idea would cross one of these, stop
and say so. Don't implement it quietly.

- No LLM calls from the app. The agent is the user's MCP client. The morning
  scan runs a CLI the user configures.
- No paywall, usage cap, licence key or telemetry. The app has no account of
  its own. The one sign-in is to the user's own Google account, used only to
  post to their YouTube channel, and everything else works without it.
- Network only for what the user asked for: the feeds they listed, downloads
  they approved, and the YouTube upload they approved (Google's token
  address and YouTube's API, nothing else). mockingbird itself stays offline.
- Nothing is posted without an approval of the upload step in
  `project.toml`. A video goes up private unless the niche says otherwise.
- The Google token stays in the macOS Keychain. It never goes into a project
  folder, a config file, a log or a tool's reply.
- Tauri is the only GUI. No Electron, no bundled browser, no other toolkit.
- Never open mockingbird's `data.db`. Talk to mockingbird through its MCP
  server or CLI.
- Every step works with the window closed. The MCP tools and the CLI must do
  everything the window does, so every step can be tested headless.
- A render or long job never depends on the chat that started it.
- Every approval is written to the project's `project.toml` with the time.
- MIT, DCO sign-off, no CLA. Don't copy code under a licence that isn't
  MIT-compatible.

## Workflow

1. Open a GitHub issue first. The PR closes it.
2. One task, one worktree in `.worktrees/` (gitignored), branch `agent/<name>`
   from `origin/main`. Never commit to `main`.
3. Keep PRs small: under 30 changed files if you can, stacked at most one
   level deep.
4. Run the checks below before pushing.
5. Commit with `git commit -s`. Run commit messages, PR text, docs and
   comments through `/unslop`.
6. Evidence goes in the PR: test output, timings, or a screenshot of the
   window before and after.
7. CodeRabbit reviews PRs here. Fix every comment in one push per round, and
   don't push just to start a review. If CodeRabbit can't review, run a local
   `/code-review` at medium.
8. Don't merge unless told to.

## Checks

| Check | Command |
|---|---|
| Format | `cargo fmt --check` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Tests | `cargo test` |

Tests that need mockingbird installed are marked `#[ignore]` and run with
`cargo test -- --ignored`, only for changes to how the app calls it.

## Pacing

Usage is shared across every session. Follow `~/.claude/CLAUDE.md`: one
engineer session per project, the cheaper model for mechanical work, one
background `gh pr checks <n> --watch --required` instead of polling.
