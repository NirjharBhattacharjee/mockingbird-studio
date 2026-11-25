# PRD: the first build

Status: draft, waiting for the owner's approval. Written 25 November 2026.

Read [README.md](../README.md) first. It says what the app is and what went
wrong on 24 November. This document says what the first build has to do, in
what order, and where the owner's notes ask for something the rules in
[AGENTS.md](../AGENTS.md) don't allow. [DESIGN.md](./DESIGN.md) says how it's
built. [PLAN.md](./PLAN.md) lists the tasks.

## Who it's for

One person who runs one or more video channels from one Mac and talks to an
agent in Claude Desktop. Any other MCP client works the same way.

## What the notes ask for

| # | Ask | Planned | Where |
|---|---|---|---|
| 1 | Ask it to find topics for videos | Yes | M3 |
| 2 | The latest trends for every niche you pick | Yes, from the feeds you list | M3, flag F2 |
| 3 | Pick niches and projects, with a separate workflow per project | Yes | M1 has one workflow, M4 has templates |
| 4 | Preview timelines and leave comments that reach Claude | Yes. Claude reads them; the app can't push them | M1 and M2, flag F3 |
| 5 | Post to YouTube automatically | No. It breaks three rules | flag F1, decision D1 |

## Where the notes and the rules disagree

Nothing below is planned around quietly. Each flag says what breaks, what the
plan does instead, and what the owner has to decide.

### F1. Posting to YouTube

This breaks three things:

- The README lists "Upload anything for you" under what the app won't do.
  The upload sheet is for you to paste.
- AGENTS.md allows network use for two things: the feeds you listed and the
  downloads you approved. An upload is neither.
- AGENTS.md says no account and no login. Uploading needs a Google sign-in
  and an API project of your own.

"Automatically" also runs into the gates. A step that needs your yes stops
until it has one, and publishing a video is the step that most needs one.

One fact changes how useful this would be. Google's reference for
[`videos.insert`](https://developers.google.com/youtube/v3/docs/videos/insert)
says videos uploaded from an unverified API project created after 28 July
2020 are locked to private, and lifting that takes a
[compliance audit](https://developers.google.com/youtube/v3/guides/quota_and_compliance_audits)
of the project. So without the audit, the app could put a private video on
the channel, and you would still open YouTube Studio to publish it.

The choices:

1. **Leave it out.** The app makes the upload sheet and the files, and you
   paste. No rule changes. This is what the plan does until D1 is answered.
2. **An uploader you configure, run as a job.** The app holds no Google
   login. You declare an upload tool in the config, the same way as the
   voice and score tools. After you approve the upload step, the job runner
   runs it with the file, the sheet and the visibility as arguments. The
   default visibility is private. This needs the owner to change the README
   line and the network rule in AGENTS.md first.
3. **Upload code inside the app.** Google sign-in, tokens in the Keychain,
   resumable upload. This crosses all three rules and puts an account inside
   the app. Not recommended.

Recommendation: choice 1 now. If you want posting, choice 2 in M5, after you
edit the two documents yourself.

### F2. Trends for every niche

Two ways this could break a rule:

- **A model inside the app.** Picking the stories that matter out of 300
  headlines takes a model, and the app may not call one. The plan keeps the
  two routes the rules allow. When you ask in a chat, the agent pulls the
  items with the app's tools and ranks them itself. For the morning scan, the
  app runs the command you configured (`claude -p` today), gives it the items
  and reads back a list. With no command set, the scan still runs and shows
  the items newest first.
- **Sources you didn't list.** Google Trends, YouTube's trending page or X
  would be network use nobody asked for. A niche is a name and the feed
  addresses you typed in. The app fetches those addresses and nothing else.
  It doesn't follow the links inside a feed. To follow a YouTube channel or a
  subreddit, add its RSS address to the niche.

So "trends" means what your feeds show today, shortlisted by your agent. It
doesn't mean a trends service.

### F3. Comments that go to Claude

The app can't put a message into a Claude Desktop chat. In MCP the client
asks and the server answers. Sending the comment to a model itself would be
an LLM call from the app.

What the plan does: a comment is saved in `project.toml` the moment you write
it. The agent gets every open comment with one tool call, and every reply
from any of the app's tools ends with a line saying how many comments are
waiting. In practice you leave your notes, press Redo, and say "notes are in"
in the chat.

Two things would make it feel more direct. One is a `wait` tool that returns
when you press Approve or Redo, so the agent can sit at a gate. Whether
Claude Desktop lets a tool call run that long isn't known, so M1 measures it
and the tool is an optional task in M2. The other is the app starting an
agent from the command line when you comment. AGENTS.md allows a configured
CLI for the morning scan only, so that one isn't planned.

### F4. A workflow per project

Planned as templates: a workflow is a list of steps with gates, copied into
the project when it's made. If "it will create workflows" means the app
invents one with a model, that's an LLM call from the app and isn't planned.
The agent can pick a template or pass its own list of steps when it creates
the project.

"Project" is used two ways. In the README a project is one video. The notes
could mean a channel. See D2.

### F5. An agent can approve for you

AGENTS.md says the MCP tools and the CLI must do everything the window does.
Approving is something the window does, so there has to be an `approve`
tool, and an agent could call it without your yes. The plan: every decision
records where it came from (`window`, `cli` or `mcp`), the caller can't set
that field, and the window marks decisions that came through MCP. See D3.

### F6. Two smaller ones

- **Preview copies.** The README says every clip is converted to something
  the window can play. AGENTS.md says video rendering is mockingbird's job.
  The plan calls `ffmpeg` directly to make a small H.264 copy for the window,
  and nothing else. See D4.
- **Pulling feeds.** The README says the app pulls the feeds. AGENTS.md says
  research code stays in its own tool. The plan: the app reads the feeds
  (titles, links, dates). Reading the articles behind a topic stays with the
  research tool and the agent. See D5.

## Goals

- A new chat finds out where a video stands with one tool call.
- Every preview is in one window, and you can comment on a moment in it.
- A gate holds: nothing after it is accepted until you say yes.
- A render outlives the chat that started it.
- Each morning there's a short list of topics per niche.

## Not goals

- Editing on a timeline. The window shows and collects comments. The agent
  and mockingbird make the changes.
- Anything in the README's "What it won't do".
- Windows or Linux. The README says one Mac app.
- More than one person using the same projects.

## Milestones

Each one ends with something you can use. Only M1 is planned in detail. The
later ones will change once M1 has been used on a real video.

| | Name | At the end you can |
|---|---|---|
| M1 | One project, one gate, the notes round trip | Have the agent start a project and submit a script or a preview clip, see it in the window, comment on it, press Redo or Approve, and have the agent pick that up. The same from the CLI with the window closed. |
| M2 | Jobs and previews | Start a mockingbird render that keeps going after the chat ends, two at a time. Play any clip. See a step's clips as one timeline with your comments pinned to it. |
| M3 | Niches and the scan | List feeds per niche, get a shortlist every morning, pick a topic and start its project. |
| M4 | Workflows | Give each niche its own steps. Run steps side by side. Review each short on its own. |
| M5 | Upload | Get the upload sheet with copy buttons. Posting to YouTube only if D1 says so. |

## M1 requirements

1. A project is one folder with a `project.toml`, in the layout DESIGN.md
   gives.
2. Through MCP tools the agent can create a project, read where it stands,
   and submit a step's files.
3. A gated step waits. A step can't be submitted until every step it comes
   after has been approved.
4. The window lists projects and their steps, and shows a submitted step's
   files: text, PDF, images, audio and video. Video is H.264 with AAC in M1.
   Other formats show "can't play this yet" until M2.
5. You can leave a note on a step, on a file, or at a time in an audio or
   video file.
6. You can approve, or ask for a redo with a note. Both are written to
   `project.toml` with the time and where they came from.
7. The agent reads the open notes with one tool call and marks each one done
   with a reply.
8. Everything the window does is also a CLI command and an MCP tool. A test
   checks that the three lists match.
9. M1 uses no network at all.

**Done when** this runs on a synthetic project, once in a headless test and
once by hand with screenshots:

1. The agent creates a project and submits `script/script.md`.
2. The window shows the script waiting. You add two notes and press Redo.
3. The agent reads the notes, submits again and marks them done.
4. You approve. `project.toml` shows both decisions with their times.
5. The agent submits a clip to `previews`. You play it, add a note at 0:04,
   and the agent reads the note with its time.

Not in M1: jobs, preview copies, niches, the scan, workflow templates, a
packaged app, the upload sheet.

## Decisions for the owner

M1 can start on the recommendations. D1 only blocks the YouTube task.

| | Question | Recommendation |
|---|---|---|
| D1 | Should the app post to YouTube? It needs changes to the README and AGENTS.md (F1). | Not now. If yes: an uploader you configure, run as a job after your approval, private by default. |
| D2 | Is a project one video, as in the README, or a channel? | One video. A niche holds what's shared across a channel: its feeds and its default workflow. |
| D3 | May an agent record your approval through MCP (F5)? | Yes, marked `via = "mcp"` and shown as such in the window. |
| D4 | May the app call `ffmpeg` itself to make preview copies (F6)? | Yes, for preview copies only. |
| D5 | Does the app read the feeds itself (F6)? | Yes: the feed files only, never the pages they link to. |
| D6 | mockingbird has no render command in its CLI. `render`, `motion` and `look` are MCP tools only. How does the job runner start one? | The runner starts `mockingbird mcp` and calls the tool over stdio. It works with mockingbird as it is. A one-shot command in mockingbird would be simpler, and can replace it later. |

## Risks to measure early

| | Risk | Measured in |
|---|---|---|
| S1 | How Claude Desktop treats a local stdio server: start and stop, and how long one tool call may run | M1-4 |
| S2 | WebKit playing and seeking local files through Tauri's asset protocol | M1-5 |
| S3 | A detached job runner surviving Claude Desktop quitting | M2-1 |
| S4 | A five-minute `motion` call over MCP from the runner without a timeout | M2-2 |

## How we'll know it worked

The app has no telemetry, so these are counted by hand on the next real
video and compared with 24 November.

| Measure | 24 November | Target |
|---|---|---|
| Sessions that rebuilt the state from a transcript or the folder | 3 | 0, from M1 |
| Decisions that lived only in a chat or a memory note | at least 2 (the look, the voice) | 0, from M1 |
| Previews opened outside the window | all of them | 0, from M2 |
| Renders lost when a chat ended | it happened; not counted | 0, from M2 |
| Briefs written by hand to coordinate panes | 1 | 0, from M4 |
