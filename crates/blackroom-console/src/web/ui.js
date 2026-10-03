"use strict";
// The app shell around the remote-desktop engine in app.js: the connect screen, the in-session menu, the settings
// sheet (saved on this device, inside the limits the laptop owner sets) and toasts.

// ---- settings: this device's choices; the laptop's profile.json only supplies the defaults ----
const DEVICE_KEY = "br.settings.v1";
function deviceStoreWorks() {
  try { localStorage.setItem("br.probe", "1"); localStorage.removeItem("br.probe"); return true; } catch (_) { return false; }
}
const onDevice = deviceStoreWorks();

const PRESETS = {
  private: { blank_panel: true, block_local_input: true, lock_on_stop: true, cursor_in_video: false },
  shared: { blank_panel: false, block_local_input: false, lock_on_stop: false, cursor_in_video: true },
};
const PRESET_NOTES = {
  private: "The laptop's screen goes dark and its keyboard and touchpad stop until you disconnect.",
  shared: "The laptop keeps its screen and input. You and the person at the laptop both see and move the pointer.",
  custom: "A custom mix: change it under Settings > Connection.",
};
let settings = {
  version: 1,
  session: { blank_panel: true, block_local_input: true, lock_on_stop: true, resolution: null, cursor_in_video: false, audio: false,
    fps_cap: 0, bitrate_kbps: 0, heartbeat_secs: null, idle_minutes: 0, max_hours: 0 },
  client: { quality: "medium", scale: "fit", show_local_cursor: false, volume: 80, text_mode: "keys", mac_keys: false,
    touch_mode: "trackpad", fit_resolution: false },
};

function toast(text, kind) {
  const box = document.createElement("div");
  box.className = "toast" + (kind ? " " + kind : "");
  box.textContent = text;
  $("toasts").appendChild(box);
  setTimeout(() => box.remove(), kind === "bad" ? 8000 : 4000);
}

function dig(path) { return path.split(".").reduce((o, k) => (o ? o[k] : undefined), settings); }
function put(path, value) {
  const keys = path.split("."), last = keys.pop();
  keys.reduce((o, k) => o[k], settings)[last] = value;
}

function resolutionChoice() {
  if (settings.client.fit_resolution) return "fit";
  const size = settings.session.resolution;
  return size ? `${size.width}x${size.height}` : "native";
}

function currentPreset() {
  const s = settings.session;
  if (s.blank_panel && s.block_local_input) return "private";
  if (!s.blank_panel && !s.block_local_input) return "shared";
  return "custom";
}

let saveTimer = null;
function saveSoon(note) {
  clearTimeout(saveTimer);
  $("sheetnote").textContent = "Saving...";
  saveTimer = setTimeout(async () => {
    if (onDevice) {
      store.set(DEVICE_KEY, JSON.stringify(settings));
      $("sheetnote").textContent = note || "Your choices are saved on this device.";
      return;
    }
    try {
      const response = await post("/settings", settings);
      const body = await response.json();
      if (!response.ok) { toast("Settings not saved: " + (body.error || response.status), "bad"); $("sheetnote").textContent = "Not saved."; return; }
      $("sheetnote").textContent = note || "Saved on the laptop (this browser cannot keep them).";
    } catch (error) { toast("Settings not saved: " + error, "bad"); $("sheetnote").textContent = "Not saved."; }
  }, 350);
}

// ---- applying settings to the page ----
function applyClient() {
  const c = settings.client;
  stageScale = c.scale;
  $("stage").className = "scale-" + c.scale;
  $("scale").value = c.scale;
  document.body.classList.toggle("localcursor", c.show_local_cursor);
  rtcVideo.volume = c.volume / 100;
  $("volume").value = c.volume;
  if (c.touch_mode !== touchMode) setMode(c.touch_mode);
  if (document.activeElement !== $("quality")) $("quality").value = c.quality;
  placeDot();
}

function applyHome() {
  const preset = currentPreset();
  document.querySelectorAll(".mode").forEach((button) => button.setAttribute("aria-checked", String(button.dataset.preset === preset)));
  $("modenote").textContent = PRESET_NOTES[preset];
  $("homeAudio").checked = settings.session.audio;
  $("homeLock").checked = settings.session.lock_on_stop;
}

function fillSheet() {
  document.querySelectorAll("[data-key]").forEach((el) => {
    const key = el.dataset.key;
    const value = key === "ui.resolution" ? resolutionChoice() : dig(key);
    if (el.type === "checkbox") el.checked = !!value;
    else el.value = value === null || value === undefined ? "" : String(value);
  });
}

function applyAll() { applyClient(); applyHome(); fillSheet(); }

async function loadSettings() {
  try {
    const response = await fetch("/settings", { credentials: "same-origin" });
    if (response.ok) settings = await response.json();
  } catch (_) { toast("Could not load the saved settings; using defaults.", "bad"); }
  if (onDevice) {
    try {
      const mine = JSON.parse(store.get(DEVICE_KEY) || "null");
      if (mine && typeof mine === "object" && mine.session && mine.client) {
        settings = { version: 1, session: { ...settings.session, ...mine.session }, client: { ...settings.client, ...mine.client } };
      }
    } catch (_) { /* damaged: the laptop's defaults stay */ }
  }
  policyKey = "";
  if (lastState) applyPolicy(lastState.policy);
  applyAll();
}

function readControl(el) {
  if (el.type === "checkbox") return el.checked;
  const type = el.dataset.type;
  if (type === "number") return Number(el.value);
  if (type === "number-or-null") return el.value === "" ? null : Number(el.value);
  return el.value;
}

function setResolution(choice) {
  settings.client.fit_resolution = choice === "fit";
  if (choice === "native" || choice === "fit") settings.session.resolution = null;
  else { const [width, height] = choice.split("x").map(Number); settings.session.resolution = { width, height }; }
}

async function liveAudio(enabled) {
  const response = await post("/audio", { enabled });
  const state = await response.json();
  // The sound is part of the WebRTC offer: a new connection carries it.
  if (videoStarted) { videoStarted = false; stopVideo(); }
  show(state);
}

async function liveTuning() {
  if (!running) return;
  const response = await post("/tuning", { fps_cap: settings.session.fps_cap, bitrate_kbps: settings.session.bitrate_kbps });
  if (!response.ok) toast("Could not change the rate limits.", "bad");
}

document.querySelectorAll("[data-key]").forEach((el) => {
  const handler = async () => {
    const key = el.dataset.key;
    if (key === "ui.resolution") setResolution(el.value); else put(key, readControl(el));
    applyAll();
    const liveKey = key === "session.audio" || key === "session.fps_cap" || key === "session.bitrate_kbps";
    saveSoon(key.startsWith("session.") || key === "ui.resolution"
      ? (liveKey && running ? "Saved and sent to the running session." : "Saved. Applies the next time you connect.")
      : undefined);
    if (key === "client.quality") post("/quality", { level: settings.client.quality }).catch(() => {});
    if (key === "session.audio" && running) liveAudio(settings.session.audio).catch((e) => toast("Sound: " + e, "bad"));
    if (key === "session.fps_cap" || key === "session.bitrate_kbps") liveTuning();
  };
  el.addEventListener(el.type === "range" ? "input" : "change", handler);
});

// ---- connect screen ----
document.querySelectorAll(".mode").forEach((button) => button.addEventListener("click", () => {
  Object.assign(settings.session, PRESETS[button.dataset.preset]);
  // A preset must not undo what the owner forces (the lock choice, the allowed modes, sound): apply the policy again.
  if (policy) { policyKey = ""; applyPolicy(policy); } else applyAll();
  saveSoon();
}));
$("homeAudio").addEventListener("change", () => { settings.session.audio = $("homeAudio").checked; applyAll(); saveSoon(); });
$("homeLock").addEventListener("change", () => { settings.session.lock_on_stop = $("homeLock").checked; applyAll(); saveSoon(); });

function fitSize() {
  const ratio = window.devicePixelRatio || 1;
  let w = window.innerWidth * ratio, h = window.innerHeight * ratio;
  const k = Math.min(1, 2560 / w, 1440 / h);
  w = Math.max(640, w * k); h = Math.max(360, h * k);
  return { width: Math.round(w) & ~1, height: Math.round(h) & ~1 };
}

let connecting = false;
async function connect() {
  connecting = true;
  $("connect").disabled = true;
  setHostState(waitingText(), "warn");
  const options = { ...settings.session };
  if (settings.client.fit_resolution && options.blank_panel) options.resolution = fitSize();
  try {
    await post("/quality", { level: settings.client.quality });
    const response = await post("/start", options);
    const body = await response.json();
    if (!response.ok) { toast("Could not connect: " + (body.error || response.status), "bad"); refresh(); }
    else show(body);
  } catch (error) { toast("Could not connect: " + error, "bad"); }
  connecting = false;
  $("connect").disabled = false;
}

const waitingText = () => (policy && policy.approval === "ask" ? "Waiting for the laptop owner to accept..." : "Connecting...");
$("connect").addEventListener("click", connect);

function setHostState(text, kind) {
  const pill = $("hoststate");
  pill.textContent = text;
  pill.className = "pill" + (kind ? " " + kind : "");
}

function reasonText(stop) {
  const reason = stop.reason.charAt(0).toUpperCase() + stop.reason.slice(1);
  let text = `Last session ended: ${reason}.`;
  if (stop.topology_restored === true) text += " The screen was restored.";
  if (stop.locked === true) text += " The laptop was locked.";
  // An explicit false is a failed step (null: not asked for); never read it as success.
  const problems = [];
  if (stop.topology_restored === false) problems.push("the screen could not be confirmed restored");
  if (stop.locked === false) problems.push("the laptop could not be locked");
  if (stop.grab_released === false) problems.push("release of the laptop's keyboard and touchpad was not confirmed");
  if (problems.length) text += " WARNING: " + problems.join("; ") + ". Check the laptop; if its screen is blank or its keyboard is blocked, see the runbook.";
  if (stop.errors && stop.errors.length) text += " Problems: " + stop.errors.join("; ") + ".";
  return { text, warn: problems.length > 0 || !!(stop.errors && stop.errors.length) };
}

function fmtTime(total) {
  const t = Math.max(0, Math.floor(total)), h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
  const two = (n) => String(n).padStart(2, "0");
  return h ? `${h}:${two(m)}:${two(s)}` : `${two(m)}:${two(s)}`;
}

let sessionClock = { secs: 0, at: 0 };
setInterval(() => { if (running) $("timer").textContent = fmtTime(sessionClock.secs + (performance.now() - sessionClock.at) / 1000); }, 1000);

// Called by app.js after every status poll.
function onState(state) {
  applyPolicy(state.policy);
  $("hostname").textContent = state.host || "This laptop";
  $("hostline").textContent = state.host ? `Remote control for ${state.host}` : "Remote control for this laptop";
  $("version").textContent = "Blackroom Console " + (state.version || "");
  const view = state.phase === "running" ? "session" : "home";
  if (document.body.dataset.view !== view) {
    document.body.dataset.view = view;
    if (view === "home") { closeMenu(); placeDot(); }
    else { $("diag").style.display = "none"; }
  }
  if (view === "session") {
    sessionClock = { secs: state.session_secs || 0, at: performance.now() };
    $("timer").textContent = fmtTime(state.session_secs || 0);
    const audio = state.audio || "off";
    const on = audio === "on";
    $("soundBtn").textContent = on ? "Sound on" : audio === "off" ? "Sound off" : audio === "waiting" ? "Sound..." : "No sound";
    $("soundBtn").setAttribute("aria-pressed", String(on));
    $("soundnote").textContent = state.audio_note || (audioBlocked ? "Tap Sound to allow it in this browser." : "");
    $("soundrow").hidden = false;
    if (state.mode) document.body.dataset.mode = state.mode;
    return;
  }
  const busy = state.phase === "starting" || state.phase === "stopping";
  $("connect").disabled = busy || connecting;
  if (linkLost) setHostState("No connection", "warn");
  else if (connecting) setHostState(waitingText(), "warn");
  else if (busy) setHostState(state.phase === "starting" ? "Connecting..." : "Disconnecting...", "warn");
  else setHostState("Ready", "live");
  const stop = state.last_stop;
  $("hostnote").hidden = !stop;
  if (stop) {
    const { text, warn } = reasonText(stop);
    $("hostnote").textContent = text;
    $("hostnote").classList.toggle("warn", warn);
  }
}

// ---- in-session menu ----
function openMenu() {
  releaseAll();
  $("menu").hidden = false;
  $("menuBtn").setAttribute("aria-expanded", "true");
  $("menuClose").focus();
}
function closeMenu() {
  $("menu").hidden = true;
  $("menuBtn").setAttribute("aria-expanded", "false");
  if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
}
$("menuBtn").addEventListener("click", () => ($("menu").hidden ? openMenu() : closeMenu()));
$("menuClose").addEventListener("click", closeMenu);
$("stop").addEventListener("click", closeMenu);

$("scale").addEventListener("change", () => { settings.client.scale = $("scale").value; applyAll(); saveSoon(); });
$("quality").addEventListener("change", () => { settings.client.quality = $("quality").value; fillSheet(); saveSoon(); });
$("mode").addEventListener("click", () => { settings.client.touch_mode = touchMode; fillSheet(); saveSoon(); });
$("volume").addEventListener("input", () => { settings.client.volume = Number($("volume").value); rtcVideo.volume = settings.client.volume / 100; fillSheet(); saveSoon(); });
$("soundBtn").addEventListener("click", async () => {
  if (audioBlocked) { rtcVideo.muted = false; audioBlocked = false; $("soundnote").textContent = ""; return; }
  const enable = !(lastState && lastState.audio === "on");
  settings.session.audio = enable; applyAll(); saveSoon();
  try { await liveAudio(enable); } catch (error) { toast("Sound: " + error, "bad"); }
});
$("menuSettings").addEventListener("click", () => openSheet());

// ---- settings sheet ----
let sheetReturn = null;
function openSheet() {
  fillSheet();
  sheetReturn = document.activeElement;
  $("sheet").hidden = false;
  $("sheetClose").focus();
}
function closeSheet() {
  $("sheet").hidden = true;
  if (sheetReturn && sheetReturn.focus) sheetReturn.focus();
}
$("openSettings").addEventListener("click", openSheet);
$("sheetClose").addEventListener("click", closeSheet);
$("sheet").addEventListener("click", (event) => { if (event.target === $("sheet")) closeSheet(); });
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !$("sheet").hidden) { event.stopPropagation(); event.preventDefault(); closeSheet(); }
}, true);
document.querySelectorAll(".tabs [data-tab]").forEach((tab) => tab.addEventListener("click", () => {
  document.querySelectorAll(".tabs [data-tab]").forEach((t) => t.setAttribute("aria-selected", String(t === tab)));
  document.querySelectorAll(".pane").forEach((pane) => { pane.hidden = pane.dataset.pane !== tab.dataset.tab; });
}));
$("settingsReset").addEventListener("click", async () => {
  if (!window.confirm("Forget this device's choices and go back to the laptop's defaults?")) return;
  try {
    if (onDevice) {
      try { localStorage.removeItem(DEVICE_KEY); } catch (_) { /* nothing kept */ }
      await loadSettings();
    } else {
      // This browser keeps nothing, so its choices live on the laptop: that is what gets reset.
      const response = await post("/settings/reset");
      if (!response.ok) throw new Error("the laptop answered " + response.status);
      settings = await response.json();
      policyKey = "";
      if (lastState) applyPolicy(lastState.policy);
      applyAll();
    }
    toast("Settings reset.");
  } catch (error) { toast("Could not reset: " + error, "bad"); }
});

// ---- the rest ----
const logout = $("logout");
if (logout) logout.addEventListener("click", () => { fetch("/logout", { method: "POST", credentials: "same-origin" }).then(() => location.reload()); });

applyAll();
loadSettings();

// ---- installable web app ----
const standalone = () => window.matchMedia("(display-mode: standalone)").matches || navigator.standalone === true;
let installPrompt = null;
function showInstall(text) {
  if (standalone()) return;
  $("installCard").hidden = false;
  if (text) { $("installCard").querySelector(".note").textContent = text; $("installBtn").hidden = true; }
}
window.addEventListener("beforeinstallprompt", (event) => { event.preventDefault(); installPrompt = event; showInstall(); });
window.addEventListener("appinstalled", () => { installPrompt = null; $("installCard").hidden = true; toast("Installed. Open Blackroom Console from your apps."); });
$("installBtn").addEventListener("click", async () => {
  if (!installPrompt) return;
  installPrompt.prompt();
  await installPrompt.userChoice.catch(() => {});
  installPrompt = null; $("installCard").hidden = true;
});
if (capabilities().ios) showInstall("To install on this device: tap Share, then Add to Home Screen.");
if ("serviceWorker" in navigator && window.isSecureContext) navigator.serviceWorker.register("/sw.js").catch(() => { /* untrusted certificate: no install, the page still works */ });

// ---- what the laptop owner allows: the page offers only that (the laptop refuses the rest anyway) ----
let policy = null, policyKey = "";
function enable(el, on) { if (el) el.disabled = !on; }
function bySel(key) { return document.querySelector(`[data-key="${key}"]`); }

// Disables the options a limit forbids and moves the current value to the largest allowed one.
function limitOptions(key, max, zeroAllowed) {
  const el = bySel(key);
  if (!el || !max) { if (el) for (const o of el.options) o.disabled = false; return false; }
  let allowed = null, changed = false;
  for (const o of el.options) {
    const v = o.value === "" ? 0 : Number(o.value);
    o.disabled = (v === 0 && !zeroAllowed) || v > max;
    if (!o.disabled && (allowed === null || v > Number(allowed.value || 0))) allowed = o;
  }
  const current = dig(key);
  if (allowed && ((current === 0 && !zeroAllowed) || current > max)) { put(key, Number(allowed.value)); changed = true; }
  return changed;
}

function applyPolicy(p) {
  if (!p) return;
  const text = JSON.stringify(p);
  if (text === policyKey) return;
  policyKey = text;
  policy = p;
  const s = settings.session, c = settings.client;
  const notes = [];
  document.querySelectorAll(".mode").forEach((button) => {
    const ok = button.dataset.preset === "private" ? p.allow_private : p.allow_shared;
    button.disabled = !ok;
    button.title = ok ? "" : "The laptop owner has switched this off";
  });
  const only = p.allow_private && !p.allow_shared ? "private" : !p.allow_private && p.allow_shared ? "shared" : null;
  if (only) {
    s.blank_panel = s.block_local_input = only === "private";
    notes.push(only === "private" ? "Only Private sessions are allowed." : "Only Shared sessions are allowed.");
  }
  for (const key of ["session.blank_panel", "session.block_local_input"]) enable(bySel(key), !only);
  const forced = p.force_lock_on_stop;
  if (forced !== null && forced !== undefined) {
    s.lock_on_stop = forced;
    notes.push(forced ? "The laptop always locks when you disconnect." : "The laptop does not lock when you disconnect.");
  }
  enable($("homeLock"), forced === null || forced === undefined);
  enable(bySel("session.lock_on_stop"), forced === null || forced === undefined);
  if (!p.allow_audio) { s.audio = false; notes.push("Laptop sound is switched off."); }
  for (const el of [$("homeAudio"), bySel("session.audio"), $("soundBtn"), $("volume")]) enable(el, p.allow_audio);
  const textMode = bySel("client.text_mode");
  if (textMode) for (const o of textMode.options) if (o.value === "text") o.disabled = !p.allow_text;
  if (!p.allow_text) { if (c.text_mode === "text") c.text_mode = "keys"; notes.push("Typing text is switched off."); }
  limitOptions("session.fps_cap", p.max_fps, true);
  if (p.max_fps) notes.push(`Frame rate is limited to ${p.max_fps}.`);
  limitOptions("session.bitrate_kbps", p.max_bitrate_kbps, true);
  if (p.max_bitrate_kbps) notes.push(`Bitrate is limited to ${(p.max_bitrate_kbps / 1000).toFixed(1).replace(/\.0$/, "")} Mbit/s.`);
  limitOptions("session.idle_minutes", p.max_idle_minutes, false);
  if (p.max_idle_minutes) notes.push(`A session ends after ${p.max_idle_minutes} idle minutes at most.`);
  limitOptions("session.max_hours", p.max_session_hours, false);
  if (p.max_session_hours) notes.push(`A session lasts ${p.max_session_hours} hour${p.max_session_hours === 1 ? "" : "s"} at most.`);
  if (p.approval === "ask") notes.push("The laptop owner has to accept each connection.");
  const note = notes.filter(Boolean).join(" ");
  $("policynote").hidden = note === "";
  $("policynote").textContent = note === "" ? "" : "Set by the laptop owner: " + note;
  applyAll();
}
