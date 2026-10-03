"use strict";
// The laptop owner's settings page. Everything it shows comes from /host/state; nothing is built with innerHTML.
const $ = (id) => document.getElementById(id);
const keyed = () => [...document.querySelectorAll("[data-key]")];

let saved = null;     // the host.json the laptop holds
let state = null;     // the last /host/state
let timer = 0;

async function api(method, path, body) {
  const response = await fetch(path, {
    method, credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data = {};
  try { data = await response.json(); } catch { /* an empty reply */ }
  return { status: response.status, ok: response.ok, data };
}

function get(object, path) { return path.split(".").reduce((value, key) => (value == null ? value : value[key]), object); }
function put(object, path, value) {
  const keys = path.split(".");
  const last = keys.pop();
  keys.reduce((target, key) => target[key], object)[last] = value;
}

function show(view) {
  $("loginView").hidden = view !== "login";
  $("appView").hidden = view !== "app";
  $("savebar").hidden = view !== "app";
  $("logout").hidden = view !== "app";
}

function fill(config) {
  for (const el of keyed()) {
    const value = get(config, el.dataset.key);
    if (el.type === "checkbox") el.checked = value === true;
    else if (el.dataset.type === "bool-or-null") el.value = value === null || value === undefined ? "" : String(value);
    else if (el.dataset.type === "string-or-null") el.value = value ?? "";
    else el.value = value === null || value === undefined ? "" : String(value);
  }
}

function collect() {
  const config = JSON.parse(JSON.stringify(saved));
  for (const el of keyed()) {
    let value;
    if (el.type === "checkbox") value = el.checked;
    else if (el.dataset.type === "number") value = Number(el.value);
    else if (el.dataset.type === "bool-or-null") value = el.value === "" ? null : el.value === "true";
    else if (el.dataset.type === "string-or-null") value = el.value.trim() === "" ? null : el.value.trim();
    else if (el.dataset.type === "string") value = el.value.trim();
    else value = el.value;
    put(config, el.dataset.key, value);
  }
  config.tls_listen = $("tlsOn").checked ? $("tlsListen").value.trim() : "";
  return config;
}

// A value the console was started with shows as the current one when host.json says nothing.
function withEffective(config, effective) {
  const shown = JSON.parse(JSON.stringify(config));
  shown.http_listen ??= effective.http_listen;
  shown.tls_listen ??= effective.tls_listen ?? "";
  shown.tls_cert ??= effective.tls_cert ?? null;
  shown.tls_key ??= effective.tls_key ?? null;
  shown.public ??= effective.public === true;
  shown.allow_clipboard ??= effective.clipboard === true;
  shown.audio_sink ??= effective.audio_sink ?? null;
  return shown;
}

function renderSinks(sinks, current) {
  const select = $("sinkSelect");
  select.replaceChildren(new Option("The laptop's default output", ""));
  const names = new Set();
  for (const sink of sinks) { names.add(sink.name); select.append(new Option(sink.description, sink.name)); }
  if (current && !names.has(current)) select.append(new Option(`${current} (saved, not found now)`, current));
}

function renderForm(snapshot) {
  const config = withEffective(snapshot.config, snapshot.effective);
  saved = config;
  renderSinks(snapshot.sinks, config.audio_sink);
  fill(config);
  $("tlsOn").checked = config.tls_listen !== "";
  $("tlsListen").value = config.tls_listen !== "" ? config.tls_listen : "0.0.0.0:8443";
  $("tlsListen").disabled = !$("tlsOn").checked;
  $("autostart").checked = snapshot.unit.autostart;
  $("autostart").disabled = !snapshot.unit.installed;
  $("unitNote").textContent = snapshot.unit.installed
    ? (snapshot.unit.by_systemd ? "" : "This console was started by hand, not by the user service: Restart will only save.")
    : "The user service is not installed (install the .deb or copy systemd/user/blackroom-console.service).";
  $("hostline").textContent = `${snapshot.host}: what this laptop allows its remote clients`;
  updateDirty();
}

function renderStatus(snapshot) {
  const status = snapshot.status;
  const lines = { idle: "Ready: nobody is connected", starting: "A remote session is starting", running: `A ${status.mode} session is running`, stopping: "A session is ending" };
  $("statusLine").textContent = lines[status.phase] ?? status.phase;
  const pending = status.pending;
  $("askBox").hidden = !pending;
  if (pending) {
    $("askText").textContent = `A ${pending.mode} session from ${pending.device} is waiting for your approval (${pending.secs_left} s left).`;
    $("askBox").dataset.id = String(pending.id);
  }
  $("hint").textContent = snapshot.restart_needed ? "Saved settings are not in effect yet: restart the console." : "";
}

function dirty() { return saved !== null && JSON.stringify(collect()) !== JSON.stringify(saved); }

function updateDirty() {
  const changed = dirty();
  $("save").disabled = !changed;
  $("restart").disabled = !changed && !(state && state.restart_needed);
  $("tlsListen").disabled = !$("tlsOn").checked;
}

async function load(first) {
  const { status, data } = await api("GET", "/host/state");
  if (status === 401) { show("login"); return false; }
  if (status !== 200) { $("loginMsg").textContent = data.error ?? "The laptop did not answer."; show("login"); return false; }
  state = data;
  show("app");
  if (first || !dirty()) renderForm(data);
  renderStatus(data);
  return true;
}

function schedule() {
  clearInterval(timer);
  timer = setInterval(() => { load(false).catch(() => {}); }, 3000);
}

$("loginForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  $("loginBtn").disabled = true;
  $("loginMsg").textContent = "";
  const { ok, data } = await api("POST", "/host/login", { password: $("password").value });
  $("password").value = "";
  $("loginBtn").disabled = false;
  if (!ok) { $("loginMsg").textContent = data.error ?? "Sign-in failed."; return; }
  await load(true);
  schedule();
});

$("logout").addEventListener("click", async () => {
  await api("POST", "/host/logout", {});
  clearInterval(timer);
  saved = null;
  show("login");
});

document.addEventListener("input", updateDirty);
document.addEventListener("change", updateDirty);

async function save() {
  const { ok, data } = await api("POST", "/host/config", collect());
  $("saveNote").textContent = ok ? "Saved." : (data.error ?? "Could not save.");
  if (ok) await load(true);
  return ok;
}

$("save").addEventListener("click", save);

$("restart").addEventListener("click", async () => {
  if (state && state.status.phase !== "idle"
      && !window.confirm("A remote session is running. Restarting the console ends it. Continue?")) return;
  if (dirty() && !(await save())) return;
  const { ok, data } = await api("POST", "/host/restart", { confirm: true });
  if (!ok) { $("saveNote").textContent = data.error ?? "Could not restart."; return; }
  if (!data.restarting) { $("saveNote").textContent = data.note ?? "Saved."; return; }
  $("saveNote").textContent = "Restarting the console...";
  clearInterval(timer);
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 1500));
    try {
      const reply = await api("GET", "/host/state");
      if (reply.status === 401) { show("login"); $("loginMsg").textContent = "The console restarted. Sign in again."; return; }
    } catch { /* still down */ }
  }
  $("saveNote").textContent = "The console did not come back: start it from a terminal.";
});

$("autostart").addEventListener("change", async () => {
  const wanted = $("autostart").checked;
  const { ok, data } = await api("POST", "/host/autostart", { enabled: wanted });
  $("saveNote").textContent = ok ? (wanted ? "The console will start when you log in." : "The console will not start at login.") : (data.error ?? "Could not change it.");
  if (!ok) $("autostart").checked = !wanted;
});

for (const [id, accept] of [["askAccept", true], ["askDeny", false]]) {
  $(id).addEventListener("click", async () => {
    await api("POST", "/host/approve", { id: Number($("askBox").dataset.id), accept });
    await load(false);
  });
}

load(true).then((signedIn) => { if (signedIn) schedule(); }).catch(() => show("login"));
