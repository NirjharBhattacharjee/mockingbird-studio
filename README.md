# mockingbird-studio

A local app that runs a video channel's pipeline, from a morning scan of the
news to a finished film, its shorts and the upload sheet. You talk to an agent
(Claude Desktop is the one I use). The agent does the work through
[mockingbird](https://github.com/NirjharBhattacharjee/mockingbird)'s tools and
this app's. The app keeps the state, shows every preview, holds the render
jobs, and waits for your yes at each step.

Nothing is built yet. This README is the plan, written after doing the whole
pipeline by hand once, on 24 November 2026.

> The Claude Code plugin that used to live here moved into mockingbird's repo
> as [`plugin/`](https://github.com/NirjharBhattacharjee/mockingbird/tree/main/plugin)
> (install `mockingbird@mockingbird`). Its old files stay in this repo until the
> app's first code lands, and nothing updates them.

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
                  └──▶ a page you keep open: every step, every preview, approve or redo
```

**One folder per video, one layout.** `project.toml` records each step's
state, its files and who approved what, when. A new session asks the app
where things stand instead of reading transcripts.

**Steps are gates.** Research, script, voice, look, sound, assets, previews,
shorts, film, upload. A step that needs your yes stops until it has one. You
approve or ask for a redo, with a note, on the page. The agent picks up the
note through the app's MCP tools.

**Previews in one place.** Audio players for voice and mix, the mood boards
side by side, contact sheets and MP4s for every render, the script with its
changes. Every clip is converted to something the page can play.

**The app owns the render queue.** It starts mockingbird renders itself, so
they keep going when a chat ends, and it can run two at a time.

**The morning scan runs on its own.** At a time you set, the app pulls the
feeds and asks an agent from the command line (`claude -p` today, any CLI you
configure) for a short list of topics. You see the list when you open the
page.

**No model inside.** The app never calls an LLM API. The agent is whatever MCP
client you use, so nothing here is tied to Claude.

**Built in Rust.** One binary: an MCP server, a job runner and a small web
server. The page is plain HTML and JavaScript served on `localhost`, and
Safari's "Add to Dock" gives it its own window. Voice, score and research
tools stay as they are and run as subprocesses.

## What it won't do

- Render video. mockingbird does that.
- Upload anything for you. The upload sheet is for you to paste.
- Go online except for what you asked for: the feeds you listed, and assets
  you approved for download.

## License

MIT
