# Plan: tasks and merge order

Status: draft, waiting for the owner's approval. Written 25 November 2026.

The tasks for [PRD.md](./PRD.md), built the way [DESIGN.md](./DESIGN.md)
says. Each task is one issue and one PR under 30 files. The rating says which
coder takes it: hard, medium or easy.

## Rules for every task

- Read README.md, AGENTS.md and DESIGN.md first. If a task seems to need
  something an invariant forbids, stop and say so.
- Merge in the order given. Rebase only the PR that's next to merge.
- One coder at a time, except where a milestone says tasks can overlap.
- Tests use fixtures made in the test. No footage, no network.

## M1. One project, one gate, the notes round trip

Order: 1, 2, then 3, 4 and 5, then 6.

Task 1 declares every dependency M1 needs and leaves stubs for the CLI, the
MCP server and the window. After task 2 merges, tasks 3, 4 and 5 touch
separate files (`core/src/cli.rs`, `core/src/mcp.rs`, and `app/` with `ui/`)
and can overlap. If they run one at a time, the order is 3, 4, 5.

| # | Task | Rating | After | Files |
|---|---|---|---|---|
| M1-1 | Scaffold | medium | | about 18 |
| M1-2 | Core: `project.toml` and the operations | hard | M1-1 | about 10 |
| M1-3 | CLI | easy | M1-2 | about 4 |
| M1-4 | MCP server | medium | M1-2 | about 5 |
| M1-5 | Window | medium | M1-2 | about 8 |
| M1-6 | The loop, end to end | easy | M1-3, M1-4, M1-5 | about 4 |

**M1-1 Scaffold.** A Cargo workspace with `core/` (the `studio-core`
library, no Tauri) and `app/` (Tauri 2, the `mockingbird-studio` binary),
and `ui/index.html` showing an empty window. `main` sends `mcp` to a stub in
core, any other argument to a CLI stub, and no arguments to the window,
returning before Tauri starts in the first two cases. Declare the M1
dependencies now so later PRs leave `Cargo.toml` and `Cargo.lock` alone.
Keep the icon set to the files Tauri needs. Add one CI workflow on a macOS
runner: format, lint, tests.
Done when: the three checks pass locally and in CI, `mockingbird-studio`
opens a window, and `mockingbird-studio mcp` exits without one.

**M1-2 Core.** In `core/`: the `project.toml` schema 1 types, read and write
under a lock with a temp file and a rename, the step states and their rules,
notes, decisions, the path check, the list of projects in `config.toml`, the
built-in `explainer` workflow, and the table of ten operations with JSON in
and JSON out. `via` is an argument of the dispatch function, not of any
operation.
Done when: tests cover every arrow in the state diagram, a refused `submit`
past an open gate, a path that leaves the folder, and two processes writing
the same project without losing a write.

**M1-3 CLI.** One command per operation, generated from the table, with
`--json`. Honour `MOCKINGBIRD_STUDIO_HOME`. Errors go to stderr with a
non-zero exit.
Done when: a test runs the binary through new, submit, note-add, redo,
notes, note-done, submit and approve on a temp folder, and checks
`project.toml`.

**M1-4 MCP server.** `mockingbird-studio mcp` over stdio with `rmcp`: one
tool per operation with a schema, the instructions text, and the line about
waiting notes on every reply. Add `docs/CLAUDE_DESKTOP.md` on registering it.
Add the test that every operation is both a CLI command and an MCP tool.
Measure S1: how Claude Desktop starts and stops the server, and how long a
tool call may run. Write the numbers in the PR.
Done when: a test starts the binary, lists the tools and runs the M1-3 loop
over JSON-RPC. The PR shows Claude Desktop listing the tools.

**M1-5 Window.** `ui/` and `app/`: the layout in DESIGN.md, the `op`
command, the asset protocol scoped to the listed project folders, the
content security policy, a preview by file kind, notes with a time taken
from the player, Approve, Redo with a note, and the 2-second refresh.
Measure S2: playing and seeking a local H.264 file.
Done when: the PR has screenshots of a script in review, a note pinned at a
time on a clip, and the step after Approve. A decision made in the window
has `via = "window"` in `project.toml`.

**M1-6 The loop, end to end.** One test that plays the PRD's five "done
when" steps with the MCP server as the agent and the CLI as the person. A
"Try it" section in the README, and the README's "Nothing is built yet"
line brought up to date.
Done when: the test passes and the README steps work on a clean checkout.

## M2. Jobs and previews

Order: 1, 2, 3, 4, 5. M2-6, M2-7 and M2-8 depend only on M1 and can go in
any gap.

| # | Task | Rating | After | Notes |
|---|---|---|---|---|
| M2-1 | Jobs: files, the detached runner, two at a time, cancel, recovery, the `tool` kind, four operations | hard | M1 | Measures S3. Test: the process that queued a job is killed and the job finishes. |
| M2-2 | The `mockingbird` job kind: an MCP client over stdio to `mockingbird mcp` | medium | M2-1 | Measures S4. Tests are `#[ignore]`. Needs D6. |
| M2-3 | Preview copies: `ffprobe` on submit, the `proxy` job, lengths | medium | M2-1 | Needs D4. |
| M2-4 | Window: the jobs page with the queue, the end of the log and cancel | easy | M2-1 | |
| M2-5 | Timeline: segments on submit, the strip, pins, click to seek | medium | M2-3 | |
| M2-6 | Text rounds: keep each round's text, show what changed | medium | M1 | |
| M2-7 | Packaging: the `.app`, and `setup` that writes the Claude Desktop entry after asking | medium | M1 | |
| M2-8 | `wait`: a tool that returns when you decide | easy | M1-4 | Only if S1 shows Claude Desktop allows a call long enough to be useful. |

## M3. Niches and the scan

Order: 1, 2, 3, 4, 5.

| # | Task | Rating | After | Notes |
|---|---|---|---|---|
| M3-1 | Niches and feeds in `config.toml`, with their operations | easy | M2 | |
| M3-2 | Scan: fetch the listed feeds, parse, the time window, drop seen links, the scan file, `scan` and `topics` | medium | M3-1 | Tests read feeds from files. Needs D5. |
| M3-3 | Shortlist: the configured command, `topics_set` for an agent, `topic_pick` into a project | medium | M3-2 | |
| M3-4 | Schedule: the launchd agent for the scan time | medium | M3-2 | |
| M3-5 | Window: the Today page and the niche and feed settings | medium | M3-3 | |

## M4. Workflows

A first cut. Split again when M2 has been used on a real video.

| # | Task | Rating | After |
|---|---|---|---|
| M4-1 | Workflow templates, the `short` built-in, a niche's default | medium | M3 |
| M4-2 | Steps side by side: what can be worked on now, who has claimed it, shown in the window | medium | M4-1 |
| M4-3 | Shorts: a decision per output | medium | M4-2 |

## M5. Upload

| # | Task | Rating | After | Notes |
|---|---|---|---|---|
| M5-1 | The upload sheet in the window, with copy buttons | easy | M4 | |
| M5-2 | Post to YouTube | hard | M5-1 | Blocked on D1. Not to be started until the owner has changed README.md and AGENTS.md. |

## Open before M1 starts

The six decisions in the PRD. M1 can start on the recommendations for D2 and
D3. A different answer later means renaming a field or refusing one tool.
