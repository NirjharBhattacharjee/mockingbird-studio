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
let asked = 0; // the latest refresh; an older reply is dropped

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
  const n = ++asked;
  try {
    const list = (await op("projects")).projects;
    if (n !== asked) return;
    projects = list;
    if (!projects.some((p) => p.project === sel.project)) {
      Object.assign(sel, { project: projects.find((p) => !p.error)?.project ?? null, step: null });
    }
    const project = sel.project;
    const got = project ? await op("status", { project }) : null;
    if (n !== asked || project !== sel.project) return;
    status = got;
    if (status && !step()) {
      sel.step = (status.steps.find((s) => s.state === "review") ?? status.steps[0])?.id ?? null;
    }
    const outputs = step()?.output ?? [];
    if (!outputs.some((o) => o.path === sel.file)) sel.file = outputs[0]?.path ?? null;
    showError();
  } catch (e) {
    if (n !== asked) return;
    status = null;
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
  drawTimeline(s);
}

// The step's audio and video outputs in order, each as wide as its length,
// with its segments as ticks and its open notes as pins. A click seeks.
function drawTimeline(s) {
  const strip = $("timeline");
  strip.replaceChildren();
  const outs = (s?.output ?? []).filter((o) => o.duration > 0);
  strip.hidden = !outs.length;
  const notes = (status?.open_notes ?? []).filter((n) => n.step === sel.step && n.at != null);
  for (const o of outs) {
    const clip = el("div", null, { class: "clip", title: o.path });
    if (o.path === sel.file) clip.classList.add("current");
    clip.style.flexGrow = o.duration;
    const pct = (t) => `${(Math.min(Math.max(t, 0), o.duration) / o.duration) * 100}%`;
    for (const g of o.segments ?? []) {
      const seg = el("span", g.label, { class: "segment", title: `${g.label}, ${clock(g.from)}–${clock(g.to)}` });
      seg.style.left = pct(g.from);
      seg.style.width = `calc(${pct(g.to)} - ${pct(g.from)})`;
      clip.append(seg);
    }
    for (const n of notes.filter((n) => n.path === o.path)) {
      const pin = el("button", null, { type: "button", class: "pin", title: n.text, "aria-label": `Note at ${clock(n.at)}: ${n.text}` });
      pin.style.left = pct(n.at);
      pin.onclick = (e) => {
        e.stopPropagation();
        seek(n);
      };
      clip.append(pin);
    }
    clip.onclick = (e) => {
      const r = clip.getBoundingClientRect();
      seek({ path: o.path, at: ((e.clientX - r.left) / r.width) * o.duration });
    };
    strip.append(clip);
  }
}

// The newest job making this output's preview copy, if the list has one.
const proxyJob = (out) =>
  (status?.jobs ?? [])
    .filter((j) => j.kind === "proxy" && j.values?.out === out.proxy)
    .sort((a, b) => a.id.localeCompare(b.id))
    .pop();

// A file is shown with the element WebKit already has for its kind.
function drawPreview(out, round) {
  const copy = out?.proxy ? (proxyJob(out)?.state ?? "done") : "";
  const key = out ? `${sel.project}|${out.path}|${round}|${copy}` : "";
  if (key === previewKey) return;
  previewKey = key;
  const box = $("preview");
  box.replaceChildren();
  box.classList.remove("thumb", "proxied");
  if (!out) return;
  // status.project is the canonical folder, which is what the scope allows.
  const url = `${convertFileSrc(`${status.project}/${out.path}`)}?r=${round}`; // a new round of the same file name isn't served from cache
  if (out.path === "description/sheet.toml") return drawSheet(box);
  switch (out.kind) {
    case "text":
      drawText(box, out.path);
      break;
    case "image": {
      const full = el("img", null, { src: url, alt: out.path });
      if (sel.step !== "thumbnail") return box.append(full);
      // Beside the title at 168x94, about how a feed shows it. The sheet's
      // title isn't read yet (M3-2), so this is the project's.
      const feed = el("div", null, { class: "feed" });
      feed.append(el("img", null, { src: url, alt: "", width: 168, height: 94 }), el("span", status.title));
      box.append(full, feed);
      box.classList.add("thumb");
      break;
    }
    case "audio":
    case "video": {
      if (!out.proxy) return box.append(el(out.kind, null, { src: url, controls: "", preload: "metadata" }));
      // WebKit can't play the original: play the H.264 copy and say so.
      if (copy === "queued" || copy === "running") return box.append(el("p", "Making a preview copy: this file doesn't play here as it is."));
      if (copy !== "done") return box.append(el("p", `No preview copy: ${proxyJob(out).error ?? copy}.`));
      const src = `${convertFileSrc(`${status.project}/${out.proxy}`)}?r=${round}`;
      box.append(
        el("p", "Preview copy, made because this file doesn't play here as it is. The original is unchanged.", { class: "proxy-label" }),
        el(out.kind, null, { src, controls: "", preload: "metadata" }),
      );
      box.classList.add("proxied");
      break;
    }
    case "pdf":
      box.append(el("embed", null, { src: url, type: "application/pdf" }));
      break;
    default:
      box.append(el("p", `No preview for ${out.path}.`));
  }
}

// A text file as it was submitted this round, not as it is on disk now.
// After the first round, each line is marked kept, gone (−) or new (+).
async function drawText(box, path) {
  const key = previewKey;
  let t;
  try {
    t = await op("round_text", { project: sel.project, step: sel.step, path });
  } catch (e) {
    if (key === previewKey) showError(e);
    return;
  }
  if (key !== previewKey) return;
  const pre = el("pre");
  const diff = el("div", null, { class: "diff" });
  if (!t.kept) diff.append(el("p", "As on disk now: this round was submitted before rounds were kept.", { class: "diff-head" }));
  if (t.diff === "too large") diff.append(el("p", `Too large to show the changes since round ${t.previous_round}.`, { class: "diff-head" }));
  if (!t.changes) {
    pre.textContent = t.text;
    diff.append(pre);
    return box.replaceChildren(diff);
  }
  const count = (c) => t.changes.filter(([m]) => m === c).length;
  const head = el("p", `Changes since round ${t.previous_round}: ${count("+")} new, ${count("-")} gone`, { class: "diff-head" });
  const MARK = { " ": "  ", "+": "+ ", "-": "− " };
  const CLASS = { " ": "same", "+": "add", "-": "del" };
  for (const [m, line] of t.changes) pre.append(el("span", `${MARK[m]}${line}`, { class: CLASS[m] }));
  diff.append(head, pre);
  box.replaceChildren(diff);
}

// The description sheet as YouTube lays it out, each part with its count
// against YouTube's limit and a copy button.
async function drawSheet(box) {
  const key = previewKey;
  let s;
  try {
    s = await op("sheet", { project: sel.project });
  } catch (e) {
    if (key === previewKey) showError(e);
    return;
  }
  if (key !== previewKey) return; // another file was picked meanwhile
  const yes = (b) => (b ? "yes" : "no");
  const part = (label, tag, text, count, limit, unit) => {
    const head = el("div", null, { class: "sheet-head" });
    const copy = el("button", "Copy", { type: "button", "aria-label": `Copy the ${label}` });
    copy.onclick = () => navigator.clipboard.writeText(text).catch(showError);
    head.append(el("h3", label), el("span", `${count} / ${limit} ${unit}`, { class: count > limit ? "over" : "count" }), copy);
    sheet.append(head, el(tag, text));
  };
  const sheet = el("div", null, { class: "sheet" });
  if (!s.kept) sheet.append(el("p", "As on disk now: this round was submitted before rounds were kept.", { class: "declarations" }));
  if (s.error) sheet.append(el("p", `YouTube would refuse this sheet: ${s.error}`, { class: "error" }));
  part("Title", "p", s.title, s.counts.title, s.limits.title, "characters");
  part("Description", "pre", s.description, s.counts.description, s.limits.description, "bytes");
  part("Tags", "p", s.tags.join(", "), s.counts.tags, s.limits.tags, "characters");
  sheet.append(
    el("p", `Category ${s.category} · made for kids: ${yes(s.made_for_kids)} · altered or synthetic content: ${yes(s.altered_content)}`, { class: "declarations" }),
  );
  box.replaceChildren(sheet);
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
    if (!step()?.output.some((o) => o.path === note.path)) return; // not in this round
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
  // A time only when it means something: pinned with `n`, or the player has played.
  const at = pinned ?? (m?.played.length ? m.currentTime : null);
  if (at != null) args.at = at;
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

$("redo").onclick = () => {
  $("redo-dialog").returnValue = ""; // so Escape can't send the last round's "send"
  $("redo-dialog").showModal();
};
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
  const typing = e.target.closest("input, textarea, dialog");
  if (jobsOpen() || !m || typing || e.target === m || e.metaKey || e.ctrlKey || e.altKey) return;
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

// A clicked button gives up focus, so Space plays the video instead of clicking it again.
document.addEventListener("click", (e) => e.target.closest?.("button")?.blur());

refresh();
setInterval(() => document.hidden || refresh(), 2000);

// The jobs page: every job, the end of the selected job's log, and cancel.
// It has its own button, polling and state, apart from the step page above.
const ACTIVE = ["queued", "running"];
let jobList = [];
let jobSel = null;
let jobLog = null; // the selected job as `job` returned it, with its log
let jobsShown = "";
let jobsAsked = 0;
const stopping = new Set(); // jobs cancelled here that haven't changed state yet
let jobsError = ""; // from the last refresh; clears on the next good one
let cancelError = ""; // from a cancel; stays until another job is picked
let pendingStop = null; // the job the confirmation dialog is about

const jobsOpen = () => document.body.classList.contains("jobs");

function setJobsPage(open) {
  document.body.classList.toggle("jobs", open);
  $("jobs-page").hidden = !open;
  $("jobs-button").setAttribute("aria-pressed", String(open));
  if (open) {
    setSettingsPage(false);
    media()?.pause(); // the player is hidden, so it must not keep playing
    refreshJobs();
  }
}

const showJobError = () => ($("job-error").textContent = cancelError || jobsError);

// Every job names itself the same way in the list, the dialog and the label.
const jobWhat = (j) => [j.tool, j.project?.split("/").filter(Boolean).pop(), j.step].filter(Boolean).join(" · ");

async function refreshJobs() {
  const n = ++jobsAsked;
  try {
    const list = (await op("jobs")).jobs.reverse(); // newest first
    if (n !== jobsAsked) return;
    jobList = list;
    if (!list.some((j) => j.id === jobSel)) jobSel = (list.find((j) => j.state === "running") ?? list.find((j) => j.state === "queued") ?? list[0])?.id ?? null;
    for (const id of stopping) if (!list.some((j) => j.id === id && ACTIVE.includes(j.state))) stopping.delete(id);
    jobLog = null; // so a failed read can't leave another job's log under this one
    jobLog = jobSel ? await op("job", { id: jobSel }) : null;
    if (n !== jobsAsked) return;
    jobsError = "";
  } catch (e) {
    if (n !== jobsAsked) return;
    jobsError = String(e);
  }
  showJobError();
  drawJobs();
}

function jobTimes(j) {
  const t = (x) => new Date(x).toLocaleTimeString();
  if (j.ended) return `${t(j.started ?? j.created)} to ${t(j.ended)}`;
  return j.started ? `since ${t(j.started)}` : `queued ${t(j.created)}`;
}

function drawJobs() {
  const now = JSON.stringify([jobList, jobSel, jobLog, [...stopping]]);
  if (now === jobsShown) return;
  jobsShown = now;
  const list = $("jobs");
  list.replaceChildren();
  if (!jobList.length) list.append(el("li", "No jobs yet. Your agent starts them.", { class: "empty" }));
  for (const j of jobList) {
    const li = el("li", null, { class: `job ${j.state}` });
    const pick = el("button", null, { type: "button", "aria-pressed": String(j.id === jobSel) });
    pick.append(
      el("span", j.state, { class: "state" }),
      el("span", jobWhat(j), { class: "what" }),
      el("span", jobTimes(j), { class: "when" }),
    );
    pick.onclick = () => {
      if (jobSel !== j.id) cancelError = "";
      jobSel = j.id;
      refreshJobs();
    };
    li.append(pick);
    if (stopping.has(j.id)) {
      li.append(el("span", "stopping…", { class: "stopping" }));
    } else if (ACTIVE.includes(j.state)) {
      const stop = el("button", "Cancel", { type: "button", "aria-label": `Cancel ${jobWhat(j)}` });
      stop.onclick = () => {
        pendingStop = j;
        $("stop-text").textContent = `Stop ${jobWhat(j)}? It can't be resumed.`;
        $("stop-dialog").returnValue = ""; // so Escape can't stop the last one asked about
        $("stop-dialog").showModal();
      };
      li.append(stop);
    }
    if (j.error) li.append(el("p", j.error, { class: "error" }));
    if (j.outputs?.length) li.append(el("p", `Wrote ${j.outputs.join(", ")}`, { class: "outputs" }));
    list.append(li);
  }
  $("job-heading").textContent = jobLog ? `Log · ${jobLog.tool} ${jobLog.id}` : "Log";
  const log = $("job-log");
  // Follow the end of the log, unless the reader scrolled up in the same job.
  const follow = log.dataset.id !== jobLog?.id || log.scrollTop + log.clientHeight >= log.scrollHeight - 4;
  log.dataset.id = jobLog?.id ?? "";
  log.textContent = jobLog ? jobLog.log || "No output yet." : "";
  if (follow) log.scrollTop = log.scrollHeight;
}

$("jobs-button").onclick = () => setJobsPage(!jobsOpen());
// Picking a step goes back to the step page.
$("projects").addEventListener("click", () => { setJobsPage(false); setSettingsPage(false); }, true);

$("stop-dialog").onclose = async () => {
  const j = pendingStop;
  pendingStop = null;
  if ($("stop-dialog").returnValue !== "stop" || !j) return;
  cancelError = "";
  try {
    await op("job_cancel", { id: j.id });
    stopping.add(j.id);
  } catch (e) {
    cancelError = String(e);
  }
  showJobError();
  refreshJobs();
};

// Every 2 seconds while a job is queued or running, every 10 when nothing is.
async function pollJobs() {
  if (jobsOpen() && !document.hidden) await refreshJobs();
  setTimeout(pollJobs, jobList.some((j) => ACTIVE.includes(j.state)) ? 2000 : 10000);
}
pollJobs();

// The settings page: niches and their feeds. Its own functions and state.
// Every change is one operation; the page then redraws from `niches`.
let nicheList = [];
let settingsShown = "";
let settingsAsked = 0;
const VISIBILITY = ["private", "unlisted", "public"];
let pendingRemove = null; // the niche the confirmation dialog is about

const settingsOpen = () => document.body.classList.contains("settings");

function setSettingsPage(open) {
  document.body.classList.toggle("settings", open);
  $("settings-page").hidden = !open;
  $("settings-button").setAttribute("aria-pressed", String(open));
  if (open) {
    setJobsPage(false);
    media()?.pause();
    $("settings-error").textContent = "";
    refreshNiches();
    refreshChannels();
  }
}

// Runs one niche operation, then redraws. A refused change shows its reason;
// the page then shows what is stored, except text typed and not yet saved.
// `clear` is the input to empty when the change worked.
let cleared = null;
async function nicheOp(name, args, clear = null) {
  $("settings-error").textContent = "";
  let failed = "";
  try {
    await op(name, args);
  } catch (e) {
    failed = String(e);
  }
  cleared = failed ? null : clear;
  await refreshNiches(true);
  if (failed) $("settings-error").textContent = failed;
}

async function refreshNiches(force = false) {
  const n = ++settingsAsked;
  try {
    const list = (await op("niches")).niches;
    if (n !== settingsAsked) return;
    nicheList = list;
    $("settings-error").textContent = "";
  } catch (e) {
    if (n !== settingsAsked) return;
    $("settings-error").textContent = String(e);
  }
  if (force) settingsShown = "";
  drawNiches();
}

function drawNiches() {
  const now = JSON.stringify(nicheList);
  if (now === settingsShown) return;
  settingsShown = now;
  const box = $("niches");
  // A redraw replaces every card, so keep what was typed and not saved, and
  // where the cursor was.
  const typed = {};
  for (const i of box.querySelectorAll("input")) if (i.id && i.id !== cleared && i.value !== i.defaultValue) typed[i.id] = i.value;
  const had = document.activeElement;
  const focus = box.contains(had) && had.id ? { id: had.id, from: had.selectionStart, to: had.selectionEnd } : null;
  box.replaceChildren();
  if (!nicheList.length) box.append(el("p", "No niches yet.", { class: "hint" }));
  for (const n of nicheList) {
    const card = el("section", null, { class: "niche", "aria-label": n.name });
    card.append(el("h2", n.name));
    // A text field saves when it loses focus, and only if it changed.
    const field = (label, key, value) => {
      const id = `niche-${n.id}-${key}`;
      const input = el("input", null, { id, value: value ?? "" });
      input.onchange = () => nicheOp("niche_set", { niche: n.id, [key]: input.value });
      card.append(el("label", label, { for: id }), input);
    };
    field("Name", "name", n.name);
    field("Workflow", "workflow", n.workflow);
    const vis = el("select", null, { id: `niche-${n.id}-visibility` });
    for (const v of VISIBILITY) vis.append(el("option", v, { value: v }));
    vis.value = n.visibility;
    vis.onchange = () => nicheOp("niche_set", { niche: n.id, visibility: vis.value });
    card.append(el("label", "Visibility", { for: vis.id }), vis);
    field("Channel", "channel", n.channel);
    const feeds = el("ul", null, { class: "feeds", "aria-label": `${n.name} feeds` });
    for (const url of n.feeds) {
      const li = el("li");
      const gone = el("button", "Remove", { type: "button", "aria-label": `Remove ${url}` });
      gone.onclick = () => nicheOp("feed_remove", { niche: n.id, url });
      li.append(el("span", url), gone);
      feeds.append(li);
    }
    card.append(el("label", "Feeds", { for: `niche-${n.id}-newfeed` }), feeds);
    const row = el("form", null, { class: "row" });
    const url = el("input", null, { id: `niche-${n.id}-newfeed`, type: "url", required: "", placeholder: "https://example.com/feed.xml", "aria-label": `New feed for ${n.name}` });
    row.append(url, el("button", "Add feed", { type: "submit" }));
    row.onsubmit = (e) => {
      e.preventDefault();
      nicheOp("feed_add", { niche: n.id, url: url.value }, url.id);
    };
    const rm = el("button", `Remove ${n.name}`, { type: "button" });
    rm.onclick = () => {
      pendingRemove = n;
      $("remove-text").textContent = `Remove ${n.name} and its feed list? Projects that name it keep the name.`;
      $("remove-dialog").returnValue = ""; // so Escape can't remove the last one asked about
      $("remove-dialog").showModal();
    };
    card.append(row, el("div", null, { class: "row" }));
    card.lastChild.append(rm);
    box.append(card);
  }
  for (const [id, value] of Object.entries(typed)) if ($(id)) $(id).value = value;
  if (focus && $(focus.id)) {
    $(focus.id).focus();
    if (focus.from != null) $(focus.id).setSelectionRange?.(focus.from, focus.to);
  }
}

$("remove-dialog").onclose = () => {
  const n = pendingRemove;
  pendingRemove = null;
  if ($("remove-dialog").returnValue === "remove" && n) nicheOp("niche_remove", { niche: n.id });
};

$("settings-button").onclick = () => setSettingsPage(!settingsOpen());
$("niche-form").onsubmit = async (e) => {
  e.preventDefault();
  await nicheOp("niche_add", { name: $("niche-name").value });
  if (!$("settings-error").textContent) $("niche-name").value = "";
};

// Sign in to YouTube. The reply says only whether a label is signed in;
// no operation returns a token.
async function refreshChannels() {
  try {
    const { channels } = await op("youtube_status");
    $("channels").replaceChildren(...channels.map((c) =>
      el("li", `${c.label}: ${c.signed_in ? "signed in" : "not signed in"}${c.title ? ` (${c.title})` : ""}`)));
  } catch (e) {
    $("youtube-error").textContent = String(e);
  }
}

$("signin-form").onsubmit = async (e) => {
  e.preventDefault();
  const button = e.target.querySelector("button");
  button.disabled = true;
  button.textContent = "Waiting up to 5 minutes for the browser…";
  $("youtube-error").textContent = "";
  try {
    await op("youtube_connect", { label: $("signin-label").value, client_file: $("signin-file").value });
  } catch (err) {
    $("youtube-error").textContent = String(err);
  }
  button.disabled = false;
  button.textContent = "Sign in";
  refreshChannels();
};
