# mockingbird-studio

A local app that runs a video channel's pipeline, from a morning scan of the
news to a finished film and its shorts on your YouTube channel. You talk to
an agent (Claude Desktop is the one I use). The agent does the work through
[mockingbird](https://github.com/NirjharBhattacharjee/mockingbird)'s tools and
this app's. The app keeps the state, shows every preview, holds the render
jobs, and waits for your yes at each step.

Only the first slice is built: projects, the gate on each step, notes, and
Approve or Redo, from the window, the command line and the MCP server. Jobs,
preview copies and the YouTube upload come later. The rest of this README is
the plan, written after doing the whole pipeline by hand once, on 24 November
2026. What gets built first, and in what order, is in [docs/](./docs/): the
[PRD](./docs/PRD.md), the [design](./docs/DESIGN.md) and the
[tasks](./docs/PLAN.md).

> The Claude Code plugin that used to live here moved into mockingbird's repo
> as [`plugin/`](https://github.com/NirjharBhattacharjee/mockingbird/tree/main/plugin).
> Install `mockingbird@mockingbird` from there.

## The pipeline, as it ran on 24 November

One 9-minute news explainer and five shorts, from an empty folder to
upload-ready files, took about eight hours (14:00 to 22:10) and eight Claude
Code sessions in tmux panes.

| # | Step | What it made | How | What I approved |
|---|---|---|---|---|
| 1 | Research | the day's AI stories, then every article on the one I picked | a small bot that reads RSS feeds (Hacker News, Google News, lab blogs, tech press), with a web page and an MCP tool, no API keys | the topic |
| 2 | Style study | how a reference channel writes and directs its videos | transcripts and frame sheets of its videos | |
| 3 | Script | 77 lines of narration with visual notes and sources, as a PDF | Claude, from the research and the style study | the script |
| 4 | Direction | the motion design: look, 3D world, shot list, source cards, a rights check for b-roll | Claude | |
| 5 | Voice | 8:58 of narration at −14 LUFS | Qwen3-TTS on the Mac, one line at a time, checked by Whisper and pitch | the voice (one built-in speaker, after rejecting a cloned one that drifted) |
| 6 | Look | two rounds of three mood boards, the second from my own reference images | mockingbird `look`, 9-second boards | one look |
| 7 | Sound | an original score in eight movements, 118 sound effects tied to words, a mix | a sampler over CC0 orchestral samples, licensed effects | the mix, by ear |
| 8 | Title and thumbnail | a title from 124 trending videos, a 1280×720 thumbnail | search, ImageMagick, a cutout | the title (the thumbnail is still waiting) |
| 9 | Assets | article screenshots, 18 public-domain and CC clips, photos, a credits file | yt-dlp, Wikimedia Commons, a screenshot tool | |
| 10 | Previews | low-resolution renders of every chapter, checked on contact sheets | mockingbird `motion`, shots written as code and timed to words in the narration | |
| 11 | Shorts | five 1080×1920 shorts, 41 to 59 s each | three panes in parallel, each from one hand-written brief | |
| 12 | Film | 9:12 at 1920×1080, ten 60-second chunks joined with the mix | mockingbird `motion`, about 5 minutes of rendering per minute of film | |
| 13 | Upload | captions (.srt with timing), chapters, description, sources, credits | Claude | |

Steps 5, 6, 7, 8 and 9 ran at the same time in different panes.

## What went wrong

The output was good. Getting there was a mess.

- **State lived in chat.** Three sessions ran out of context or were cleared
  mid-build. Each time the next one had to rebuild what was done from the
  folder and the old transcript. Decisions like "Look F" or "the Aiden voice"
  were only in memory notes.
- **Previews were everywhere.** QuickTime windows, Finder, PDFs, "play it for
  me". Some clips were VP9 and wouldn't open on a Mac at all. To see what
  another pane had made, I had to ask a third pane to find it.
- **Panes were coordinated by hand.** I wrote the shorts brief myself, gave
  each pane its folders, and checked on them one by one.
- **Renders died with their session.** A long `motion` render belongs to the
  session that started it. Clear the session and the job is gone.
- **Code was written per video.** The film's build script, caption maker, word
  timer and patches live in that video's folder. The next video starts from
  nothing.
- **Work was spread over four places.** The research bot, the TTS script, the
  channel folder with its sound library, and mockingbird.

## The app

```
 you ──chat──▶ Claude Desktop (or any MCP client)
                  │            │
                  │ MCP        │ MCP
                  ▼            ▼
          mockingbird-studio   mockingbird
          (state, previews,    (transcribe, look,
           jobs, approvals)     motion, render, measure)
                  │
                  ├──▶ the app window: every step, every preview, approve or redo
                  └──▶ YouTube: the film, its description and thumbnail, after your yes
```

**One folder per video, one layout.** `project.toml` records each step's
state, its files and who approved what, when. A new session asks the app
where things stand instead of reading transcripts.

**Steps are gates.** Research, script, voice, look, sound, assets, previews,
shorts, film, thumbnail, description, upload. A step that needs your yes
stops until it has one. You approve or ask for a redo, with a note, in the
app window. The agent picks up the note through the app's MCP tools.

**Previews in one place.** Audio players for voice and mix, the mood boards
side by side, contact sheets and MP4s for every render, the script with its
changes. Every clip is converted to something the window can play.

**The app owns the render queue.** It starts mockingbird renders itself, so
they keep going when a chat ends, and it can run two at a time.

**It posts to YouTube after your yes.** You sign in to your own Google
account once, and the token stays in the Keychain on your Mac. Every video
has a thumbnail step and a description step (title, description, tags,
chapters). The agent makes both and you approve them like anything else.
When you approve the upload step, the app uploads the film to your channel
with its description and thumbnail. Each niche says whether its videos go up
private, unlisted or public, and private is the default. Google keeps
uploads private until the API project you made has passed its audit, so
until then you press Publish in YouTube Studio yourself.

**The morning scan runs on its own.** At a time you set, the app pulls the
feeds and asks an agent from the command line (`claude -p` today, any CLI you
configure) for a short list of topics. You see the list when you open the
app.

**No model inside.** The app never calls an LLM API. The agent is whatever MCP
client you use, so nothing here is tied to Claude.

**Built in Rust with Tauri.** One Mac app: an MCP server, a job runner and
a window. The window is plain HTML and JavaScript in the system's WebKit, not
a bundled browser. Voice, score and research
tools stay as they are and run as subprocesses.

## Try it

Needs a Rust toolchain on a Mac. Build once, then play both sides of the
loop: the command line is you, and `mockingbird-studio mcp` is what your
agent calls (see [docs/CLAUDE_DESKTOP.md](./docs/CLAUDE_DESKTOP.md)).

```sh
rm -rf /tmp/video /tmp/studio-home
cargo build --release
alias studio="$PWD/target/release/mockingbird-studio"
export MOCKINGBIRD_STUDIO_HOME=/tmp/studio-home   # keeps the try-out apart from your projects

studio project-new --dir /tmp/video --title Example
echo "a topic" > /tmp/video/research/topic.md
studio submit --project /tmp/video --step research --files research/topic.md
studio note-add --project /tmp/video --step research --text "name a second source"
studio redo --project /tmp/video --step research --note "one note"
studio notes --project /tmp/video          # what the agent reads
studio note-done --project /tmp/video --id 1 --reply "added"
studio submit --project /tmp/video --step research --files research/topic.md
studio approve --project /tmp/video --step research
grep -A5 decision /tmp/video/project.toml  # both decisions, with their times
studio                                     # the window: the same project, the same buttons
unset MOCKINGBIRD_STUDIO_HOME
```

The same steps run headless in `cargo test --test loop`, with the MCP server
as the agent.

## The scan's shortlist command

`mockingbird-studio scan` pulls each niche's feeds. Run from the command
line, or by the morning schedule, it can also give the new items to a
command you choose, which picks the topics. Set the command in
`~/.mockingbird-studio/config.toml`:

```toml
[scan]
command = ["claude", "-p", "--tools", "", "--strict-mcp-config"]
```

- The command runs exactly as written, with no shell.
- On stdin it gets a fixed instruction, the niche's name, and the new items
  as a JSON list of `{link, feed, title, published}`.
- The items are text from other people's websites, so give the command no
  tools. In the example, `--tools ""` turns off Claude Code's built-in tools
  and `--strict-mcp-config` loads no MCP servers.
- On stdout it prints a JSON list of up to 20 topics, best first:
  `[{"title": "...", "why": "...", "items": ["<a link from the input>"]}]`.
  Text around the list is ignored. If any topic cites a link that wasn't in
  the input, or has a title over one line, the whole list is refused.
- It has 10 minutes and 1 MB of output. If it fails, the scan keeps its
  items and records why in `error`.
- A scan over MCP or from the window doesn't run the command. The agent
  ranks the items and saves them with `topics_set`.

## What it won't do

- Render video. mockingbird does that.
- Post anything without your yes. An upload starts only after you approve
  the upload step.
- Go online except for what you asked for: the feeds you listed, assets you
  approved for download, and the upload to your own channel that you
  approved.
- Ask for an account. The only sign-in is to your own Google account, for
  posting, and everything else works without it.

## License

MIT
