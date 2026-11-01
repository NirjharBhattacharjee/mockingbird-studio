// A motion pass for a 1080x1920 short: mockingbird's `motion` tool calls
// draw() once per frame over the video from `render`. Copy it, then change
// HOOK, the colours and the timings. One file, no imports.
const HOOK = "3 MISTAKES";
const ACCENT = "#FFD500";
const FONT = "SF Pro";

export default function draw(ctx, t, m) {
  const { width: W, height: H } = m;

  // Slow push-in on the video: 1 to 1.12 over the whole short, centred on the face.
  const zoom = m.interpolate(t, [0, m.duration], [1, 1.12], m.ease.inOut);
  const face = { x: W / 2, y: H * 0.4 };
  ctx.save();
  ctx.translate(face.x, face.y);
  ctx.scale(zoom, zoom);
  ctx.drawImage(m.video, -face.x, -face.y, W, H);
  ctx.restore();

  // Hook title: springs in at 0.1 s, slides off at 2.6 s.
  if (t < 3.2) {
    const pop = m.spring(t, { from: 0.6, to: 1, start: 0.1 });
    const away = m.interpolate(t, [2.6, 3.1], [0, -W], m.ease.in);
    ctx.save();
    ctx.translate(W / 2 + away, H * 0.17);
    ctx.scale(pop, pop);
    ctx.rotate(-0.04);
    ctx.font = `900 92px ${FONT}`;
    const width = ctx.measureText(HOOK).width + 100;
    ctx.shadowColor = "rgba(0, 0, 0, 0.45)";
    ctx.shadowBlur = 30;
    ctx.shadowOffsetY = 12;
    ctx.fillStyle = ACCENT;
    ctx.beginPath();
    ctx.roundRect(-width / 2, -95, width, 190, 36);
    ctx.fill();
    ctx.shadowColor = "transparent";
    ctx.fillStyle = "#111";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(HOOK, 0, 0);
    ctx.restore();
  }

  // Captions from `captions` (perCue 3, lit on): each cue pops in, the spoken
  // word (*marked*) in the accent colour, words laid out centred.
  const cue = m.cues.find((c) => t >= c.start && t < c.end);
  if (cue) {
    const pop = m.spring(t, { from: 0.85, to: 1, start: cue.start, damping: 14 });
    const words = cue.text.split(/\s+/).map((w) => ({ lit: /^\*.+\*$/.test(w), text: w.replace(/\*/g, "").toUpperCase() }));
    ctx.save();
    ctx.translate(W / 2, H * 0.7);
    ctx.scale(pop, pop);
    ctx.font = `900 96px ${FONT}`;
    ctx.textBaseline = "middle";
    ctx.lineJoin = "round";
    ctx.lineWidth = 20;
    ctx.strokeStyle = "black";
    const space = ctx.measureText(" ").width;
    const widths = words.map((w) => ctx.measureText(w.text).width);
    const total = widths.reduce((a, b) => a + b, 0) + space * (words.length - 1);
    // Shrink a long cue to 90% of the width rather than run off the edges.
    const fit = Math.min(1, (W * 0.9) / total);
    ctx.scale(fit, fit);
    let x = -total / 2;
    words.forEach((w, i) => {
      ctx.strokeText(w.text, x, 0);
      ctx.fillStyle = w.lit ? ACCENT : "white";
      ctx.fillText(w.text, x, 0);
      x += widths[i] + space;
    });
    ctx.restore();
  }

  // Progress bar, clear of the apps' buttons at the bottom.
  ctx.fillStyle = "rgba(255, 255, 255, 0.25)";
  ctx.fillRect(60, H * 0.86, W - 120, 12);
  ctx.fillStyle = ACCENT;
  ctx.fillRect(60, H * 0.86, (W - 120) * (t / m.duration), 12);
}
