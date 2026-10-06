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
  // Leaving the signed-in view (sign out, expired session) must not leave secrets or typed passwords behind.
  if (view !== "app") {
    hideSecret();
    closeTotp();
    $("editPassword").value = "";
    setUnlock(0);
    $("credStatus").textContent = "";
  }
}

// A saved value the dropdown has no entry for (a limit set by hand) must show as itself, never as an empty choice.
function ensureOption(el, value) {
  if (el.tagName !== "SELECT" || value === null || value === undefined) return;
  const text = String(value);
  if (![...el.options].some((option) => option.value === text)) el.append(new Option(`${text} (current setting)`, text));
}

function fill(config) {
  for (const el of keyed()) {
    const value = get(config, el.dataset.key);
    if (el.type === "checkbox") el.checked = value === true;
    else if (el.dataset.type === "bool-or-null") el.value = value === null || value === undefined ? "" : String(value);
    else if (el.dataset.type === "string-or-null") el.value = value ?? "";
    else {
      if (el.dataset.type === "number" || el.dataset.type === "number-or-null") ensureOption(el, value);
      el.value = value === null || value === undefined ? "" : String(value);
    }
  }
}

function collect() {
  const config = JSON.parse(JSON.stringify(saved));
  for (const el of keyed()) {
    let value;
    if (el.type === "checkbox") value = el.checked;
    // An empty number is never "no limit" (0): keep what is saved.
    else if (el.dataset.type === "number") value = el.value === "" ? get(saved, el.dataset.key) : Number(el.value);
    else if (el.dataset.type === "bool-or-null") value = el.value === "" ? null : el.value === "true";
    else if (el.dataset.type === "string-or-null") value = el.value.trim() === "" ? null : el.value.trim();
    else if (el.dataset.type === "string") value = el.value.trim();
    else value = el.value;
    put(config, el.dataset.key, value);
  }
  config.tls_listen = $("tlsOn").checked ? $("tlsListen").value.trim() : "";
  if (accessTouched) collectAccess(config);
  return config;
}

// ---- access from outside: home only, a private VPN, or direct; the settings of each mode are remembered ----
let accessTouched = false;   // untouched: collect() hands back the saved access settings byte for byte

function modeOf(config) { return config.public === true ? "direct" : config.tls_cert ? "vpn" : "home"; }

function fillAccess(config) {
  accessTouched = false;
  const mode = modeOf(config);
  const memory = config.saved_access ?? {};
  const live = { public_name: config.public_name ?? null, public_cert: config.public_cert ?? null, tls_cert: config.tls_cert ?? null, tls_key: config.tls_key ?? null, ice_ports: config.ice_ports ?? null };
  const vpn = mode === "vpn" ? live : (memory.vpn ?? {});
  const direct = mode === "direct" ? live : (memory.direct ?? {});
  $("vpnName").value = vpn.public_name ?? "";
  $("vpnCert").value = vpn.tls_cert ?? "";
  $("vpnKey").value = vpn.tls_key ?? "";
  $("directName").value = direct.public_name ?? "";
  $("directCert").value = direct.public_cert ?? "ca";
  $("directCertFile").value = direct.tls_cert ?? "";
  $("directKeyFile").value = direct.tls_key ?? "";
  $("directPorts").value = direct.ice_ports ?? "";
  $("accessMode").value = mode;
  showAccess();
}

// Switching to or from Direct changes who can find the login page, so it needs "Enable editing".
function directChanges() { return !!saved && ($("accessMode").value === "direct") !== (saved.public === true); }

const MODE_LABEL = { home: "home network only", vpn: "a private VPN", direct: "direct from the internet" };
const RUNNING_MODE = { direct: "direct", "private-network": "vpn" };

// The settings in the form (pending until saved and restarted) against what the running console reports.
function renderPending() {
  const running = RUNNING_MODE[state?.effective?.internet?.access] ?? "home";
  const chosen = $("accessMode").value;
  const waiting = !!state?.restart_needed;
  $("pendingAccess").hidden = !(chosen !== running || waiting);
  $("pendingAccess").textContent = `Selected: ${MODE_LABEL[chosen]}. Running: ${MODE_LABEL[running]}. Save and restart to apply; restarting ends any active session.`;
  $("vpnCommands").textContent = vpnCommands($("vpnName").value.trim());
  $("fpSteps").hidden = !($("accessMode").value === "direct" && $("directCert").value === "self_signed");
}

function vpnCommands(name) {
  const dir = "~/.config/blackroom/tls";
  const full = name || "laptop.tailnet-name.ts.net";
  return [`mkdir -p ${dir} && chmod 700 ${dir}`,
    `sudo tailscale cert --cert-file ${dir}/cert.pem --key-file ${dir}/key.pem ${full}`,
    `sudo chown $USER: ${dir}/*.pem && chmod 600 ${dir}/*.pem`].join("\n");
}

function showAccess() {
  const mode = $("accessMode").value;
  renderPending();
  $("accessVpn").hidden = mode !== "vpn";
  $("accessDirect").hidden = mode !== "direct";
  $("directFiles").hidden = $("directCert").value === "self_signed";
  $("accessHelp").textContent = {
    home: "Meant for your own network only. This is what is configured; \"Listening now\" below shows which network interfaces the running console actually answers on.",
    vpn: "Your phone and this laptop join the same private network (Tailscale, NetBird, Headscale). No port is forwarded to the internet by this setting; the VPN's relay is used when no direct path exists. The console still answers on the interfaces under \"Listening now\".",
    direct: "The router forwards ports to this laptop, so anyone who finds the address can reach the login page. Needs a static IP address or a name that follows your address, and a router that accepts incoming connections. Choosing it also moves plain http to this laptop only, turns https on and makes the login authority the only way to sign in; the router forwards to set up are listed under \"Currently running\" after you save and restart.",
  }[mode];
}

function textOrNull(id) { const value = $(id).value.trim(); return value === "" ? null : value; }

function collectAccess(config) {
  const mode = $("accessMode").value;
  const vpn = { public_name: textOrNull("vpnName"), tls_cert: textOrNull("vpnCert"), tls_key: textOrNull("vpnKey") };
  const direct = { public_name: textOrNull("directName"), public_cert: $("directCert").value, tls_cert: textOrNull("directCertFile"), tls_key: textOrNull("directKeyFile"), ice_ports: textOrNull("directPorts") };
  const blank = (profile) => Object.values(profile).every((value) => value === null || value === "ca");
  const memory = { vpn: blank(vpn) ? null : vpn, direct: blank(direct) ? null : direct };
  config.saved_access = memory.vpn || memory.direct ? memory : null;
  config.public = mode === "direct";
  config.public_name = mode === "vpn" ? vpn.public_name : mode === "direct" ? direct.public_name : null;
  config.public_cert = mode === "direct" ? direct.public_cert : null;
  const files = mode === "vpn" ? vpn : mode === "direct" && direct.public_cert === "ca" ? direct : { tls_cert: null, tls_key: null };
  config.tls_cert = files.tls_cert;
  config.tls_key = files.tls_key;
  if (mode === "direct") {
    config.ice_ports = direct.ice_ports;
    // What the console insists on for internet use is set together with the mode, so no unit file needs editing.
    const [host, port] = (config.http_listen || "127.0.0.1:8080").replace(/^\[|\]$/g, "").split(/:(?=\d+$)/);
    if (!["127.0.0.1", "::1", "localhost"].includes(host)) config.http_listen = `127.0.0.1:${port || "8080"}`;
    if (config.tls_listen === "") config.tls_listen = "0.0.0.0:8443";
    config.login = "hostd";
  }
}

$("internetCard").addEventListener("input", () => { accessTouched = true; showAccess(); });
$("internetCard").addEventListener("change", () => { accessTouched = true; showAccess(); });

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
  fillAccess(config);
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
    : lock.enabled ? "On: a remote session can be opened on the lock screen."
    : lock.pending_off ? "Pending off: a remote session is running. If the screen is locked now, it keeps running until it ends; locking later ends it."
    : "Off: locking the laptop ends remote sessions. A session already open on a locked screen keeps running until it ends.";
}

// Where the running console actually answers, in words: a bind address is not the same as "private".
function describeListen(address) {
  if (!address) return "off";
  const host = address.replace(/:\d+$/, "").replace(/^\[|\]$/g, "");
  const reach = ["127.0.0.1", "::1", "localhost"].includes(host) ? "this laptop only" : ["0.0.0.0", "::"].includes(host) ? "every network interface: the whole local network and any VPN" : `the interface ${host}`;
  return `${address} (${reach})`;
}

// What a new owner still has to do, in order; the card disappears when everything is done.
function renderFirstSteps(snapshot) {
  const input = snapshot.input || { nodes: 0, allowed: 0 };
  const steps = [
    [snapshot.login.ready && snapshot.totp.enrolled, "Set up sign-in", "Under \"Login and credentials\" press \"Set up remote sign-in\": it creates your authenticator, a Remote Access Key and recovery codes, shown once. Keep them in a password manager."],
    [input.nodes === 0 || input.allowed === input.nodes, "Allow keyboard blocking", "Under \"Keyboard blocking\" press the button and answer the password dialog on this laptop. Only needed for Private sessions, and again after each restart."],
    [false, "Connect from your tablet or phone", `Open https://${snapshot.host || "this-laptop"}:${(snapshot.effective.tls_listen || "").split(":").pop() || "8443"}/ (or this laptop's address on your network). Your browser warns about the certificate the first time: compare the fingerprint under \"Access from outside\" before you continue.`],
  ];
  const list = $("firstSteps");
  list.replaceChildren(...steps.map(([done, title, text]) => {
    const item = document.createElement("li");
    const strong = document.createElement("b");
    strong.textContent = (done ? "Done: " : "") + title + ". ";
    item.append(strong, document.createTextNode(done ? "" : text));
    return item;
  }));
  const finished = steps[0][0] && steps[1][0];
  $("firstCard").hidden = finished;
  $("setupSection").hidden = finished;
  $("navSetup").hidden = finished;
}

// "Come back after a restart" is on only when every part is: the keyboard rule, GDM login, the lock marker, the lock-screen extension.
let restartBusy = false;
function renderRestart(snapshot) {
  if (restartBusy) return;
  const r = snapshot.restart_access;
  const ready = r.input_rule && r.autologin === "ours" && r.lock_at_login && snapshot.lockscreen.enabled;
  const some = r.input_rule || r.autologin === "ours" || r.lock_at_login;
  $("restartOn").checked = ready;
  $("restartOn").disabled = !r.gdm || r.autologin === "other";
  $("restartNote").textContent = !r.gdm ? "This desktop does not use GDM, so automatic login cannot be set up from here."
    : r.autologin === "other" ? "Automatic login is already set in the GDM settings by someone else: it was left alone. Remove it there first."
    : ready ? "On: after a restart the laptop logs in, locks, and waits for you."
    : some ? "Partly set up. Switch it on again to finish, or off to undo it." : "";
}

function renderInput(snapshot) {
  const input = snapshot.input || { nodes: 0, allowed: 0 };
  $("inputLine").textContent = input.nodes === 0 ? "No built-in keyboard or touchpad was found, so there is nothing to block."
    : input.allowed === input.nodes ? `Allowed: the console can block this laptop's keyboard and touchpad (${input.allowed} of ${input.nodes} devices).`
    : `Not allowed yet (${input.allowed} of ${input.nodes} devices): a Private session that blocks the keyboard cannot start.`;
  $("inputAllow").hidden = input.nodes === 0 || input.allowed === input.nodes;
}

function renderInternet(snapshot) {
  $("listenLine").textContent = `Listening now: plain http on ${describeListen(snapshot.effective.http_listen)}; https on ${describeListen(snapshot.effective.tls_listen)}.`;
  const report = snapshot.effective.internet;
  const list = $("internetList");
  list.replaceChildren();
  if (!report) { $("internetLine").textContent = "Not reported."; return; }
  const access = { direct: "Reachable from the internet (internet mode is on).", "private-network": "Meant for a private network or VPN (a certificate is set, internet mode is off)." };
  $("internetLine").textContent = access[report.access] ?? "Home network only.";
  const lines = [];
  if (report.url) lines.push("Address for the phone: " + report.url);
  if (report.certificate) {
    lines.push(`Certificate covers ${report.certificate.names.join(", ") || "no names"}; ${report.days_left} day(s) left; ${report.self_signed ? "self-signed" : "issued by an authority"}.`);
    lines.push("Certificate fingerprint (SHA-256): " + report.certificate.fingerprint);
  }
  for (const line of report.forwards) lines.push("Forward on the router: " + line);
  for (const line of report.problems) lines.push("Problem: " + line);
  for (const line of report.warnings) lines.push("Warning: " + line);
  for (const text of lines) { const item = document.createElement("li"); item.textContent = text; list.append(item); }
  $("certBanner").hidden = !report.renew_note;
  $("certBanner").textContent = report.renew_note ? report.renew_note.charAt(0).toUpperCase() + report.renew_note.slice(1) + "." : "";
}

const DATE = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

// The list prints seconds since 1970; people read dates.
function lastUsed(field) {
  const seconds = Number((field || "").replace("last_used=", ""));
  const meta = document.createElement("span");
  meta.className = "meta";
  if (!Number.isFinite(seconds) || seconds < 0) { meta.textContent = "Last used: unknown"; return meta; }
  if (seconds === 0) { meta.textContent = "Never used"; return meta; }
  const when = new Date(seconds * 1000);
  const time = document.createElement("time");
  time.dateTime = when.toISOString();
  time.textContent = DATE.format(when);
  meta.append("Last used ", time);
  return meta;
}

function renderDevices(text) {
  const list = $("deviceList");
  list.replaceChildren();
  for (const line of text.split("\n")) {
    const [id, label, , used, status] = line.split("\t");
    if (!id || status !== "trusted") continue;   // a forgotten browser is not a trusted one: do not list it
    const item = document.createElement("li");
    const info = document.createElement("span");
    info.className = "device-info";
    const name = document.createElement("strong");
    name.textContent = label || "Unnamed browser";
    info.append(name, lastUsed(used));
    const forget = document.createElement("button");
    forget.type = "button";
    forget.textContent = "Forget";
    forget.setAttribute("aria-label", `Forget ${label || "this browser"}`);
    forget.addEventListener("click", () => change("revoke_device", { device: id }, `Forget ${label || "this browser"}?`));
    item.append(info, forget);
    list.append(item);
  }
  if (!list.children.length) { const none = document.createElement("li"); none.textContent = "No trusted browsers."; list.append(none); }
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
  renderPending();
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
  setUnlock(data.unlock ? data.unlock.secs_left : 0);
  $("configNote").hidden = !data.note;
  $("configNote").textContent = data.note ? "Warning: " + data.note + ". Saving this page writes a valid file again." : "";
  if (first || !dirty()) renderForm(data);
  renderStatus(data);
  renderLogin(data);
  renderTotp(data);
  renderLock(data);
  renderInput(data);
  renderRestart(data);
  renderFirstSteps(data);
  renderInternet(data);
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
  let reply;
  try {
    reply = await api("POST", "/host/login", { password: $("password").value });
  } catch (_) {
    reply = { ok: false, data: { error: "The laptop did not answer. Try again." } };
  } finally {
    $("password").value = "";
    $("loginBtn").disabled = false;
  }
  const { ok, data } = reply;
  if (!ok) { $("loginMsg").textContent = data.error ?? "Sign-in failed."; return; }
  await load(true);
  schedule();
  loadCredentials();
});

window.addEventListener("beforeunload", (event) => { if (dirty()) { event.preventDefault(); event.returnValue = ""; } });

$("logout").addEventListener("click", async () => {
  if (dirty() && !window.confirm("There are changes you have not saved. Sign out and lose them?")) return;
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

let saving = false;
async function save() {
  if (saving) return false;
  const config = collect();
  if (directChanges() && !needUnlock("switch to or from Direct")) return false;
  if (config.public === true && saved.public !== true
    && !window.confirm("Direct internet access lets anyone who finds this address reach the login page. Only continue if the router forwarding and the login are set up as described here. Save it?")) {
    $("saveNote").textContent = "Not saved.";
    return false;
  }
  saving = true;
  $("save").disabled = $("restart").disabled = true;
  let reply;
  try { reply = await api("POST", "/host/config", config); } finally { saving = false; }
  const { ok, data } = reply;
  updateDirty();
  cardNote("internetCard", ok ? "" : (data.error ?? "Could not save."));
  $("saveNote").textContent = ok ? "Saved." : (data.error ?? "Could not save.");
  if (ok) await load(true);
  return ok;
}

$("save").addEventListener("click", save);

let allowing = false;
$("inputAllow").addEventListener("click", async () => {
  if (allowing) return;
  allowing = true;
  $("inputAllow").disabled = true;
  cardNote("inputCard", "A password dialog is waiting on this laptop's screen. Answer it there.");
  const reply = await api("POST", "/host/inputaccess", {});
  allowing = false;
  $("inputAllow").disabled = false;
  if (!reply.ok) { cardNote("inputCard", reply.data.error ?? "That did not work."); return; }
  cardNote("inputCard", "");
  $("saveNote").textContent = "Keyboard blocking is allowed until the laptop restarts.";
  renderInput({ input: reply.data.input });
});

$("copyVpn").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText($("vpnCommands").textContent);
    $("saveNote").textContent = "Commands copied. Run them in a terminal on this laptop.";
  } catch {
    window.getSelection().selectAllChildren($("vpnCommands"));
    $("saveNote").textContent = "Select the commands above and copy them.";
  }
});

$("restart").addEventListener("click", async () => {
  // The laptop refuses to end a running session without `confirm`: it is sent only after the owner agreed.
  const warning = "A remote session is running. Restarting the console ends it. Continue?";
  let confirmed = false;
  if (state && state.status.phase !== "idle") {
    if (!window.confirm(warning)) return;
    confirmed = true;
  }
  if (dirty() && !(await save())) return;
  let reply = await api("POST", "/host/restart", { confirm: confirmed });
  if (reply.status === 409) {
    if (!window.confirm(warning)) { $("saveNote").textContent = "Not restarted: a remote session is running."; return; }
    reply = await api("POST", "/host/restart", { confirm: true });
  }
  const { ok, data } = reply;
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
  cardNote("laptopCard", ok ? "" : (data.error ?? "Could not change it."));
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
  if (wanted && !needUnlock("turn on remote use on the lock screen")) { $("lockOn").checked = false; return; }
  if (wanted && !window.confirm("Allow remote sessions on the lock screen? While this is on, locking the laptop no longer ends a remote session.")) { $("lockOn").checked = false; return; }
  const reply = await api("POST", "/host/lockscreen", { enabled: wanted });
  if (!reply.ok) { $("lockOn").checked = !wanted; $("saveNote").textContent = reply.data.error ?? "That did not work."; cardNote("lockCard", reply.data.error ?? "That did not work."); return; }
  cardNote("lockCard", "");
  $("saveNote").textContent = wanted ? "Remote use on the lock screen is on." : "Remote use on the lock screen is off. A session already open on a locked screen keeps running until it ends.";
  await load(false);
});

// ---- "Enable editing": one password for the whole page, good for five minutes ----
let unlockUntil = 0;   // when editing locks again (ms since 1970); the laptop is the authority, this only shows it
let editTicker = 0;
function unlocked() { return Date.now() < unlockUntil; }
function setUnlock(secs) {
  const was = unlocked();
  unlockUntil = secs > 0 ? Date.now() + secs * 1000 : 0;
  clearInterval(editTicker);
  if (secs > 0) editTicker = setInterval(renderEdit, 1000);
  renderEdit();
  if (was && !unlocked()) $("editError").hidden = true;
}
function renderEdit() {
  const on = unlocked();
  const left = Math.max(0, Math.ceil((unlockUntil - Date.now()) / 1000));
  $("editForm").hidden = on;
  $("editActive").hidden = !on;
  $("editBar").classList.toggle("open", on);
  $("editTitle").textContent = on ? "Editing enabled" : "Editing locked";
  $("editHelp").textContent = on ? "Protected changes need no password until the time runs out." : "Enable editing once to change protected settings. It stays on for 5 minutes.";
  $("editCountdown").textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
  if (!on && unlockUntil !== 0) { unlockUntil = 0; clearInterval(editTicker); }
}
// A protected action pressed while locked does nothing but say how to unlock.
function needUnlock(what) {
  if (unlocked()) return true;
  $("editError").textContent = `Enable editing to ${what}.`;
  $("editError").hidden = false;
  $("saveNote").textContent = `Enable editing to ${what}.`;
  $("editBar").scrollIntoView({ block: "center" });
  $("editPassword").focus();
  return false;
}
$("editForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  $("editEnable").disabled = true;
  let reply;
  try { reply = await api("POST", "/host/unlock", { password: $("editPassword").value }); } finally {
    $("editPassword").value = "";
    $("editEnable").disabled = false;
  }
  if (!reply.ok) { $("editError").textContent = reply.data.error ?? "That did not work."; $("editError").hidden = false; return; }
  $("editError").hidden = true;
  $("saveNote").textContent = "Editing is enabled for 5 minutes.";
  setUnlock(reply.data.secs_left);
});
$("editLock").addEventListener("click", async () => {
  setUnlock(0);   // protected requests stop at once, even before the laptop answers
  const reply = await api("POST", "/host/lock", {}).catch(() => ({ ok: false, data: {} }));
  $("saveNote").textContent = reply.ok ? "Editing is locked." : "Could not confirm that the laptop locked editing.";
});
document.addEventListener("visibilitychange", () => { if (!document.hidden && state) load(false).catch(() => {}); });

$("restartOn").addEventListener("change", async () => {
  const wanted = $("restartOn").checked;
  if (restartBusy) { $("restartOn").checked = !wanted; return; }
  if (wanted && !needUnlock("turn on restart access")) { $("restartOn").checked = false; return; }
  if (wanted && !window.confirm("After a restart this laptop will log in by itself and lock the screen, and remote use on the lock screen will be on. Continue?")) { $("restartOn").checked = false; return; }
  restartBusy = true;
  $("restartOn").disabled = true;
  if (wanted) cardNote("restartCard", "A password dialog is waiting on this laptop's screen. Answer it there.");
  let reply;
  try { reply = await api("POST", "/host/restartaccess", { enabled: wanted }); } finally { restartBusy = false; $("restartOn").disabled = false; }
  if (!reply.ok) { $("restartOn").checked = !wanted; cardNote("restartCard", reply.data.error ?? "That did not work."); await load(false); return; }
  cardNote("restartCard", "");
  $("saveNote").textContent = wanted ? "The laptop will log in and lock itself after a restart." : "Restart access is off. Remote use on the lock screen stays as set above.";
  await load(false);
});

// ---- authenticator app: scan or type the key, then one right code stores it ----
let totpTimer = 0;
function closeTotp() {
  clearTimeout(totpTimer);
  $("totpBox").hidden = true;
  $("totpQr").removeAttribute("src");
  $("totpSecret").textContent = "";
  $("totpCode").value = "";
  $("totpMsg").textContent = "";
}

$("totpStart").addEventListener("click", async () => {
  if (!needUnlock("change the authenticator")) return;
  const reply = await api("POST", "/host/totp/start", {});
  if (!reply.ok) { $("saveNote").textContent = reply.data.error ?? "That did not work."; cardNote("totpCard", reply.data.error ?? "That did not work."); return; }
  cardNote("totpCard", "");
  $("totpQr").src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(reply.data.svg);
  $("totpSecret").textContent = reply.data.secret;
  $("totpAccount").textContent = "Blackroom Console:" + reply.data.account;
  $("totpMsg").textContent = "Waiting for a code from the app (this setup expires in " + Math.round(reply.data.expires_secs / 60) + " minutes).";
  $("totpBox").hidden = false;
  $("totpCode").focus();
  // The laptop drops the setup after expires_secs: do not leave a dead QR code and key on screen.
  totpTimer = setTimeout(() => { closeTotp(); $("saveNote").textContent = "The authenticator setup expired. Start again."; }, reply.data.expires_secs * 1000);
});

$("totpVerify").addEventListener("click", async () => {
  const reply = await api("POST", "/host/totp/verify", { code: $("totpCode").value.trim() });
  if (reply.ok) {
    closeTotp();
    $("saveNote").textContent = reply.data.authority_restarted
      ? "Authenticator confirmed and saved. Clients now need a code from this app (the login authority was restarted; remote logins ended)."
      : "Authenticator confirmed and saved, but the login authority was not restarted: until it is (systemctl --user restart remote-hostd), clients are still checked against the old authenticator.";
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

// An action's own outcome shows inside its card (the save bar at the bottom may be off screen); empty text clears it.
function cardNote(cardId, text) {
  const card = $(cardId);
  let note = card.querySelector(".actionnote");
  if (!note) {
    note = document.createElement("p");
    note.className = "note warn actionnote";
    note.setAttribute("role", "alert");
    card.querySelector("h3").after(note);
  }
  note.textContent = text;
  note.hidden = text === "";
}

// One credential command at a time: its buttons stay off until the laptop answers.
let credBusy = false;
async function credential(action, extra = {}, needsPassword = true) {
  if (needsPassword && credBusy) return null;
  const body = { action, ...extra };
  // Reads (status, devices) may run together; a change holds the card's buttons until the laptop answers.
  const buttons = needsPassword ? [...$("credCard").querySelectorAll("button")] : [];
  if (needsPassword) credBusy = true;
  buttons.forEach((button) => { button.disabled = true; });
  let reply;
  try { reply = await api("POST", "/host/credentials", body); } finally {
    if (needsPassword) credBusy = false;
    buttons.forEach((button) => { button.disabled = false; });
  }
  if (!reply.ok) { const text = reply.data.error ?? "That did not work."; $("saveNote").textContent = text; cardNote("credCard", text); return null; }
  cardNote("credCard", "");
  return reply.data;
}

async function loadCredentials() {
  const [status, devices] = await Promise.all([credential("status", {}, false), credential("devices", {}, false)]);
  renderDevices(((devices && devices.output) || "").trim());
  const text = [status].filter(Boolean).map((part) => (part.output || part.notice || "").trim()).filter(Boolean).join("\n\n");
  $("credStatus").textContent = text || "Nothing to show: the login authority is not set up (run blackroom setup).";
}

async function change(action, extra, question) {
  if (!needUnlock("change sign-in settings")) return;
  if (question && !window.confirm(question)) return;
  const data = await credential(action, extra);
  if (!data) return;
  const out = [data.output.trim(), data.notice].filter(Boolean).join("\n\n");
  $("saveNote").textContent = data.ok ? "Done." : "The command reported a problem.";
  cardNote("credCard", data.ok ? "" : "The command reported a problem. Read its output below.");
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
