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

function renderLogin(snapshot) {
  const names = { hostd: "Linux password + authenticator code + key", totp: "authenticator code", token: "the one-time address" };
  const parts = [`Right now clients sign in with ${names[snapshot.effective.login_method] ?? snapshot.effective.login_method}.`,
    snapshot.login.ready ? "The login authority is running." : "The login authority is not running (not set up yet, or stopped)."];
  if (snapshot.effective.login_note) parts.push(snapshot.effective.login_note + ".");
  $("loginNote").textContent = parts.join(" ");
}

function renderTotp(snapshot) {
  $("totpStatus").textContent = snapshot.totp.enrolled
    ? "An authenticator is set up. Replace it if you lost the phone or want another app."
    : "No authenticator is set up yet. Add one before choosing the password + authenticator sign-in.";
  $("totpStart").textContent = snapshot.totp.enrolled ? "Replace the authenticator" : "Add an authenticator";
}

function renderLock(snapshot) {
  const lock = snapshot.lockscreen;
  if (document.activeElement === $("lockOn")) return;
  $("lockOn").checked = lock.enabled && lock.active;
  $("lockOn").disabled = !lock.installed;
  $("lockNote").textContent = !lock.installed
    ? "The lock-screen extension is not installed (the .deb installs it; log out and in once so GNOME finds it)."
    : lock.enabled && !lock.active ? "It is switched on but GNOME has not loaded it yet: log out and in once."
    : lock.enabled ? "On: a remote session can be opened on the lock screen." : "Off: locking the laptop ends remote sessions.";
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
  renderLogin(data);
  renderTotp(data);
  renderLock(data);
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
  loadCredentials();
});

$("logout").addEventListener("click", async () => {
  await api("POST", "/host/totp/cancel", {}).catch(() => {});
  await api("POST", "/host/logout", {});
  hideSecret();
  closeTotp();
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

$("lockOn").addEventListener("change", async () => {
  const wanted = $("lockOn").checked;
  if (wanted && !window.confirm("Allow remote sessions on the lock screen? While this is on, locking the laptop no longer ends a remote session.")) { $("lockOn").checked = false; return; }
  const reply = await api("POST", "/host/lockscreen", { enabled: wanted, password: $("lockPassword").value });
  $("lockPassword").value = "";
  if (!reply.ok) { $("lockOn").checked = !wanted; $("saveNote").textContent = reply.data.error ?? "That did not work."; return; }
  $("saveNote").textContent = wanted ? "Remote use on the lock screen is on." : "Remote use on the lock screen is off.";
  await load(false);
});

// ---- authenticator app: scan or type the key, then one right code stores it ----
function closeTotp() {
  $("totpBox").hidden = true;
  $("totpQr").removeAttribute("src");
  $("totpSecret").textContent = "";
  $("totpCode").value = "";
  $("totpMsg").textContent = "";
}

$("totpStart").addEventListener("click", async () => {
  const reply = await api("POST", "/host/totp/start", { password: $("totpPassword").value });
  $("totpPassword").value = "";
  if (!reply.ok) { $("saveNote").textContent = reply.data.error ?? "That did not work."; return; }
  $("totpQr").src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(reply.data.svg);
  $("totpSecret").textContent = reply.data.secret;
  $("totpAccount").textContent = "Blackroom Console:" + reply.data.account;
  $("totpMsg").textContent = "Waiting for a code from the app (this setup expires in " + Math.round(reply.data.expires_secs / 60) + " minutes).";
  $("totpBox").hidden = false;
  $("totpCode").focus();
});

$("totpVerify").addEventListener("click", async () => {
  const reply = await api("POST", "/host/totp/verify", { code: $("totpCode").value.trim() });
  if (reply.ok) {
    closeTotp();
    $("saveNote").textContent = "Authenticator confirmed and saved. Clients now need a code from this app" +
      (reply.data.authority_restarted ? " (the login authority was restarted; remote logins ended)." : ".");
    await load(false);
    return;
  }
  $("totpMsg").textContent = reply.data.error ?? "That did not work.";
  $("totpCode").value = "";
  if (reply.status === 409 || reply.status === 429) { $("totpQr").removeAttribute("src"); $("totpSecret").textContent = ""; }
});
$("totpCode").addEventListener("keydown", (event) => { if (event.key === "Enter") $("totpVerify").click(); });
$("totpCancel").addEventListener("click", async () => { await api("POST", "/host/totp/cancel", {}); closeTotp(); });

// ---- credentials: the laptop's own `blackroom` command does the work; secrets are shown once and not kept ----
let hideTimer = 0;
function hideSecret() {
  clearTimeout(hideTimer);
  $("credOut").textContent = "";
  $("secretBox").hidden = true;
}

async function credential(action, extra = {}, needsPassword = true) {
  const body = { action, ...extra };
  if (needsPassword) {
    body.password = $("credPassword").value;
    if (body.password === "") { $("saveNote").textContent = "Type your laptop password first."; return null; }
  }
  const reply = await api("POST", "/host/credentials", body);
  if (needsPassword) $("credPassword").value = "";
  if (!reply.ok) { $("saveNote").textContent = reply.data.error ?? "That did not work."; return null; }
  return reply.data;
}

async function loadCredentials() {
  const [status, devices] = await Promise.all([credential("status", {}, false), credential("devices", {}, false)]);
  const text = [status, devices].filter(Boolean).map((part) => (part.output || part.notice || "").trim()).filter(Boolean).join("\n\n");
  $("credStatus").textContent = text || "Nothing to show: the login authority is not set up (run blackroom setup).";
}

async function change(action, extra, question) {
  if (question && !window.confirm(question)) return;
  const data = await credential(action, extra);
  if (!data) return;
  const out = [data.output.trim(), data.notice].filter(Boolean).join("\n\n");
  $("saveNote").textContent = data.ok ? "Done." : "The command reported a problem.";
  if (out) {
    $("credOut").textContent = out;
    $("secretBox").hidden = false;
    clearTimeout(hideTimer);
    hideTimer = setTimeout(hideSecret, 120000);
  }
  await loadCredentials();
}

$("loginSetup").addEventListener("click", () => change("setup", {}, "Set up the login authority? It creates an authenticator, a Remote Access Key and recovery codes (shown once) if you do not have them yet, and starts remote-hostd."));
$("credKey").addEventListener("click", () => change("rotate_key", { revoke_devices: $("credForget").checked }, "Make a new Remote Access Key? The old one stops working."));
$("credCodes").addEventListener("click", () => change("recovery_codes", {}, "Make new recovery codes? The old ones stop working."));
$("credReset").addEventListener("click", () => change("reset_security", {}, "New authenticator, key and recovery codes? Every old secret, trusted browser and remote login stops working."));
$("credSignOut").addEventListener("click", () => change("revoke_all", {}, "Sign out every remote login, including a session that is running?"));
$("credOff").addEventListener("click", () => change("disable", {}, "Switch remote access off? Every remote login ends and nobody can sign in until you switch it on."));
$("credOn").addEventListener("click", () => change("enable", {}, null));
$("credRevoke").addEventListener("click", () => change("revoke_device", { device: $("credDevice").value.trim() }, "Forget this trusted browser?"));
$("credHide").addEventListener("click", hideSecret);

load(true).then((signedIn) => { if (signedIn) { schedule(); loadCredentials(); } }).catch(() => show("login"));
