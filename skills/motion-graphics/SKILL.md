---
name: motion-graphics
description: Animate a mockingbird edit with code: titles that spring in, word captions that pop, zooms and pans on the footage, lower thirds, highlight boxes and arrows that draw themselves on, b-roll that slides or fades, progress bars, explainer cards. You write a Canvas 2D script and mockingbird's `motion` tool draws it frame by frame on the Mac. Use whenever a short or long-form edit needs anything on screen to move.
---

# Motion graphics

Motion graphics here are code. You write one TypeScript file whose
`draw(ctx, t, m)` draws a single frame with the browser's Canvas 2D API, and
mockingbird's `motion` tool calls it for every frame and saves an MP4. There's
no browser and no editor: mockingbird draws with macOS's Core Graphics. The
tool's description lists the whole API; this skill is how to use it well.

Each extra on screen costs attention the speech needs, and music or effects
under an explanation measurably lower what people remember (Mayer's coherence
principle, [RESEARCH.md](../../docs/RESEARCH.md)). Animate to do a job: say the
hook with the sound off, show what the speaker names, mark a turn, keep the
eye moving through a long take. Leave it out when in doubt.

## The workflow

1. **Cut first.** `render` makes the cut: clips, framing, b-roll, music and
   sound effects (it ducks and levels them). Don't pass it `captions` or
   `texts` if they'll be animated here, or they'll be drawn twice.
2. **Time it.** Call `captions` with the same clips: SRT timed to the cut,
   with the spoken word `*marked*`. Pass that SRT as `motion`'s `captions` and
   it arrives as `m.cues` (`{ start, end, text }`). Use `perCue: 1` when you
   need every word's own time.
3. **Write the script.** Start from [templates/short.ts](./templates/short.ts):
   push-in zoom, a hook card that springs in and slides away, word captions
   that pop with the spoken word in the accent colour and shrink to fit, and a
   progress bar. Copy it next to the video and change it; don't write one
   from nothing.
4. **Draw.** `motion` with `script` and `path` (the cut). It keeps the cut's
   sound and saves `<name>-motion-N.mp4`. A 6 s pass at 1080x1920 takes about
   4 s on an M3. Iterate on a short stretch with `duration`.
5. **Check.** `frames` over the result, then the **reviewer** agent.

Without a video, `motion` makes graphics on their own (`size`, `duration`):
an intro card, a chart, a full-screen explainer to `inserts` into a `render`.

## Writing the script

- **One file, no imports.** It's read afresh each run, so edit and rerun.
  Everything you need is on `m`: `interpolate`, `spring`, `ease`, `cues`,
  `image(path)`, `video`, `width`, `height`, `duration`, `fps`, `frame`.
- **A function of time.** Compute everything from `t`. No state between
  frames, no `Date`, no `Math.random()` (use a fixed sequence), so a frame
  draws the same every time.
- **The video is already drawn** under each frame. To zoom or pan it, draw
  `m.video` again with a transform: zooming in covers the original. To zoom
  out or move it aside, `ctx.clearRect` first.
- **Wrap each element in `save()` / `restore()`**, so a transform or shadow
  doesn't leak into the next.
- **Sizes from the frame**, `m.width` and `m.height`, so a script works at
  1080x1920 and 1920x1080.
- **Fonts as CSS**: `"900 96px SF Pro"` (the system font, any weight 100 to
  900), or any installed family: `"800 80px Avenir Next"`.

## Motion that looks finished

| Move | How | Numbers |
|---|---|---|
| Pop in | `spring(t, { from: 0.6, to: 1, start })` as the scale | defaults overshoot a little and settle in about a second; `damping: 14` for captions |
| Slide in or out | `interpolate(t, [t0, t1], [from, to], ease.out)` in, `ease.in` out | 0.25–0.5 s |
| Fade | `globalAlpha = interpolate(...)` | 0.2–0.3 s; slower reads as sluggish |
| Push-in | redraw `m.video` scaled 1 → 1.1–1.15 over the take, `ease.inOut` | slow is the point |
| Punch-in | the same, 1 → 1.3 with a spring, on the line that matters | once or twice a short |
| Draw-on line or box | `setLineDash([len, len])`, `lineDashOffset = interpolate(t, [t0, t1], [len, 0])` | 0.4–0.6 s |
| Highlight a word | a `roundRect` behind it in the accent colour, scaled in with a spring | |
| Lower third | a bar that slides in with `ease.out`, the name after it, 4–5 s on screen | |
| B-roll | `m.image(path)` drawn over the frame, sliding or scaling in, under the captions | 1–3 s in a short |

Rules that keep it from looking cheap:

- **One accent colour, one font,** used the same way all the way through.
- **Everything eases.** Nothing moves at constant speed except a progress bar.
- **Things leave as deliberately as they arrive:** slide or fade out, don't
  just vanish.
- **Text gets a shadow or an outline** when it sits on footage, never both
  heavy.
- **Nothing in the apps' way:** in a short, keep text between about 12% and
  80% of the height and off the right-hand strip.
- **One thing moves at a time.** A title arriving while captions pop and the
  footage zooms is noise.

## Sound

Sound goes in `render`'s `audio`, before the motion pass; `motion` keeps it.

- **Music**: `loop: true, duck: true`, `volume` 0.1–0.15. In a short, under
  the whole thing if it fits the mood. In long-form, never under
  explanation: only an intro, outro or a montage with no talking.
- **A sound effect** on a beat the motion lands on: a whoosh as a card slides,
  a pop as a title springs in. Time the animation to the sound's `at`, one
  every few seconds at most.
- Licensing is the user's call; a song they don't have rights to gets the
  video muted or blocked. YouTube's Audio Library is free for YouTube.

## When it fails

`motion` reports `draw failed at 2.40 s (frame 72): …` and saves nothing.
Read the time, fix the script, and render that stretch again with `duration`
before doing the whole video. A colour or font it doesn't know is an error,
not a silent black. It needs a Mac with Apple silicon.
