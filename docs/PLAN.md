# Plan: tasks and merge order

Status: draft, waiting for the owner's approval. Written 25 November 2026,
and revised the same day after the owner's decisions.

The tasks for [PRD.md](./PRD.md), built the way [DESIGN.md](./DESIGN.md)
says. Each task is one issue and one PR under 30 files. The rating says which
coder takes it: hard, medium or easy.

There are 28 tasks in five milestones: M1 the first slice, M2 jobs and
previews, M3 publishing, M4 the scan, M5 workflows.

## Rules for every task

- Read README.md, AGENTS.md and DESIGN.md first. If a task seems to need
  something an invariant forbids, stop and say so.
- Merge in the order given. Rebase only the PR that's next to merge.
- One coder at a time, except where a milestone says tasks can overlap.
- Tests use fixtures made in the test. No footage, no network, and never a
  real upload.

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
built-in `explainer` workflow with its twelve steps (thumbnail and
description among them), and the table of ten operations with JSON in and
JSON out. `via` is an argument of the dispatch function, not of any
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
| M2-2 | The `mockingbird` job kind: an MCP client over stdio to `mockingbird mcp` | medium | M2-1 | Measures S4. Tests are `#[ignore]`. |
| M2-3 | Preview copies: `ffprobe` on submit, the `proxy` job, lengths | medium | M2-1 | |
| M2-4 | Window: the jobs page with the queue, the end of the log and cancel | easy | M2-1 | |
| M2-5 | Timeline: segments on submit, the strip, pins, click to seek | medium | M2-3 | |
| M2-6 | Text rounds: keep each round's text, show what changed | medium | M1 | |
| M2-7 | Packaging: the `.app`, and `setup` that writes the Claude Desktop entry after asking | medium | M1 | |
| M2-8 | `wait`: a tool that returns when you decide | easy | M1-4 | Only if S1 shows Claude Desktop allows a call long enough to be useful. |

## M3. Publish

Order: 1, 2, 3, 4, 5, 6. M3-2 and M3-3 depend only on M1, so they can also
go in a gap in M2.

| # | Task | Rating | After | Files |
|---|---|---|---|---|
| M3-1 | Niches: config, operations, a settings page | medium | M2 | about 8 |
| M3-2 | Description step: the sheet, its checks, its view | medium | M1 | about 7 |
| M3-3 | Thumbnail step: its checks and its preview | easy | M1 | about 5 |
| M3-4 | Google sign-in | hard | M3-1 | about 8 |
| M3-5 | The `youtube` job | hard | M2-1, M3-4 | about 8 |
| M3-6 | Upload step: the review, the automatic start, the post in the window | medium | M3-1, M3-2, M3-3, M3-5 | about 8 |

**M3-1 Niches.** `[[niche]]` in `config.toml` with name, feeds, workflow,
visibility (private unless set) and channel. The `niches`, `niche_add`,
`niche_set`, `niche_remove`, `feed_add` and `feed_remove` operations. A
settings page in the window that lists niches and edits those fields. Feeds
are stored now and used in M4.
Done when: a test sets a niche's visibility from the CLI and reads it back
through MCP, and the PR has a screenshot of the page.

**M3-2 Description step.** The `description/sheet.toml` format. `submit` on
the description step refuses a sheet that breaks a limit in DESIGN.md, with
a message naming the field. Check the limits against YouTube's reference
and say in the PR where each came from. The window shows the sheet as
YouTube lays it out, with counts and copy buttons.
Done when: tests cover each refusal and one good sheet, and the PR has a
screenshot of a sheet in review with a note on it.

**M3-3 Thumbnail step.** `submit` on the thumbnail step refuses anything
but one PNG or JPEG at 1280×720 under 2 MB. The window shows it full size
and at feed size beside the title from the sheet, when there is one.
Done when: tests cover a wrong size, a wrong type and an oversized file, on
images made in the test, and the PR has a screenshot.

**M3-4 Google sign-in.** `youtube_connect`, `youtube_status` and
`youtube_disconnect`, as in DESIGN.md: the browser, PKCE, the loopback
redirect, the `youtube.upload` scope only, the refresh token in the
Keychain. A "Sign in" button on the settings page. `docs/YOUTUBE.md` on
making the OAuth client and setting it to "In production". Measure S5:
sign in, and check eight days later that the token still works. The PR
doesn't wait for that. Add the result to it as a comment afterwards.
Done when: a test runs the whole exchange against a local server and shows
that no reply, file or log holds the token. The PR shows a real sign-in by
hand, with the token blanked.

**M3-5 The `youtube` job.** The five steps in DESIGN.md: the access token,
the resumable upload with a retry inside a broken chunk, the `post` row
written as soon as Google answers, the thumbnail, and each failure's
message. Google's hosts are two constants a test can replace. Measure S6
with one real upload of a synthetic film to a test channel, private.
Done when: tests against a local server cover a clean upload, a chunk that
breaks and carries on, a visibility Google changed, a refused thumbnail, an
expired sign-in and a spent quota. The PR shows the `post` row from the
real upload, with the id blanked.

**M3-6 Upload step.** `submit` on `upload` takes exactly one video. The
review view shows the film, the sheet, the thumbnail, the channel and the
visibility, with "Approve and post". `approve` on `upload` queues the job
when there's no post yet and the channel is signed in. An approval with
`via = "mcp"` posts private. `job_start` refuses a second post without
`again`. The window shows the post and its link, and says so when Google
set a different visibility or refused the thumbnail.
Done when: a headless test goes from three approved steps to a `post` row
against the local server, a second approval makes no second upload, and an
MCP approval on a public niche asks for private. The PR has screenshots of
the review and of the posted state.

## M4. The scan

Order: 1, 2, 3, 4.

| # | Task | Rating | After | Notes |
|---|---|---|---|---|
| M4-1 | Scan: fetch the listed feeds, parse, the time window, drop seen links, the scan file, `scan` and `topics` | medium | M3-1 | Tests read feeds from files. |
| M4-2 | Shortlist: the configured command, `topics_set` for an agent, `topic_pick` into a project | medium | M4-1 | |
| M4-3 | Schedule: the launchd agent for the scan time | medium | M4-1 | |
| M4-4 | Window: the Today page | medium | M4-2 | |

## M5. Workflows

A first cut. Split again when M2 has been used on a real video.

| # | Task | Rating | After |
|---|---|---|---|
| M5-1 | Workflow templates, the `short` built-in, a niche's default | medium | M4 |
| M5-2 | Steps side by side: what can be worked on now, who has claimed it, shown in the window | medium | M5-1 |
| M5-3 | Shorts: a decision per output | medium | M5-2 |
| M5-4 | Post a film's shorts, each with its own sheet | medium | M5-3, M3-6 |

## What changed in this revision

| Before | Now |
|---|---|
| M5-2 Post to YouTube, blocked on D1 | Unblocked and split into M3-4, M3-5 and M3-6 |
| M5-1 The upload sheet | Became M3-2, the description step's view |
| | New: M3-3, the thumbnail step |
| M3-1 Niches and feeds, easy | M3-1, medium: it now carries visibility, the channel and a settings page |
| M3-2 to M3-5, the scan | M4-1 to M4-4. The Today page lost its settings half to M3-1 |
| M4-1 to M4-3, workflows | M5-1 to M5-3 |
| | New: M5-4, posting a film's shorts |
| M1-2 | The built-in workflow has twelve steps, not ten |
