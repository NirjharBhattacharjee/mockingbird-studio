# PRD: the first build

Status: draft, waiting for the owner's approval. Written 25 November 2026,
and revised the same day after the owner's decisions.

Read [README.md](../README.md) first. It says what the app is and what went
wrong on 24 November. This document says what the first build has to do, in
what order, and where the owner's notes met the rules in
[AGENTS.md](../AGENTS.md). [DESIGN.md](./DESIGN.md) says how it's built.
[PLAN.md](./PLAN.md) lists the tasks.

## Who it's for

One person who runs one or more video channels from one Mac and talks to an
agent in Claude Desktop. Any other MCP client works the same way.

## What's asked for

| # | Ask | Planned | Where |
|---|---|---|---|
| 1 | Ask it to find topics for videos | Yes | M4 |
| 2 | The latest trends for every niche you pick | Yes, from the feeds you list | M4, F2 |
| 3 | Pick niches and projects, with a separate workflow per project | Yes | M1 has one workflow, M3 has niche settings, M5 has templates |
| 4 | Preview timelines and leave comments that reach Claude | Yes. Claude reads them; the app can't push them | M1 and M2, F3 |
| 5 | Post to YouTube automatically | Yes, once you've approved the upload step | M3, F1 |
| 6 | A thumbnail step and a description step for every video (added by the owner on 25 November) | Yes | The steps exist from M1. Their checks and the upload are in M3 |

## Where the notes met the rules

Each of these crossed or came near an invariant in AGENTS.md. Each says what
the rule was, what the owner decided, and what the plan does.

### F1. Posting to YouTube

**The rule.** The README said the app won't upload anything for you.
AGENTS.md allowed network use for listed feeds and approved downloads only,
and said no account and no login.

**The decision.** The app posts to YouTube. The change to README.md and
AGENTS.md is in the same pull request as this document:

- README.md: the "won't upload" line is gone, and a paragraph says how
  posting works.
- AGENTS.md: the app still has no account of its own. One sign-in is
  allowed, to your own Google account, for posting. The network rule adds
  the upload you approved. Two invariants are new: nothing is posted without
  an approval of the upload step in `project.toml`, and the Google token
  stays in the Keychain.

**How it works.** You sign in to your Google account once. Every video has
a thumbnail step and a description step, each with a preview, notes, and
Approve or Redo. The upload step shows exactly what will go up and where.
When your approval of it is written to `project.toml`, the app queues a job
that uploads the film and then sets the thumbnail. The gate stays: no
approval, no upload. The app calls no model. The agent writes the title and
description and makes the thumbnail with mockingbird.

**What Google limits.** None of these can be designed away.

- Videos uploaded from an API project that hasn't passed Google's
  [audit](https://developers.google.com/youtube/v3/guides/quota_and_compliance_audits)
  are locked to private, per the reference for
  [`videos.insert`](https://developers.google.com/youtube/v3/docs/videos/insert).
  Until your project passes, the app can put the video on the channel with
  everything filled in, and you press Publish in YouTube Studio. The app
  records the visibility Google set, not the one it asked for.
- A sign-in through an OAuth client that is still in "Testing"
  [expires after seven days](https://support.google.com/cloud/answer/15549945).
  Signing in once needs the client set to "In production".
- Custom thumbnails need a verified channel.
  [`thumbnails.set`](https://developers.google.com/youtube/v3/docs/thumbnails/set)
  answers 403 when the channel may not set them. The video still goes up,
  and the app records that the thumbnail was refused.
- Uploads and thumbnails count against a daily quota per API project.

**Choices this plan makes.** Each one can be changed. Say so if one is wrong.

1. *The app ships no Google client.* You create an OAuth client in your own
   Google Cloud project and give the app its file. A client shipped with the
   app would put every user's uploads on one project, with its quota, its
   audit and its terms.
2. *The app asks for the upload scope only* (`youtube.upload`). It can upload
   a video and set a thumbnail. It can't read, edit or delete anything on the
   channel. One cost: captions aren't uploaded, because that needs a wider
   scope. The captions file stays in the project for you to add.
3. *A project posts once.* Approving the upload step a second time doesn't
   upload a second video. Posting again is a command you run yourself.
4. *An approval recorded through MCP posts private*, whatever the niche says.
   See F5.
5. *The first cut posts the film.* Shorts made inside a film's project are
   posted in M5.

### F2. Trends for every niche

**The rule.** No LLM calls from the app, and network only for the feeds you
listed.

**The decision.** Keep the plan's approach.

- A niche is a name and the feed addresses you typed in. The app fetches
  those addresses and nothing else. It doesn't follow the links inside a
  feed. To follow a YouTube channel or a subreddit, add its RSS address.
- The app doesn't rank. When you ask in a chat, the agent pulls the items
  with the app's tools and ranks them itself. For the morning scan, the app
  runs the command you configured (`claude -p` today), gives it the items
  and reads back a list. With no command set, the scan shows the items
  newest first.

### F3. Comments that go to Claude

**The rule.** The app can't put a message into a Claude Desktop chat. In MCP
the client asks and the server answers. Sending the comment to a model
itself would be an LLM call from the app.

**The decision.** Keep the plan's approach. A comment is saved in
`project.toml` the moment you write it. The agent gets every open comment
with one tool call, and every reply from any of the app's tools ends with a
line saying how many are waiting. In practice you leave your notes, press
Redo, and say "notes are in" in the chat.

A `wait` tool that returns when you press Approve or Redo would let the
agent sit at a gate. Whether Claude Desktop lets a tool call run that long
isn't known, so M1 measures it and the tool is an optional task in M2.
Starting an agent from the command line when you comment isn't planned.
AGENTS.md allows a configured CLI for the morning scan only.

### F4. A workflow per project

Planned as templates: a workflow is a list of steps with gates, copied into
the project when it's made. The app doesn't invent one. That would be an LLM
call. The agent can pick a template or pass its own list of steps when it
creates the project.

A project is one video, as in the README (D2). A niche holds what's shared
across a channel.

### F5. An agent can approve for you

**The rule.** The MCP tools and the CLI must do everything the window does,
so there is an `approve` tool, and an agent could call it without your yes.

**The decision.** Allowed and marked (D3). Every decision records where it
came from (`window`, `cli` or `mcp`). The caller can't set that field, and
the window marks decisions that came through MCP.

**What F1 adds.** Approving the upload step now posts a video. So an
approval of the upload step that came through MCP uploads as private,
whatever the niche says. The worst an agent can do alone is put a private
video on your channel. This guards the Claude Desktop case. An agent with a
shell can run the CLI, which counts as you.

### F6. Two smaller ones

- **Preview copies** (D4). The app calls `ffmpeg` itself to make a small
  H.264 copy for the window, and for nothing else. Rendering stays
  mockingbird's.
- **Pulling feeds** (D5). The app reads the feed files: titles, links,
  dates. Reading the articles behind a topic stays with the research tool
  and the agent.

## Goals

- A new chat finds out where a video stands with one tool call.
- Every preview is in one window, and you can comment on a moment in it.
- A gate holds: nothing after it is accepted until you say yes.
- A render outlives the chat that started it.
- After your yes, the film is on your channel with its title, description
  and thumbnail.
- Each morning there's a short list of topics per niche.

## Not goals

- Editing on a timeline. The window shows and collects comments. The agent
  and mockingbird make the changes.
- Writing titles or descriptions, or designing thumbnails, inside the app.
  The agent does that. The app checks them and shows them.
- Anything in the README's "What it won't do".
- Windows or Linux. The README says one Mac app.
- More than one person using the same projects.

## Milestones

Each one ends with something you can use. M1 is planned in detail and M3's
requirements are below. The later ones will change once M1 has been used on
a real video.

Posting comes before the scan. The scan already exists outside the app, as
the research bot the README describes. Posting exists nowhere, and after M2
there's a film to post.

| | Name | At the end you can |
|---|---|---|
| M1 | One project, one gate, the notes round trip | Have the agent start a project and submit a script, a thumbnail or a preview clip, see it in the window, comment on it, press Redo or Approve, and have the agent pick that up. The same from the CLI with the window closed. |
| M2 | Jobs and previews | Start a mockingbird render that keeps going after the chat ends, two at a time. Play any clip. See a step's clips as one timeline with your comments pinned to it. |
| M3 | Publish | Review the description and the thumbnail as YouTube will show them, approve the upload step, and find the film on your channel. |
| M4 | The scan | List feeds per niche, get a shortlist every morning, pick a topic and start its project. |
| M5 | Workflows | Give each niche its own steps. Run steps side by side. Review and post each short on its own. |

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
9. The built-in workflow has the thumbnail and description steps. In M1 they
   behave like any other step: the thumbnail shows as an image and the
   description as text.
10. M1 uses no network at all.

**Done when** this runs on a synthetic project, once in a headless test and
once by hand with screenshots:

1. The agent creates a project and submits `script/script.md`.
2. The window shows the script waiting. You add two notes and press Redo.
3. The agent reads the notes, submits again and marks them done.
4. You approve. `project.toml` shows both decisions with their times.
5. The agent submits a clip to `previews`. You play it, add a note at 0:04,
   and the agent reads the note with its time.

Not in M1: jobs, preview copies, niches, posting, the scan, workflow
templates, a packaged app.

## M3 requirements

1. A niche has a visibility (private, unlisted or public; private unless
   you change it) and the channel it posts to.
2. The description step takes one sheet: title, description, tags and
   chapters. `submit` checks it against YouTube's limits and refuses a sheet
   that would be rejected. The window shows it as a sheet with a count beside
   each field, not as a file.
3. The thumbnail step takes one image: 1280×720, PNG or JPEG, under 2 MB.
   `submit` checks it. The window shows it full size, and at the size it has
   in a feed, next to the title.
4. You sign in to Google once, from the window or the CLI. The token goes
   in the Keychain and nowhere else.
5. The upload step's review shows the film, the sheet, the thumbnail, the
   channel and the visibility that will be used.
6. Approving the upload step queues the upload. It runs in the job runner,
   so it doesn't depend on the window or a chat.
7. `project.toml` records the post: the video's id and link, the time, the
   visibility asked for and the one Google set, and whether the thumbnail
   was set.
8. Nothing is uploaded before the approval is in `project.toml`. A project
   posts once.
9. No test touches YouTube. The upload is tested against a server the test
   starts.

**Done when** a synthetic film goes up private on a test channel with its
title, description and thumbnail, `project.toml` holds the link, and the
headless test does the same against the local server.

## Decisions

Made by the owner on 25 November 2026 and passed on by the gateway session.

| | Question | Decided |
|---|---|---|
| D1 | Should the app post to YouTube? | Yes. One sign-in to your own Google account, the token kept on this Mac. The upload is a job that starts when the upload step's approval is recorded. Visibility is a niche setting, private by default. README.md and AGENTS.md change in the same pull request. |
| D2 | Is a project one video or a channel? | One video. A niche holds what's shared across a channel. |
| D3 | May an agent record your approval through MCP? | Yes, marked `via = "mcp"` and shown as such in the window. |
| D4 | May the app call `ffmpeg` itself to make preview copies? | Yes, for preview copies only. |
| D5 | Does the app read the feeds itself? | Yes: the feed files only, never the pages they link to. |
| D6 | mockingbird has no render command in its CLI. How does the job runner start a render? | It starts `mockingbird mcp` and calls the tool over stdio. A one-shot command in mockingbird can replace that later. |

## Risks to measure early

| | Risk | Measured in |
|---|---|---|
| S1 | How Claude Desktop treats a local stdio server: start and stop, and how long one tool call may run | M1-4 |
| S2 | WebKit playing and seeking local files through Tauri's asset protocol | M1-5 |
| S3 | A detached job runner surviving Claude Desktop quitting | M2-1 |
| S4 | A five-minute `motion` call over MCP from the runner without a timeout | M2-2 |
| S5 | Whether one sign-in really lasts, with the client "In production" but unverified | M3-4 |
| S6 | Uploading a real film in one resumable session, and what Google sends back from an unaudited project | M3-5 |

## How we'll know it worked

The app has no telemetry, so these are counted by hand on the next real
video and compared with 24 November.

| Measure | 24 November | Target |
|---|---|---|
| Sessions that rebuilt the state from a transcript or the folder | 3 | 0, from M1 |
| Decisions that lived only in a chat or a memory note | at least 2 (the look, the voice) | 0, from M1 |
| Previews opened outside the window | all of them | 0, from M2 |
| Renders lost when a chat ended | it happened; not counted | 0, from M2 |
| Fields typed or pasted into YouTube Studio | every one | 0, from M3. Publish is still pressed by hand while Google's lock applies |
| Briefs written by hand to coordinate panes | 1 | 0, from M5 |
