---
name: motion-graphics
description: Add titles, b-roll, zooms, music and sound effects to a mockingbird render without making it look busy or cheap. Use when a short or long-form edit needs a hook title, section titles, cutaway pictures or clips, background music, or a whoosh on a cut.
---

# Motion graphics

Each extra on screen costs attention the speech needs, and music or sound
effects under an explanation measurably lower what people remember (Mayer's
coherence principle). Add something only when it does a job: says the hook to
someone with the sound off, shows what the speaker names, marks a turn. Leave
it out when in doubt.

All times here are in seconds of the **finished** video, which is the sum of
the clips before it. The captions SRT shows where each line lands.

## Text: `texts`

`[{ text, from, to, y }]`, drawn in the same style as the captions.

- **Hook title** (shorts): six words or fewer, from 0 to 2–3 s, `y` 0.2.
- **Section title** (long-form): two to four words at a chapter's start, for
  2–3 s, only when the topic changes sharply.
- **A number or term** the speaker says that the viewer should keep: on for
  as long as it's being talked about.
- Never two texts at once, never text over the captions' `y`, never under a
  second on screen.

## Zoom: `clips[].zoom`

`zoom` with `focus` punches in on a clip. 1.1–1.2 on alternate jump cuts (see
**retention-editing**); 1.4–1.6 once or twice for the line that matters; 1.5–2
on a screen recording when the speaker points at something small. There's no
animated zoom, so each clip keeps one framing.

## B-roll: `inserts`

`[{ path, from, to }]`: a picture or video over the whole frame while the
speech carries on. Captions stay on top.

- Only where the speech names something you can show: the product, the
  place, the result, the screenshot.
- 1–3 s in a short, up to about 5 s in long-form. Long enough to see, short
  enough not to lose the speaker.
- Not over the hook's first second, unless the b-roll *is* the hook.
- Use the user's own pictures and clips, or ones they have a licence for. The
  plugin downloads nothing; ask the user for files if the edit needs them.

## Music and sound: `audio`

`[{ path, at, volume, loop, duck }]`, mixed under the speech; everything comes
out at −14 LUFS.

- **Music**: `loop: true, duck: true`, `volume` 0.1–0.15. In a short, under the
  whole thing if it fits the mood. In long-form, never under explanation:
  only an intro, outro or a montage with no talking.
- **A sound effect**: `{ path, at }`. A whoosh on a cut into b-roll, a pop on a
  title. One every few seconds at most; on every cut it becomes noise.
- Licensing is the user's call; a song they don't have rights to gets the
  video muted or blocked. YouTube's Audio Library is free for YouTube.

## What mockingbird can't do

Animated captions (a bounce on each word), keyframed motion, shapes, arrows,
transitions other than a cut. That's deliberate; see
[ARCHITECTURE.md](../../docs/ARCHITECTURE.md#why-no-animation-renderer). If the
user needs them, say so and suggest finishing in an editor such as CapCut or
DaVinci Resolve.
