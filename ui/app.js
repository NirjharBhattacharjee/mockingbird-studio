// The window: one Tauri command, `op(name, args)`, and plain DOM.
// Text from projects, notes and agents is set with textContent, never as HTML.
"use strict";

const { invoke, convertFileSrc } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

const GLYPH = { todo: "·", review: "●", redo: "↺", approved: "✓", done: "✓" };
const SAYS = {
  todo: "not started",
  review: "waiting for you",
  redo: "sent back",
  approved: "approved",
  done: "done",
};

const sel = { project: null, step: null, file: null };
let projects = [];
let status = null;
let shown = ""; // what the page last drew, to skip redraws that change nothing
let previewKey = ""; // the file in the preview, so a refresh doesn't reload a playing video
let pinned = null; // the player's time when `n` was pressed

const op = (name, args = {}) => invoke("op", { name, args });

function el(tag, text, attrs = {}) {
  const e = document.createElement(tag);
  if (text != null) e.textContent = text;
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}

function clock(seconds) {
  const s = Math.floor(seconds);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function showError(e) {
  $("error").textContent = e ? String(e) : "";
}

const step = () => status?.steps.find((s) => s.id === sel.step);
const media = () => $("preview").querySelector("audio, video");

async function refresh() {
  try {
    ({ projects } = await op("projects"));
    if (!projects.some((p) => p.project === sel.project)) {
      Object.assign(sel, { project: projects.find((p) => !p.error)?.project ?? null, step: null });
    }
    status = sel.project ? await op("status", { project: sel.project }) : null;
    if (status && !step()) {
      sel.step = (status.steps.find((s) => s.state === "review") ?? status.steps[0])?.id ?? null;
    }
    const outputs = step()?.output ?? [];
    if (!outputs.some((o) => o.path === sel.file)) sel.file = outputs[0]?.path ?? null;
    showError();
  } catch (e) {
    showError(e);
  }
  draw();
}

function draw() {
  const now = JSON.stringify([projects, status, sel, pinned]);
  if (now === shown) return;
  shown = now;
  drawProjects();
  drawStep();
  drawNotes();
}

function drawProjects() {
  const list = $("projects");
  list.replaceChildren();
  if (!projects.length) {
    list.append(el("li", "No projects yet. Ask your agent to start one."));
  }
  for (const p of projects) {
    const li = el("li");
    li.append(el("span", p.title ?? p.project, { class: "title" }));
    if (p.error) li.append(el("p", p.error, { class: "error" }));
    const steps = el("ul");
    for (const s of p.steps ?? []) {
      const b = el("button", `${s.id} ${GLYPH[s.state]}`, { type: "button", class: s.state });
      b.setAttribute("aria-label", `${s.id}, ${SAYS[s.state]}`);
      if (p.project === sel.project && s.id === sel.step) b.setAttribute("aria-current", "true");
      b.onclick = () => {
        Object.assign(sel, { project: p.project, step: s.id, file: null });
        pinned = null;
        refresh();
      };
      const item = el("li");
      item.append(b);
      steps.append(item);
    }
    li.append(steps);
    list.append(li);
  }
}

function drawStep() {
  const s = step();
  const inReview = s?.state === "review";
  $("approve").disabled = $("redo").disabled = !inReview;
  $("heading").textContent = s ? `${s.id} · round ${s.round} · ${SAYS[s.state]}` : "Pick a step";

  const d = s?.last_decision;
  const said = d && `${d.decision} via ${d.via}, ${new Date(d.at).toLocaleString()}${d.note ? `: ${d.note}` : ""}`;
  $("decision").textContent = [s?.summary, said].filter(Boolean).join(" · ");

  const files = $("files");
  files.replaceChildren();
  for (const o of s?.output ?? []) {
    const b = el("button", o.path, { type: "button", "aria-pressed": String(o.path === sel.file) });
    b.onclick = () => {
      sel.file = o.path;
      pinned = null;
      draw();
    };
    files.append(b);
  }
  drawPreview(s?.output?.find((o) => o.path === sel.file), s?.round);
}

// A file is shown with the element WebKit already has for its kind.
function drawPreview(out, round) {
  const key = out ? `${sel.project}|${out.path}|${round}` : "";
  if (key === previewKey) return;
  previewKey = key;
  const box = $("preview");
  box.replaceChildren();
  if (!out) return;
  const src = convertFileSrc(`${sel.project}/${out.path}`);
  switch (out.kind) {
    case "text": {
      const pre = el("pre");
      box.append(pre);
      fetch(src)
        .then((r) => (r.ok ? r.text() : Promise.reject(`${out.path}: ${r.status}`)))
        .then((t) => (pre.textContent = t), showError);
      break;
    }
    case "image":
      box.append(el("img", null, { src, alt: out.path }));
      break;
    case "audio":
    case "video":
      box.append(el(out.kind, null, { src, controls: "", preload: "metadata" }));
      break;
    case "pdf":
      box.append(el("embed", null, { src, type: "application/pdf" }));
      break;
    default:
      box.append(el("p", `No preview for ${out.path}.`));
  }
}

function drawNotes() {
  const list = $("notes");
  list.replaceChildren();
  const notes = (status?.open_notes ?? []).filter((n) => n.step === sel.step);
  if (!notes.length) list.append(el("li", "No open notes on this step.", { class: "empty" }));
  for (const n of notes) {
    const li = el("li");
    if (n.path && n.path !== sel.file) li.append(el("span", n.path, { class: "path" }));
    if (n.at != null) {
      const b = el("button", clock(n.at), { type: "button", "aria-label": `Play from ${clock(n.at)}` });
      b.onclick = () => seek(n);
      li.append(b, " ");
    }
    li.append(el("span", n.text));
    list.append(li);
  }
  $("note-at").textContent = pinned != null ? `at ${clock(pinned)}` : "";
}

function seek(note) {
  if (note.path && note.path !== sel.file) {
    sel.file = note.path;
    draw();
  }
  const m = media();
  if (m) {
    m.currentTime = note.at;
    m.focus();
  }
}

$("note-form").onsubmit = async (e) => {
  e.preventDefault();
  if (!step()) return;
  const m = media();
  const args = { project: sel.project, step: sel.step, text: $("note-text").value };
  if (sel.file) args.path = sel.file;
  if (m) args.at = pinned ?? m.currentTime;
  try {
    await op("note_add", args);
    $("note-text").value = "";
    pinned = null;
    await refresh();
  } catch (err) {
    showError(err);
  }
};

$("approve").onclick = async () => {
  try {
    await op("approve", { project: sel.project, step: sel.step });
    await refresh();
  } catch (e) {
    showError(e);
  }
};

$("redo").onclick = () => $("redo-dialog").showModal();
$("redo-dialog").onclose = async () => {
  const dialog = $("redo-dialog");
  const note = $("redo-text").value;
  if (dialog.returnValue !== "send" || !note.trim()) return;
  try {
    await op("redo", { project: sel.project, step: sel.step, note });
    $("redo-text").value = "";
    await refresh();
  } catch (e) {
    showError(e);
  }
};

// Space plays and pauses; `n` pins a note at the player's current time.
document.addEventListener("keydown", (e) => {
  const m = media();
  const typing = e.target.closest("input, textarea, button, dialog");
  if (!m || typing || e.target === m || e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key === " ") {
    e.preventDefault();
    m.paused ? m.play() : m.pause();
  } else if (e.key === "n") {
    e.preventDefault();
    m.pause();
    pinned = m.currentTime;
    draw();
    $("note-text").focus();
  }
});

refresh();
setInterval(() => document.hidden || refresh(), 2000);
