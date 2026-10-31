# Architecture

The plugin holds judgement; mockingbird holds mechanics.

```
you ──"make shorts from talk.mp4"──▶ Claude Code
                                       │  loads skills/ (what to cut, where, why)
                                       │  asks agents/reviewer to check the result
                                       ▼
                         mockingbird mcp (on your Mac)
                transcribe · frames · captions · render · dictionary
                                       │
                         whisper-server + ffmpeg + Core Text
                                       ▼
                         talk-short-1.mp4 beside the source
```

| Part | Lives in | Does |
|---|---|---|
| Skills | this repo, `skills/` | Know-how: which moments make a short, how to hook, how tight to cut, where captions go, when music hurts. Plain Markdown that Claude reads. |
| Scripts | `skills/*/scripts/` | Measurements Claude can't make by reading text: where the pauses are (`pauses.sh`, `clips.sh`), a finished video's pace and loudness (`measure.sh`). POSIX shell over ffmpeg. |
| Reviewer | `agents/reviewer.md` | Checks a render in a fresh context, so the agent that made the choices isn't the one grading them. |
| Engine | [mockingbird](https://github.com/NirjharBhattacharjee/mockingbird), via `.mcp.json` | Transcribes with local Whisper, cuts and frames with ffmpeg, draws captions with Core Text. It makes no editing choices. |

The plugin imports nothing from mockingbird and mockingbird knows nothing
about the plugin. They meet at the MCP tools, which
[mockingbird's MCP.md](https://github.com/NirjharBhattacharjee/mockingbird/blob/main/docs/MCP.md)
documents. Anything another editor could reuse (a new render option, a faster
transcription) goes into mockingbird; anything that's an editing opinion goes
here.

## Where your footage goes

Rendering is local, and the video never leaves your Mac. But Claude is a cloud
model, and what the tools return goes to it, as it would if you pasted it into
a chat: the transcript, the word times, and every still Claude looks at,
including the reviewer's. That's what lets it choose the moments and check the
framing. Don't run this on footage you wouldn't show Claude.

## Why no animation renderer

Remotion and Motion Canvas render through a browser so captions can bounce and
graphics can move. Everything here is a still per caption page or title, laid
over the video by ffmpeg, which renders a 10-second short in about three
seconds. A renderer would add Chromium and a Node project for one effect (the
pop-in on each caption page). If a later version needs keyframed motion, it
goes into mockingbird as a render option, not into this repo.
