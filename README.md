# mockingbird-studio

A Claude Code plugin that edits your videos into shorts for YouTube Shorts,
Instagram Reels and TikTok, and into long-form edits for YouTube. Give it a
recording; it picks the moments, cuts the pauses, opens on a hook, captions
every word, frames the speaker, and checks the result before handing it back.
The rendering happens on your Mac, with
[mockingbird](https://github.com/NirjharBhattacharjee/mockingbird).

```
> make two shorts from ~/Movies/talk.mp4
> edit ~/Movies/tutorial.mov for YouTube
> cut this like ~/Downloads/reference.mp4: ~/Movies/vlog.mov
```

## What you get

**Shorts**: 1080x1920 MP4s, 15–60 seconds each, with word-by-word captions
(the spoken word in yellow), the speaker kept in frame, punch-in zooms over
jump cuts, and optional hook title, b-roll, ducked music and sound effects.
A screen recording with a webcam becomes screen on top, face below.

**Long-form**: the whole video with dead air, retakes and tangents cut, a
rebuilt opening, an `.srt` captions file for YouTube, chapters, three titles
and three thumbnail stills to A/B test, and a description draft.

Both come out at −14 LUFS, the loudness the platforms play at.

## Install

You need macOS and mockingbird, with the render options from
[mockingbird#125](https://github.com/NirjharBhattacharjee/mockingbird/issues/125):

```sh
brew install nirjharbhattacharjee/mockingbird/mockingbird
```

Then in Claude Code:

```
/plugin install mockingbird-studio --marketplace NirjharBhattacharjee/mockingbird-studio
```

Before Claude Code 2.1.275, add the marketplace first:
`/plugin marketplace add NirjharBhattacharjee/mockingbird-studio`, then
`/plugin install mockingbird-studio@mockingbird-studio`.

The plugin starts `mockingbird mcp` itself. If you already added mockingbird
with `claude mcp add`, remove that one (`claude mcp remove mockingbird`) so
Claude doesn't see every tool twice.

## What's in it

| | |
|---|---|
| `viral-short` | The shorts workflow, and the bar a short has to clear |
| `long-form` | The long-form workflow: cut, opening, chapters, captions file, titles, thumbnails |
| `hooks` | The first seconds of a short, the first minute of a long video |
| `retention-editing` | Cutting pauses and retakes, where to cut, punch-ins, pace |
| `captions` | Word-by-word captions or an SRT, spelled right, placed clear of faces and buttons |
| `motion-graphics` | Titles, b-roll, zooms, music, sound effects, and when to leave them out |
| `style-match` | Measure a video that worked and match its pace |
| `reviewer` agent | Checks every render: size, loudness, black frames, opening, captions, framing |

## Privacy

Your video is cut and rendered on your Mac and never uploaded. Claude reads
the transcript and the stills it looks at, which go to Anthropic like anything
else you show Claude. See [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md).

## Docs

- [ARCHITECTURE.md](./docs/ARCHITECTURE.md): how the plugin and mockingbird
  split the work.
- [RESEARCH.md](./docs/RESEARCH.md): what the research says about hooks,
  cutting, captions and long-form attention, and how strong each piece is.
- [REVERSE-ENGINEERING.md](./docs/REVERSE-ENGINEERING.md): the numbers taken
  from Remotion's TikTok template and auto-editor.

## Limits

- macOS only, because mockingbird is.
- Captions and titles are stills: no bouncing words, no keyframed motion.
- It edits what you recorded. It can move your best line to the front; it
  can't add a point you didn't make.
- Music, b-roll and sound effects are yours to supply, with the rights to
  use them.

## License

MIT
