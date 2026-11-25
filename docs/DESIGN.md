# Design: the first build

Status: draft, waiting for the owner's approval. Written 25 November 2026.

[PRD.md](./PRD.md) says what gets built and why. This says how. M1 is
designed in full. M2 to M5 are designed far enough that M1's files don't have
to change shape later.

## The shape

There's no daemon, no database, no socket and no port. The state is files.
Every way in runs the same operations from one Rust library.

```
Claude Desktop ──stdio──▶ mockingbird-studio mcp ───┐
a terminal ─────────────▶ mockingbird-studio <op> ──┤
the window (Tauri) ─────▶ invoke("op", …) ──────────┼─▶ core ─▶ project.toml
launchd, at scan time ──▶ mockingbird-studio scan ──┤           ~/.mockingbird-studio/
                          mockingbird-studio runner ┘
                            │  (detached, lives while there are jobs)
                            └─▶ mockingbird mcp · ffmpeg · the tools you declared
```

Why files and no daemon:

- The README's first complaint is that state lived in chat. A file in the
  project folder is state you can read, copy and put under git.
- "Every step works with the window closed" comes free. A test runs the
  binary against a temp folder.
- Claude Desktop starts the MCP server as its own child and stops it when it
  quits. Jobs can't live there, so they need a separate process anyway. A
  runner that exists only while jobs do is less to keep alive than a daemon.

The cost is more than one process writing the same file. Every write takes
an advisory lock on `<file>.lock`, reads, changes, writes a temp file and
renames it over the original. If the window ever needs progress faster than
it can poll, or writes start to contend, that's the point to add a daemon,
and not before.

## Words

| Word | Means |
|---|---|
| Niche | A subject you make videos about: a name, the feeds you listed, a default workflow. |
| Feed | One RSS or Atom address you typed in. |
| Scan | One pull of a niche's feeds. |
| Item | One entry from a feed: title, link, date. |
| Topic | A candidate video on a scan's shortlist. It points at the items behind it. |
| Project | One video. One folder with a `project.toml`. |
| Workflow | A list of steps with gates. It's copied into the project when the project is made. |
| Step | One stage of a project: research, script, voice, and so on. |
| Output | A file the agent submitted for a step. |
| Segment | A named stretch of an audio or video output, such as a chapter or a shot. |
| Note | Something you wrote about a step, a file, or a moment in a file. |
| Decision | Your approve or redo on one round of a step. |
| Job | A long task the app runs for you: a render, a preview copy, a declared tool. |

## Entities

```mermaid
erDiagram
    NICHE ||--o{ FEED : lists
    NICHE ||--o{ SCAN : "is scanned in"
    NICHE ||--o{ PROJECT : groups
    SCAN ||--o{ ITEM : pulls
    SCAN ||--o{ TOPIC : shortlists
    TOPIC }o--o{ ITEM : cites
    TOPIC |o--o| PROJECT : becomes
    WORKFLOW ||--o{ PROJECT : "is copied into"
    PROJECT ||--|{ STEP : has
    STEP ||--o{ OUTPUT : holds
    STEP ||--o{ DECISION : receives
    STEP ||--o{ NOTE : collects
    OUTPUT ||--o{ SEGMENT : "is divided into"
    OUTPUT |o--o{ NOTE : "is pinned by"
    PROJECT |o--o{ JOB : queues
    STEP |o--o{ JOB : "is built by"

    NICHE {
        string id PK
        string name
        string workflow
    }
    FEED {
        string url PK
    }
    SCAN {
        string niche FK
        date day
        string shortlist_by
        string error
    }
    ITEM {
        string link PK
        string feed FK
        string title
        datetime published
    }
    TOPIC {
        string id PK
        string title
        string why
        string state
    }
    PROJECT {
        string path PK
        string title
        string niche FK
        string workflow
        datetime created
    }
    STEP {
        string id PK
        bool gate
        string after
        string state
        int round
    }
    OUTPUT {
        string path PK
        string kind
        float duration
    }
    SEGMENT {
        string label
        float from
        float to
    }
    NOTE {
        int id PK
        string step FK
        string path FK
        float at
        string text
        string state
        string reply
    }
    DECISION {
        string step FK
        int round
        string decision
        datetime at
        string via
        string note
    }
    JOB {
        string id PK
        string kind
        string state
        string project FK
        string step FK
    }
```

## Where each one is stored

| Entity | File | Written by | From |
|---|---|---|---|
| Project, Step, Output, Segment, Note, Decision | `<project>/project.toml` | any process, under the lock | M1 |
| The list of project folders | `~/.mockingbird-studio/config.toml` | any process, under the lock | M1 |
| Job | `~/.mockingbird-studio/jobs/<id>/job.toml` and `log.txt` | the caller creates it, then only the runner | M2 |
| Niche, Feed, settings, declared tools | `~/.mockingbird-studio/config.toml` | any process, under the lock | M3 (tools in M2) |
| Scan, Item, Topic | `~/.mockingbird-studio/scans/<niche>/<day>.json` | the scan | M3 |
| Workflow | built in, or `~/.mockingbird-studio/workflows/<name>.toml` | you | M4 |

`MOCKINGBIRD_STUDIO_HOME` moves `~/.mockingbird-studio`. Tests set it to a
temp folder.

The app writes these files with a serializer, so comments typed into them by
hand don't survive the next write.

## A project folder

```
<project>/
  project.toml
  research/ script/ voice/ look/ sound/ assets/ previews/ shorts/ film/ upload/
  .studio/        the app's own files: preview copies and text rounds, from M2
```

One folder per step. An output's path is relative to the project folder and
has to resolve inside it. A path that leaves the folder, through `..` or a
symlink, is refused.

### project.toml, schema 1

```toml
schema = 1
title = "Example video"
niche = "ai-news"            # optional
workflow = "explainer"
created = 2026-11-25T09:00:00Z
next_note = 3

[[step]]
id = "script"
gate = true
after = ["research"]
state = "redo"               # todo | review | redo | approved | done
round = 1

  [[step.output]]
  path = "script/script.md"
  kind = "text"              # text | pdf | image | audio | video | other

[[step]]
id = "previews"
gate = true
after = ["voice", "look", "assets"]
state = "review"
round = 1

  [[step.output]]
  path = "previews/ch1.mp4"
  kind = "video"
  duration = 61.2            # from M2
  segments = [               # from M2
    { label = "Cold open", from = 0.0, to = 21.4 },
  ]

[[decision]]
step = "script"
round = 1
decision = "redo"            # approved | redo
at = 2026-11-25T09:40:12Z
via = "window"               # window | cli | mcp
note = "The cold open is slow."

[[note]]
id = 1
step = "script"
path = "script/script.md"    # optional
text = "Line 12: say where the number comes from."
created = 2026-11-25T09:39:50Z
round = 1
state = "open"               # open | done

[[note]]
id = 2
step = "previews"
path = "previews/ch1.mp4"
at = 4.0                     # seconds into the file, optional
text = "This card stays up too long."
created = 2026-11-25T11:02:00Z
round = 1
state = "done"
reply = "Cut to 2.5 s in round 2."
```

Times are RFC 3339 with an offset. `decision` rows are never edited or
removed, so the file is the record AGENTS.md asks for: every approval, with
its time.

A step's `output` list is the current round's. Older rounds stay in
`decision` and `note`. Keeping old text so the window can show what changed
between rounds is a task in M2.

### Step states

```mermaid
stateDiagram-v2
    [*] --> todo
    todo --> review: submit, step has a gate
    todo --> done: submit, no gate
    review --> approved: approve
    review --> redo: redo with a note
    approved --> redo: redo with a note
    done --> redo: redo with a note
    redo --> review: submit, step has a gate
    redo --> done: submit, no gate
    approved --> review: submit again
```

Rules:

- `submit` adds one to `round` and replaces the output list.
- `submit` is refused unless every step in `after` is `approved` or `done`.
  This is what makes a gate hold. The app can't stop an agent from writing
  files early, but it won't take them, and from M2 it won't start a job for
  that step.
- `approve` works only from `review`. `redo` needs a note.
- Submitting to an approved step puts it back in `review`. A new round needs
  a new yes.

### The built-in workflow

`explainer`, taken from the README's ten steps and the 24 November run. The
gates and the order are a first guess for the owner to correct.

| Step | Gate | After |
|---|---|---|
| research | yes (the topic) | |
| script | yes | research |
| voice | yes | script |
| look | yes | script |
| sound | yes (the mix) | voice |
| assets | no | script |
| previews | yes | voice, look, assets |
| shorts | yes | previews |
| film | yes | previews, sound |
| upload | yes | film |

Voice, look and assets can run at the same time, as they did on the day.

## Operations

There is one table of operations in the core library. Each has a name, takes
JSON arguments and returns JSON. The CLI, the MCP server and the window are
three thin layers over that table, so they can't drift apart. A test checks
that every operation is a CLI command and an MCP tool.

M1:

| Operation | Arguments | Does |
|---|---|---|
| `project_new` | dir, title, niche?, workflow? | Makes the folder layout and `project.toml`, and adds it to the list. |
| `project_add` | dir | Adds an existing project folder to the list. |
| `projects` | | Every project with each step's state. |
| `status` | project | Steps, states, outputs, the last decision on each step, open notes, and jobs from M2. |
| `submit` | project, step, files, summary? | Records the step's files for a new round. |
| `notes` | project, step? | Open notes, and the note on any step in `redo`. |
| `note_add` | project, step, text, path?, at? | Adds a note. |
| `note_done` | project, id, reply? | Marks a note done. |
| `approve` | project, step, note? | Writes the decision. |
| `redo` | project, step, note | Writes the decision. |

`project` is a folder path, or a folder name when only one project has it.

Each layer sets `via` itself. A caller can't pass it, so an MCP client can't
record a decision as coming from the window.

Later: `job_start`, `jobs`, `job`, `job_cancel` (M2); `niche_add`,
`niche_remove`, `feed_add`, `feed_remove`, `niches`, `scan`, `topics`,
`topics_set`, `topic_pick`, `scan_time` (M3); `workflows` (M4).

### CLI

`mockingbird-studio <operation> [--flags] [--json]`, with dashes for
underscores: `mockingbird-studio note-add --project … --step script --text …`.
With no arguments the binary opens the window. `mcp` and, from M2, `runner`
are the two commands that aren't operations.

### MCP server

`mockingbird-studio mcp` speaks MCP over stdio, the way `mockingbird mcp`
does. Each operation is a tool with a JSON schema. It's built on `rmcp`, the
Rust MCP SDK (3.x on crates.io today), which also has the client M2 needs.

- Nothing but protocol messages goes to stdout. Logs go to stderr.
- In `mcp` mode the binary returns before any Tauri code runs, so no window
  or dock icon appears.
- Every tool reply ends with one line: how many notes are open and which
  steps are waiting on the agent.
- The server's instructions tell the agent to call `status` first in a new
  chat, to stop at a gate, to read `notes` when you say you've reviewed
  something, and to call `approve` only when you said yes in the chat, with
  your words in `note`.
- The server never uses MCP sampling. Asking the client's model for text is
  still the app asking a model.

In Claude Desktop it's one entry in `claude_desktop_config.json`, with the
full path to the binary:

```json
{ "mcpServers": { "mockingbird-studio": { "command": "/path/to/mockingbird-studio", "args": ["mcp"] } } }
```

### The window

Plain HTML, CSS and JavaScript in `ui/`, with no framework, no build step
and nothing loaded from the network. It calls one Tauri command,
`op(name, args)`.

```
┌─────────────┬────────────────────────────────────────┬──────────────────┐
│ Projects    │ script · round 2 · waiting for you     │ Notes            │
│             │ ┌────────────────────────────────────┐ │                  │
│ Example     │ │                                    │ │ ● line 12: say   │
│  research ✓ │ │   the file: text, PDF, image,      │ │   where the      │
│  script   ● │ │   audio or video                   │ │   number comes   │
│  voice    · │ │                                    │ │   from           │
│  look     · │ └────────────────────────────────────┘ │ ✓ 0:04 this card │
│  …          │ files: script.md   script.pdf          │   stays too long │
│             │                                        │                  │
│             │ [ Approve ]  [ Redo with a note… ]     │ [ Add a note ]   │
└─────────────┴────────────────────────────────────────┴──────────────────┘
```

- A file is shown with the element WebKit already has for it: `<pre>`,
  `<img>`, `<audio>`, `<video>`, and `<embed>` for a PDF. Markdown is shown
  as text until someone asks for more.
- "Add a note" on an audio or video file takes the player's current time.
  Clicking a timed note seeks to it.
- Local files reach the page through Tauri's asset protocol. Its scope is
  the project folders in the list, nothing wider.
- Text from notes, agents and feeds is set with `textContent`, never as
  HTML.
- The page asks for `status` every 2 seconds while it's visible. If that
  feels slow, watch the files and emit an event instead.
- Controls are real buttons with labels. Space plays and pauses, `n` adds a
  note at the current time.

## Jobs (M2)

```
~/.mockingbird-studio/jobs/<id>/job.toml   kind, args, project, step, state, times, pid, outputs, error
~/.mockingbird-studio/jobs/<id>/log.txt
~/.mockingbird-studio/jobs/<id>/cancel     its presence asks the runner to stop the job
~/.mockingbird-studio/runner.lock
```

- `job_start` writes `job.toml` with `state = "queued"`. If nobody holds
  `runner.lock`, it starts `mockingbird-studio runner` in its own session,
  with its output going to a log. From then on only the runner writes
  `job.toml`.
- The runner starts queued jobs oldest first, two at a time (a setting).
  When a job's `cancel` file appears it sends SIGTERM to the job's process
  group, then SIGKILL after 5 seconds. It exits after 30 idle seconds.
- A job found `running` when a new runner starts is marked failed with
  `interrupted`. Running it again is a new job.
- States: `queued`, `running`, `done`, `failed`, `cancelled`.
- A job tied to a step is refused under the same rule as `submit`.
- A finished job records its output files. It doesn't submit them. The
  agent looks at them first and submits.

The test that matters: a process queues a job and is killed, and the job
still finishes.

Kinds:

| Kind | Runs |
|---|---|
| `mockingbird` | Starts `mockingbird mcp`, calls one tool (`motion`, `render`, `look`) over stdio and waits. One process per job. See D6. |
| `tool` | A tool declared in `config.toml`, such as the voice or score script, with explicit arguments. |
| `proxy` | `ffmpeg`, to make a preview copy. See D4. |

A `tool` job runs only what you declared:

```toml
[tool.voice]
command = ["/path/to/tts", "--out", "{out}", "--script", "{script}"]
```

Claude Desktop has no shell. A job that took any command line would hand
one to every MCP client. So `job_start` takes a tool's name and values for
its placeholders, never a command.

### Preview copies and the timeline

- On `submit`, `ffprobe` reads each audio and video output's codec and
  length. A file WebKit can't play gets a `proxy` job that writes an H.264
  and AAC copy, 720p at most, to `.studio/proxies/`. The window plays the
  copy and says so. The original is never touched and the copy never goes
  into a film.
- A timeline isn't a new entity. It's a step's outputs in order, each as
  wide as its length, with its segments as ticks and its notes as pins. The
  agent passes the segments with `submit`, since it wrote the shot list.
  Clicking the strip seeks.

## The scan (M3)

```toml
[scan]
time = "07:30"                 # empty: no schedule
command = ["claude", "-p"]     # empty: no shortlist, items only
window_hours = 48

[[niche]]
id = "ai-news"
name = "AI news"
workflow = "explainer"
feeds = ["https://example.com/feed.xml"]
```

- `scan` fetches each feed of a niche: http or https only, a 10-second
  timeout, 5 MB at most, a generic user agent. It keeps entries from the
  last `window_hours` and drops links already seen in the last 7 days of
  scans. It never fetches a link inside a feed.
- With `command` set, the app writes the items and a fixed instruction to
  the command's stdin and reads a JSON list of topics from its stdout. If
  the command fails, the scan keeps the items and records the error.
- Feed text is someone else's text going into a model. It's passed on stdin,
  never as arguments, and the docs say to configure the command with its
  tools turned off.
- When you ask in a chat instead, the agent calls `scan` and `topics`, ranks
  the items itself and saves its shortlist with `topics_set`, so the window
  shows it too.
- `topic_pick` makes a project from a topic with the niche's workflow.
- The schedule is a launchd agent in `~/Library/LaunchAgents/` that runs
  `mockingbird-studio scan --all`. It's written when you set a time and
  removed when you clear it. launchd runs a missed time after the Mac wakes.
- Parsing uses `feed-rs`; fetching uses a small blocking HTTP client.

## Workflows (M4) and upload (M5)

- A workflow template is the `[[step]]` list from `project.toml` without the
  state. `explainer` and `short` are built in. Your own go in
  `~/.mockingbird-studio/workflows/`. A project copies its template when
  it's made, so editing a template doesn't change old projects.
- `status` lists the steps that can be worked on now, which is every step
  whose `after` is satisfied. That replaces the brief written by hand for
  each pane.
- The upload step's outputs are a sheet (title, description, chapters,
  tags), the captions and the thumbnail. The window shows the sheet with a
  copy button on each field. Posting waits on D1.

## Security and privacy

- M1 and M2 make no network requests. M3 adds one kind: GET on the feed
  addresses you listed.
- No telemetry, no update check, no account.
- The window loads nothing remote. Its content security policy allows the
  app's own files and the asset protocol.
- The window can read files only inside the listed project folders.
- Operations refuse paths outside the project folder.
- Jobs run only mockingbird, ffmpeg and the tools you declared.
- The app never opens `~/.mockingbird/data.db`.

## Testing

- Core tests run on temp folders with `MOCKINGBIRD_STUDIO_HOME` set.
  Fixtures are made in the test: text files, and a WAV written from code.
- MCP tests start the binary and speak JSON-RPC over its stdio.
- Tests that need ffmpeg or mockingbird are `#[ignore]`.
- The window holds almost no logic. Its evidence is screenshots in the PR.

## Code layout

```
Cargo.toml      a workspace
core/           the studio-core library: files, operations, CLI, MCP server,
                runner. No Tauri.
app/            the binary: arguments go to core, no arguments opens the window
ui/             index.html, app.js, style.css
```

Two crates, so `cargo test -p studio-core` never builds WebKit. The library
isn't named `core`, which is the name of Rust's own.

Dependencies for M1: `tauri` (the stable 2.x line, not the 3.0 alpha
crates.io also lists), `serde`, `toml`, `serde_json`, `clap`, `rmcp`, and
one time crate. M3 adds `feed-rs` and an HTTP client.

## Left out on purpose

| Not built | Add it when |
|---|---|
| A daemon | polling files is too slow, or writes contend |
| A database | a query over scans or jobs is slow on files |
| A UI framework or a bundler | never, by AGENTS.md |
| A Markdown renderer | reading scripts as plain text gets in the way |
| Pushing notes into a chat | never. `wait` is the nearest thing (PRD F3) |
| Editing on the timeline | never. That's the agent and mockingbird |
| An updater | never. It would be a network call nobody asked for |
